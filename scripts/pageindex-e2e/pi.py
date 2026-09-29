#!/usr/bin/env python3
"""Minimal client for Xavier's MCP bridge (POST /mcp/tools/call) on an isolated e2e instance.
Env: PI_URL (default http://127.0.0.1:18006/mcp/tools/call), XAVIER_TOKEN or PI_TOKEN_FILE, PI_LOG.
Usage: pi.py <tool_name> '<json arguments>'   e.g. pi.py pageindex_get_document_structure '{"doc_name":"x.pdf"}'
Every call is appended to $PI_LOG (JSONL) so page reads can be audited."""
import json, os, sys, time, urllib.request
HERE = os.path.dirname(os.path.abspath(__file__))
# Token: XAVIER_TOKEN env var, else the file named by PI_TOKEN_FILE (default: ./token, git-ignored).
TOKEN = os.environ.get("XAVIER_TOKEN") or open(os.environ.get("PI_TOKEN_FILE", os.path.join(HERE, "token"))).read().strip()
URL = os.environ.get("PI_URL", "http://127.0.0.1:18006/mcp/tools/call")
def call(name, args):
    req = urllib.request.Request(URL, data=json.dumps({"name": name, "arguments": args}).encode(),
        headers={"Content-Type": "application/json", "X-Xavier-Token": TOKEN})
    with urllib.request.urlopen(req, timeout=600) as r:
        out = json.loads(r.read())
    log = os.environ.get("PI_LOG")
    if log:
        with open(log, "a") as f:
            f.write(json.dumps({"t": time.time(), "tool": name, "args": args}) + "\n")
    return out
if __name__ == "__main__":
    print(json.dumps(call(sys.argv[1], json.loads(sys.argv[2]) if len(sys.argv) > 2 else {}), ensure_ascii=False, indent=1))
