#!/bin/bash
# ==============================================================================
# Automated Test Suite for scripts/reindex-embeddings.sh
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
REINDEX_SCRIPT="$REPO_ROOT/scripts/reindex-embeddings.sh"

echo "=== Running reindex-embeddings.sh Test Suite ==="

# 1. Verify --help option
echo ">> Test 1: --help option"
HELP_OUT=$("$REINDEX_SCRIPT" --help)
if [[ "$HELP_OUT" != *"Usage:"* ]]; then
    echo "[FAIL] --help output invalid"
    exit 1
fi
echo "[PASS] --help option verified"

# 2. Integration test against Mock HTTP Server
echo ">> Test 2: Mock HTTP server integration (--dry-run, --limit, lockfile concurrency)"

python3 -c '
import http.server, json, threading, time, subprocess, sys, os

class MockHandler(http.server.BaseHTTPRequestHandler):
    def log_message(self, format, *args):
        pass

    def do_POST(self):
        content_length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(content_length)
        payload = json.loads(body.decode("utf-8"))
        token = self.headers.get("X-Xavier-Token")

        if token != "test-token-123":
            self.send_response(401)
            self.end_headers()
            self.wfile.write(b"Unauthorized")
            return

        dry_run = payload.get("dry_run", True)
        limit = payload.get("limit", 0)

        res = {
            "status": "ok" if dry_run else "reindexing_started",
            "dry_run": dry_run,
            "null_embeddings_count": 100,
            "processed_count": 0 if dry_run else min(limit, 100)
        }

        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.end_headers()
        self.wfile.write(json.dumps(res).encode("utf-8"))

server = http.server.ThreadingHTTPServer(("127.0.0.1", 19006), MockHandler)
thread = threading.Thread(target=server.serve_forever)
thread.daemon = True
thread.start()
time.sleep(0.2)

env = os.environ.copy()
env["XAVIER_TOKEN"] = "test-token-123"
reindex_script = os.environ["REINDEX_SCRIPT"]

# Test A: --dry-run
p_dry = subprocess.run([reindex_script, "--dry-run", "--url", "http://localhost:19006", "--lockfile", "/tmp/test-reindex-1.lock"], env=env, capture_output=True, text=True)
assert p_dry.returncode == 0, f"Dry run failed: {p_dry.stderr}"
assert "Memories lacking embeddings (null count): 100" in p_dry.stdout, f"Dry run output mismatch: {p_dry.stdout}"

# Test B: --limit 50
p_limit = subprocess.run([reindex_script, "--limit", "50", "--url", "http://localhost:19006", "--lockfile", "/tmp/test-reindex-2.lock"], env=env, capture_output=True, text=True)
assert p_limit.returncode == 0, f"Limit run failed: {p_limit.stderr}"
assert "Reindex batch triggered for up to 50 memories" in p_limit.stdout, f"Limit run output mismatch: {p_limit.stdout}"
assert "Processed count: 50" in p_limit.stdout, f"Processed count mismatch: {p_limit.stdout}"

# Test C: Lockfile Concurrency Rejection
lock_path = "/tmp/test-reindex-concurrent.lock"
if os.path.exists(lock_path):
    os.remove(lock_path)

lock_fd = open(lock_path, "w")
import fcntl
fcntl.flock(lock_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)

p_lock = subprocess.run([reindex_script, "--dry-run", "--url", "http://localhost:19006", "--lockfile", lock_path], env=env, capture_output=True, text=True)
assert p_lock.returncode != 0, "Script should have failed when lockfile was held"
assert "Another reindexing process is currently running" in p_lock.stderr, f"Lock rejection stderr mismatch: {p_lock.stderr}"

fcntl.flock(lock_fd, fcntl.LOCK_UN)
lock_fd.close()
if os.path.exists(lock_path):
    os.remove(lock_path)

server.shutdown()
print("[PASS] All mock server tests passed successfully!")
'

echo "=== All Tests Passed ==="
