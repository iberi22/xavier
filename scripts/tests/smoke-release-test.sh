#!/usr/bin/env bash
set -euo pipefail

# Test suite for release script scripts/smoke-release.sh

SCRIPT_PATH="$(pwd)/scripts/smoke-release.sh"
TEMP_DIR=$(mktemp -d)

cleanup() {
    local exit_code=$?
    rm -rf "$TEMP_DIR"
    exit $exit_code
}
trap cleanup EXIT

echo "Running smoke-release-test.sh..."

setup_fake_binary() {
    local script_file="$1"
    local healthy="$2"
    local allow_unauth="$3"

    cat << PYTHON > "$script_file"
#!/usr/bin/env python3
import sys
import json
import threading
from http.server import HTTPServer, BaseHTTPRequestHandler

HEALTHY = "$healthy" == "1"
ALLOW_UNAUTH = "$allow_unauth" == "1"

class FakeXavierHandler(BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        pass

    def send_json(self, code, data):
        self.send_response(code)
        self.send_header('Content-type', 'application/json')
        self.end_headers()
        self.wfile.write(json.dumps(data).encode('utf-8'))

    def do_GET(self):
        if self.path == '/health':
            if HEALTHY:
                self.send_json(200, {"status": "ok"})
            else:
                self.send_json(503, {"status": "error"})
        else:
            self.send_response(404)
            self.end_headers()

    def do_POST(self):
        token = self.headers.get('X-Xavier-Token')

        if self.path == '/v1/memory/add':
            if not token and not ALLOW_UNAUTH:
                self.send_json(401, {"error": "unauthorized"})
                return
            self.send_json(200, {"id": "mem_123"})

        elif self.path == '/v1/memory/search':
            if not token:
                self.send_json(401, {"error": "unauthorized"})
                return
            self.send_json(200, {"results": [{"content": "smoke memory test"}]})

        elif self.path == '/mcp':
            if not token:
                self.send_json(401, {"error": "unauthorized"})
                return
            content_len = int(self.headers.get('Content-Length', 0))
            if content_len > 0:
                body = self.rfile.read(content_len)
                try:
                    data = json.loads(body)
                    if data.get('method') == 'tools/list':
                        self.send_json(200, {"jsonrpc": "2.0", "id": data.get('id'), "result": {"tools": [{"name": "fake_tool"}]}})
                        return
                except:
                    pass
            self.send_json(400, {"error": "bad request"})

        else:
            self.send_response(404)
            self.end_headers()

if __name__ == '__main__':
    if len(sys.argv) > 1 and sys.argv[1] == 'http':
        import os
        port = int(os.environ.get('XAVIER_PORT', 8006))
        mcp_port = int(os.environ.get('XAVIER_MCP_PORT', 8100))

        def run_mcp():
            server2 = HTTPServer(('127.0.0.1', mcp_port), FakeXavierHandler)
            server2.serve_forever()

        t = threading.Thread(target=run_mcp, daemon=True)
        t.start()

        server = HTTPServer(('127.0.0.1', port), FakeXavierHandler)
        server.serve_forever()
    else:
        sys.exit(1)
PYTHON
    chmod +x "$script_file"
}

# Test 1: PASS path
echo "Test 1: PASS path"
FAKE_BIN="$TEMP_DIR/fake_binary_pass.py"
setup_fake_binary "$FAKE_BIN" "1" "0"

bash "$SCRIPT_PATH" "$FAKE_BIN" > "$TEMP_DIR/out_pass.log" 2>&1
grep -q "SMOKE: PASS 5/5" "$TEMP_DIR/out_pass.log" || {
    echo "Test 1 failed. Output:"
    cat "$TEMP_DIR/out_pass.log"
    exit 1
}

# Ensure cleanup happened - check if any python http server is still running on that port
# The script cleans up TEMP_DIR but also the process. We can't easily check port, but we can verify no such process exists
# because it was backgrounded in the script and killed in trap.

# Test 2: FAIL on a broken health
echo "Test 2: FAIL on a broken health"
FAKE_BIN_HEALTH="$TEMP_DIR/fake_binary_health.py"
setup_fake_binary "$FAKE_BIN_HEALTH" "0" "0"

if bash "$SCRIPT_PATH" "$FAKE_BIN_HEALTH" > "$TEMP_DIR/out_health.log" 2>&1; then
    echo "Test 2 failed (should have exited non-zero). Output:"
    cat "$TEMP_DIR/out_health.log"
    exit 1
fi
grep -q "SMOKE: FAIL health" "$TEMP_DIR/out_health.log" || {
    echo "Test 2 failed to print correct error. Output:"
    cat "$TEMP_DIR/out_health.log"
    exit 1
}

# Test 3: FAIL when unauthenticated write is accepted
echo "Test 3: FAIL when unauth write accepted"
FAKE_BIN_UNAUTH="$TEMP_DIR/fake_binary_unauth.py"
setup_fake_binary "$FAKE_BIN_UNAUTH" "1" "1"

if bash "$SCRIPT_PATH" "$FAKE_BIN_UNAUTH" > "$TEMP_DIR/out_unauth.log" 2>&1; then
    echo "Test 3 failed (should have exited non-zero). Output:"
    cat "$TEMP_DIR/out_unauth.log"
    exit 1
fi
grep -q "SMOKE: FAIL unauthenticated write" "$TEMP_DIR/out_unauth.log" || {
    echo "Test 3 failed to print correct error. Output:"
    cat "$TEMP_DIR/out_unauth.log"
    exit 1
}

# Test 4: cleanup happens (no temp dir left)
echo "Test 4: Cleanup happens"
# We run the script in a subshell, let it create its own temp dir,
# then after it fails (or succeeds), verify that no directories matching 'tmp.*' created recently exist.
# The script's output tells us what dir it created if we hack it, but standard mktemp -d is hard to track from outside.
# However, we can use the pass test and verify no processes are left.
PIDS=$(pgrep -f "fake_binary" || true)
if [[ -n "$PIDS" ]]; then
    echo "Test 4 failed - found leaking processes:"
    echo "$PIDS"
    kill -9 $PIDS || true
    exit 1
fi

echo "All tests passed."
