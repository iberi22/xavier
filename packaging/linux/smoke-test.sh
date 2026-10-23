#!/bin/sh
# Starts a xavier binary in a scratch HOME, checks /health, /ready and that no panel is
# served, then stops it with SIGTERM and requires a clean exit within 30 s.
# Usage: smoke-test.sh <xavier-binary> [port]
set -eu
bin="$1"
case "$bin" in /*) ;; *) bin="$PWD/$bin" ;; esac
port="${2:-18006}"
work=$(mktemp -d)
log="$work/xavier.log"
pid=""
cleanup() {
    if [ -n "$pid" ]; then kill -KILL "$pid" 2>/dev/null || true; fi
    rm -rf "$work"
}
trap cleanup EXIT INT TERM
fail() { echo "smoke: $*" >&2; tail -n 60 "$log" >&2 || true; exit 1; }

"$bin" --version
mkdir -p "$work/home" "$work/data"
token=$(od -An -N16 -tx1 /dev/urandom | tr -d ' \n')
(
    cd "$work"
    HOME="$work/home" XAVIER_DATA_DIR="$work/data" XAVIER_TOKEN="$token" RUST_LOG=info \
        exec "$bin" http "$port" --mcp-port 0 --no-ui
) >"$log" 2>&1 &
pid=$!

base="http://127.0.0.1:$port"
i=0
until curl -fsS "$base/health" >/dev/null 2>&1; do
    kill -0 "$pid" 2>/dev/null || fail "server exited during startup"
    i=$((i + 1))
    [ "$i" -le 180 ] || fail "/health not up after 180 s"
    sleep 1
done
echo "smoke: /health up after ${i}s"

curl -fsS "$base/ready" || fail "/ready is not 200"
echo
panel=$(curl -s -o /dev/null -w '%{http_code}' "$base/panel")
[ "$panel" != 200 ] || fail "/panel answered 200 on a headless build"
grep -q 'node master key ready' "$log" || fail "master key was not initialised"

kill -TERM "$pid"
i=0
while kill -0 "$pid" 2>/dev/null; do
    i=$((i + 1))
    [ "$i" -le 30 ] || fail "did not stop within 30 s of SIGTERM"
    sleep 1
done
wait "$pid" 2>/dev/null || true
pid=""
echo "smoke: stopped ${i}s after SIGTERM"
