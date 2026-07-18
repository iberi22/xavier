#!/usr/bin/env python3
"""Measure progressive-disclosure token savings for Xavier MCP.

Compares fat mem_search (include_content=false) + memory_context(ids)
against a naive full-content dump of the same candidates.

Usage:
  set XAVIER_URL=http://localhost:8006
  set XAVIER_TOKEN=...
  python scripts/measure_token_savings.py --query "architecture decisions" --limit 10

Exit 0 always if the server answers; prints a JSON summary to stdout.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
import urllib.request


def estimate_tokens(text: str) -> int:
    """Match Xavier estimate_tokens heuristic (~4 chars/token)."""
    if not text:
        return 0
    return max(1, (len(text) + 3) // 4)


def rpc(url: str, token: str, method: str, params: dict) -> dict:
    body = json.dumps(
        {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}
    ).encode("utf-8")
    req = urllib.request.Request(
        url.rstrip("/") + "/mcp",
        data=body,
        headers={
            "Content-Type": "application/json",
            "X-Xavier-Token": token,
        },
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=60) as resp:
        return json.loads(resp.read().decode("utf-8"))


def tool_call(url: str, token: str, name: str, arguments: dict) -> dict:
    return rpc(
        url,
        token,
        "tools/call",
        {"name": name, "arguments": arguments},
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url", default=os.environ.get("XAVIER_URL", "http://localhost:8006"))
    parser.add_argument("--token", default=os.environ.get("XAVIER_TOKEN", ""))
    parser.add_argument("--query", default="memory architecture")
    parser.add_argument("--limit", type=int, default=10)
    parser.add_argument("--max-chars", type=int, default=4000)
    args = parser.parse_args()

    if not args.token:
        print(
            json.dumps(
                {
                    "ok": False,
                    "error": "XAVIER_TOKEN required",
                    "hint": "set XAVIER_TOKEN or pass --token",
                }
            ),
            file=sys.stderr,
        )
        return 2

    try:
        fat = tool_call(
            args.url,
            args.token,
            "mem_search",
            {
                "query": args.query,
                "limit": args.limit,
                "include_content": False,
            },
        )
        fat_raw = json.dumps(fat, ensure_ascii=False)
        fat_tokens = estimate_tokens(fat_raw)

        # Extract candidate ids if structured payload present
        ids: list[str] = []
        result = fat.get("result") or fat
        # structuredContent / content text variants
        candidates = None
        if isinstance(result, dict):
            sc = result.get("structuredContent") or result.get("structured_content")
            if isinstance(sc, dict):
                candidates = sc.get("candidates") or sc.get("results")
            if candidates is None and "content" in result:
                # may be MCP content array
                pass
        if isinstance(candidates, list):
            for c in candidates:
                if isinstance(c, dict) and c.get("id"):
                    ids.append(str(c["id"]))

        page_in = None
        page_tokens = 0
        if ids:
            page_in = tool_call(
                args.url,
                args.token,
                "memory_context",
                {
                    "ids": ids[: min(5, len(ids))],
                    "max_chars": args.max_chars,
                    "max_chars_per_doc": min(800, args.max_chars),
                },
            )
            page_tokens = estimate_tokens(json.dumps(page_in, ensure_ascii=False))

        naive = tool_call(
            args.url,
            args.token,
            "mem_search",
            {
                "query": args.query,
                "limit": args.limit,
                "include_content": True,
            },
        )
        naive_tokens = estimate_tokens(json.dumps(naive, ensure_ascii=False))

        progressive = fat_tokens + page_tokens
        savings_pct = (
            round(100.0 * (1.0 - progressive / naive_tokens), 2) if naive_tokens else 0.0
        )

        out = {
            "ok": True,
            "query": args.query,
            "limit": args.limit,
            "ids_page_in": ids[:5],
            "tokens": {
                "fat_mem_search": fat_tokens,
                "memory_context_page_in": page_tokens,
                "progressive_total": progressive,
                "naive_include_content": naive_tokens,
            },
            "savings_pct_vs_naive": savings_pct,
            "target_pct": 90,
            "meets_target": savings_pct >= 90,
        }
        print(json.dumps(out, indent=2))
        return 0
    except urllib.error.URLError as exc:
        print(json.dumps({"ok": False, "error": str(exc)}), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
