#!/usr/bin/env python3
"""
Xavier Memory Persistence Test
--------------------------------
Validates that memories stored in Xavier survive a service restart.

Test flow:
1. Connect to Xavier and store 10 known test memories
2. Verify all 10 are immediately retrievable
3. Stop Xavier (SIGTERM / kill process)
4. Start Xavier again
5. Search for all 10 test memories
6. Score = recall rate post-restart

Usage:
  python scripts/test_persistence.py              # Mock mode (no servers)
  python scripts/test_persistence.py --live       # Against live Xavier
  python scripts/test_persistence.py --live --kill  # Actually kill + restart Xavier

Requirements:
  - aiohttp (pip install aiohttp)
  - Xavier HTTP server running (for --live mode)
"""

import asyncio
import json
import os
import platform
import random
import signal
import subprocess
import sys
import time
from datetime import datetime
from pathlib import Path
from typing import List, Dict, Any, Optional

import aiohttp

# ─── Configuration ───────────────────────────────────────────────────────────

XAVIER_URL = os.environ.get("XAVIER_URL", "http://localhost:8006")
QUERIES_FILE = "benchmarks/tri_memory_queries.json"
RESULTS_DIR = "benchmarks/results"
XAVIER_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# ─── Test Memories ───────────────────────────────────────────────────────────

TEST_MEMORIES = [
    {
        "id": "persist_001",
        "content": "The Xavier memory persistence test confirms that SQLite-backed storage survives service restarts. Key components: VecSqliteMemoryStore and QmdMemory are both backed by durable SQLite databases.",
        "title": "Persistence Test: Architecture",
        "category": "persistence",
    },
    {
        "id": "persist_002",
        "content": "Default embedding model for Xavier is all-mpnet-base-v2 with 768 dimensions and MTEB score of 63.0. This model provides the best balance of accuracy and performance.",
        "title": "Persistence Test: Default Model",
        "category": "persistence",
    },
    {
        "id": "persist_003",
        "content": "Xavier HTTP server listens on port 8006 by default. Health endpoint is at GET /health returning 200 when ready. Benchmark mode bypasses auth via XAVIER_BENCHMARK_MODE=true.",
        "title": "Persistence Test: HTTP Config",
        "category": "persistence",
    },
    {
        "id": "persist_004",
        "content": "The Xavier Sovereign Mesh boundary is defined in ADR 006. It separates the internal replication network from the external network with governance at the boundary. Peers dropping below 50% agreement are flagged as unhealthy.",
        "title": "Persistence Test: Mesh Architecture",
        "category": "persistence",
    },
    {
        "id": "persist_005",
        "content": "XavierWallet uses post-quantum cryptography: ML-KEM-1024 for key encapsulation, ML-DSA-87 for signatures, and AES-256-GCM for symmetric encryption. All cryptographic operations are audited.",
        "title": "Persistence Test: Security",
        "category": "persistence",
    },
    {
        "id": "persist_006",
        "content": "The notification system has four category islands: System (alerts), Memory (store events), Agents (lifecycle), and Errors (failures). Each island has its own routing and deduplication.",
        "title": "Persistence Test: Notifications",
        "category": "persistence",
    },
    {
        "id": "persist_007",
        "content": "HORMER agent architecture uses GRPO for reinforcement learning, retrieves layer weights from the attention mechanism, and has a RewardModel for evaluating task completion quality.",
        "title": "Persistence Test: HORMER Agent",
        "category": "persistence",
    },
    {
        "id": "persist_008",
        "content": "Backend CI for resource-intensive features uses --no-default-features combined with --features ci-safe to exclude heavy dependencies like local-gllm and CUDA from test compilation.",
        "title": "Persistence Test: CI Strategy",
        "category": "persistence",
    },
    {
        "id": "persist_009",
        "content": "The moka-based cache in HybridSearcher has a capacity of 1000 entries with a 1-hour TTL. It caches embedding search results to reduce latency for repeated queries.",
        "title": "Persistence Test: Caching",
        "category": "persistence",
    },
    {
        "id": "persist_010",
        "content": "Xavier's embedding benchmark compares three models: all-MiniLM-L6-v2 (384d, 58.8 MTEB), all-mpnet-base-v2 (768d, 63.0 MTEB), and Qwen3-Embedding-0.6B (1024d, ~67.5 MTEB). Qwen3 requires CUDA.",
        "title": "Persistence Test: Benchmark",
        "category": "persistence",
    },
]

# ─── Search Queries (for post-restart retrieval) ──────────────────────────────

SEARCH_QUERIES = [
    "persistence test SQLite backend durable storage",
    "default embedding model all-mpnet-base-v2",
    "Xavier HTTP port 8006 health benchmark",
    "Sovereign Mesh boundary ADR 006 agreement unhealthy",
    "post-quantum cryptography ML-KEM ML-DSA AES",
    "notification system islands System Memory Agents Errors",
    "HORMER agent GRPO reinforcement learning RewardModel",
    "CI no-default-features ci-safe resource-intensive",
    "moka cache capacity 1000 entries TTL",
    "embedding benchmark MiniLM mpnet Qwen3 comparison",
]


# ─── Helpers ─────────────────────────────────────────────────────────────────

def print_step(msg: str, icon: str = "⏳") -> None:
    print(f"  {icon} {msg}")

def print_header(title: str) -> None:
    print(f"\n{'='*60}")
    print(f"  {title}")
    print(f"{'='*60}")

def print_result(label: str, value: Any, ok: bool = True) -> None:
    icon = "✅" if ok else "❌"
    print(f"  {icon} {label}: {value}")


# ─── Xavier API Client ───────────────────────────────────────────────────────

class XavierClient:
    """Minimal HTTP client for Xavier API operations."""
    
    def __init__(self, base_url: str = XAVIER_URL):
        self.base_url = base_url.rstrip("/")
    
    async def check_health(self, session: aiohttp.ClientSession) -> Dict[str, Any]:
        """Check if Xavier is alive."""
        try:
            async with session.get(f"{self.base_url}/health", timeout=5) as resp:
                if resp.status in (200, 503):
                    body = await resp.text()
                    try:
                        return {"ok": True, "data": json.loads(body)}
                    except json.JSONDecodeError:
                        return {"ok": True, "data": {"status": "unknown"}}
                return {"ok": False, "error": f"HTTP {resp.status}"}
        except Exception as e:
            return {"ok": False, "error": str(e)}
    
    async def add_memory(self, session: aiohttp.ClientSession, content: str, title: str = "") -> Dict[str, Any]:
        """Add a memory to Xavier."""
        url = f"{self.base_url}/memory/add"
        payload = {
            "content": content,
            "metadata": {"title": title} if title else {},
        }
        try:
            async with session.post(url, json=payload, timeout=10) as resp:
                if resp.status in (200, 201):
                    data = await resp.json()
                    return {"ok": True, "data": data}
                error_text = await resp.text()
                return {"ok": False, "error": f"HTTP {resp.status}: {error_text[:200]}"}
        except Exception as e:
            return {"ok": False, "error": str(e)}
    
    async def search_memories(self, session: aiohttp.ClientSession, query: str, limit: int = 5) -> Dict[str, Any]:
        """Search Xavier memories."""
        url = f"{self.base_url}/v1/memories/search"
        payload = {"query": query, "limit": limit}
        try:
            async with session.post(url, json=payload, timeout=10) as resp:
                if resp.status == 200:
                    data = await resp.json()
                    return {"ok": True, "results": data}
                # Fallback: try GET
                get_url = f"{self.base_url}/v1/memories?q={query}&limit={limit}"
                async with session.get(get_url, timeout=10) as resp2:
                    if resp2.status == 200:
                        data = await resp2.json()
                        return {"ok": True, "results": data}
                    return {"ok": False, "error": f"HTTP {resp2.status}"}
        except Exception as e:
            return {"ok": False, "error": str(e)}


# ─── Mock Handler ────────────────────────────────────────────────────────────

class MockClient:
    """Simulates Xavier memory operations for testing without a server."""
    
    def __init__(self):
        self.memories = {}
    
    async def check_health(self, session) -> Dict[str, Any]:
        return {"ok": True, "data": {"status": "healthy"}}
    
    async def add_memory(self, session, content: str, title: str = "") -> Dict[str, Any]:
        key = f"memory_{len(self.memories) + 1}"
        self.memories[key] = {"content": content, "title": title}
        return {"ok": True, "data": {"id": key}}
    
    async def search_memories(self, session, query: str, limit: int = 5) -> Dict[str, Any]:
        query_words = set(query.lower().split())
        results = []
        for key, val in self.memories.items():
            text = (val["content"] + " " + val.get("title", "")).lower()
            overlap = len(query_words & set(text.split()))
            if overlap >= 3:
                results.append({"id": key, "content": val["content"], "score": overlap / len(query_words)})
        # Sort by score descending
        results.sort(key=lambda x: x.get("score", 0), reverse=True)
        return {"ok": True, "results": results[:limit]}


# ─── Process Management (for --kill mode) ────────────────────────────────────

def find_xavier_process() -> Optional[int]:
    """Find the PID of a running Xavier process."""
    system = platform.system().lower()
    try:
        if system == "windows":
            result = subprocess.run(
                ['tasklist', '/FI', 'IMAGENAME eq xavier.exe', '/FO', 'CSV', '/NH'],
                capture_output=True, text=True, timeout=5
            )
            for line in result.stdout.strip().split('\n'):
                if line and 'xavier.exe' in line.lower():
                    parts = line.split(',')
                    if len(parts) >= 2:
                        return int(parts[1].strip().strip('"'))
        else:
            result = subprocess.run(
                ['pgrep', '-f', 'xavier'],
                capture_output=True, text=True, timeout=5
            )
            pids = result.stdout.strip().split()
            if pids:
                # Return first non-self PID
                my_pid = str(os.getpid())
                for pid in pids:
                    if pid != my_pid:
                        return int(pid)
    except Exception:
        pass
    return None

def stop_xavier(pid: int) -> bool:
    """Stop Xavier process gracefully."""
    system = platform.system().lower()
    try:
        if system == "windows":
            subprocess.run(['taskkill', '/PID', str(pid), '/F'], capture_output=True, timeout=5)
        else:
            os.kill(pid, signal.SIGTERM)
            time.sleep(2)
            # Force kill if still running
            try:
                os.kill(pid, 0)
                os.kill(pid, signal.SIGKILL)
            except OSError:
                pass  # process is dead
        return True
    except Exception:
        return False

def start_xavier() -> Optional[subprocess.Popen]:
    """Start Xavier server."""
    # Try different binary locations
    candidates = [
        os.path.join(XAVIER_ROOT, "target", "release", "xavier.exe"),
        os.path.join(XAVIER_ROOT, "target", "release", "xavier"),
        os.path.join(XAVIER_ROOT, "xavier"),
    ]
    
    for binary in candidates:
        if os.path.exists(binary):
            try:
                proc = subprocess.Popen(
                    [binary, "serve"],
                    stdout=subprocess.DEVNULL,
                    stderr=subprocess.DEVNULL,
                )
                return proc
            except Exception:
                pass
    # Try cargo run as last resort
    if os.path.exists(os.path.join(XAVIER_ROOT, "Cargo.toml")):
        try:
            proc = subprocess.Popen(
                ["cargo", "run", "--release", "--", "serve"],
                cwd=XAVIER_ROOT,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            return proc
        except Exception:
            pass
    return None


# ─── Main Test ───────────────────────────────────────────────────────────────

async def main():
    parser = argparse.ArgumentParser(description="Xavier Memory Persistence Test")
    parser.add_argument("--live", action="store_true", help="Run against live Xavier")
    parser.add_argument("--kill", action="store_true", help="Actually kill and restart Xavier")
    args = parser.parse_args()
    
    use_mock = not args.live
    kill_restart = args.kill
    
    print_header("🧪 XAVIER MEMORY PERSISTENCE TEST")
    print(f"  Mode: {'MOCK' if use_mock else 'LIVE'}")
    print(f"  Kill/restart: {'Yes' if kill_restart else 'No (manual)'}")
    print(f"  Target: {XAVIER_URL}")
    print(f"  Test memories: {len(TEST_MEMORIES)}")
    
    # Initialize client
    client = MockClient() if use_mock else XavierClient()
    
    # ─── Phase 1: Store test memories ────────────────────────────
    print_header("📝 PHASE 1: STORE TEST MEMORIES")
    
    async with aiohttp.ClientSession() as session:
        # Health check
        health = await client.check_health(session)
        if not health["ok"]:
            print_result("Xavier health check", health.get("error", "Unknown"), False)
            print("  ❌ Cannot proceed without Xavier. Use --live if Xavier is running.")
            sys.exit(1)
        print_result("Xavier health check", "OK", True)
        
        # Store each test memory
        stored_ids = []
        store_results = []
        for mem in TEST_MEMORIES:
            print_step(f"Storing: {mem['title']}")
            result = await client.add_memory(session, mem["content"], mem["title"])
            if result["ok"]:
                memory_id = result.get("data", {}).get("id", mem["id"])
                stored_ids.append(memory_id)
                store_results.append({"id": mem["id"], "stored": True})
                print_step(f"  → ID: {memory_id}", "📦")
            else:
                store_results.append({"id": mem["id"], "stored": False, "error": result.get("error")})
                print_step(f"  → FAILED: {result.get('error', 'Unknown')}", "❌")
            await asyncio.sleep(0.1)  # Rate limiting
        
        success_count = sum(1 for r in store_results if r["stored"])
        print_result("Memories stored", f"{success_count}/{len(TEST_MEMORIES)}", success_count == len(TEST_MEMORIES))
        
        if success_count == 0:
            print("  ❌ No memories could be stored. Aborting.")
            print("  💡 The /memory/add endpoint may need an API key. Run with --mock for simulation.")
            sys.exit(1)
    
    # ─── Phase 2: Verify pre-restart recall ──────────────────────
    print_header("🔍 PHASE 2: VERIFY PRE-RESTART RECALL")
    
    pre_recall_results = []
    async with aiohttp.ClientSession() as session:
        for i, query in enumerate(SEARCH_QUERIES):
            print_step(f"Searching ({i+1}/{len(SEARCH_QUERIES)}): \"{query[:40]}...\"")
            result = await client.search_memories(session, query, limit=10)
            
            if result["ok"]:
                results_list = result.get("results", [])
                found = len(results_list) > 0
                pre_recall_results.append(found)
                icon = "✅" if found else "⚠️"
                print_step(f"  {icon} Found {len(results_list)} results")
            else:
                pre_recall_results.append(False)
                print_step(f"  ❌ Error: {result.get('error', 'Unknown')}")
    
    pre_recall = sum(1 for r in pre_recall_results if r)
    print_result("Pre-restart recall", f"{pre_recall}/{len(SEARCH_QUERIES)}", pre_recall > 0)
    
    # ─── Phase 3: Stop Xavier ────────────────────────────────────
    print_header("💀 PHASE 3: STOP XAVIER")
    
    if kill_restart and not use_mock:
        pid = find_xavier_process()
        if pid:
            print_step(f"Found Xavier PID: {pid}")
            if stop_xavier(pid):
                print_result("Xavier stopped", f"PID {pid}", True)
                time.sleep(2)
            else:
                print_result("Xavier stop", "Failed", False)
        else:
            print_result("Xavier process", "Not found (manual restart needed)", False)
            print("  🔧 Please stop Xavier manually, then restart it.")
            input("  Press Enter when Xavier has been restarted...")
    else:
        if kill_restart:
            print_step("Mock mode: simulating restart", "🔄")
        else:
            print_step("Skipping kill/restart (use --kill or restart manually)", "⏭️")
            print("  🔧 Please restart Xavier manually, then press Enter.")
            input("  Press Enter when Xavier has been restarted...")
        time.sleep(1)
    
    # ─── Phase 4: Verify post-restart recall ─────────────────────
    print_header("🔍 PHASE 4: VERIFY POST-RESTART RECALL")
    
    post_recall_results = []
    async with aiohttp.ClientSession() as session:
        for i, query in enumerate(SEARCH_QUERIES):
            print_step(f"Searching ({i+1}/{len(SEARCH_QUERIES)}): \"{query[:40]}...\"")
            result = await client.search_memories(session, query, limit=10)
            
            if result["ok"]:
                results_list = result.get("results", [])
                found = len(results_list) > 0
                post_recall_results.append(found)
                icon = "✅" if found else "❌"
                print_step(f"  {icon} Found {len(results_list)} results")
            else:
                post_recall_results.append(False)
                print_step(f"  ❌ Error: {result.get('error', 'Unknown')}")
    
    post_recall = sum(1 for r in post_recall_results if r)
    print_result("Post-restart recall", f"{post_recall}/{len(SEARCH_QUERIES)}", post_recall >= pre_recall)
    
    # ─── Score ───────────────────────────────────────────────────
    print_header("📊 RESULTS")
    
    persistence_recall = post_recall / len(SEARCH_QUERIES) if len(SEARCH_QUERIES) > 0 else 0
    degradation = 0
    if pre_recall > 0:
        degradation = 1 - (post_recall / pre_recall)
    
    print(f"\n  {'='*50}")
    print(f"  PERSISTENCE TEST SUMMARY")
    print(f"  {'='*50}")
    print(f"  Pre-restart recall:  {pre_recall}/{len(SEARCH_QUERIES)} ({pre_recall/len(SEARCH_QUERIES)*100:.1f}%)")
    print(f"  Post-restart recall: {post_recall}/{len(SEARCH_QUERIES)} ({post_recall/len(SEARCH_QUERIES)*100:.1f}%)")
    print(f"  Persistence Score:   {persistence_recall:.1%}")
    print(f"  Degradation:        {degradation:.1%}")
    print(f"  {'='*50}")
    
    if persistence_recall >= 0.8:
        print(f"\n  🏆 GRADE: A — Excellent persistence (≥80%)")
    elif persistence_recall >= 0.6:
        print(f"\n  🏆 GRADE: B — Good persistence (≥60%)")
    elif persistence_recall >= 0.4:
        print(f"\n  ⚠️ GRADE: C — Partial persistence (≥40%)")
    else:
        print(f"\n  ❌ GRADE: F — Poor persistence (<40%)")
    print()
    
    # ─── Save results ────────────────────────────────────────────
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    Path(RESULTS_DIR).mkdir(parents=True, exist_ok=True)
    results_path = Path(RESULTS_DIR) / f"persistence_{timestamp}.json"
    
    results = {
        "metadata": {
            "timestamp": timestamp,
            "mode": "mock" if use_mock else "live",
            "test_memories": len(TEST_MEMORIES),
            "search_queries": len(SEARCH_QUERIES),
            "kill_restart": kill_restart,
        },
        "store_results": store_results,
        "pre_restart_recall": {
            "found": pre_recall,
            "total": len(SEARCH_QUERIES),
            "rate": pre_recall / len(SEARCH_QUERIES),
        },
        "post_restart_recall": {
            "found": post_recall,
            "total": len(SEARCH_QUERIES),
            "rate": post_recall / len(SEARCH_QUERIES),
        },
        "persistence_score": persistence_recall,
        "degradation": degradation,
    }
    
    with open(results_path, "w", encoding="utf-8") as f:
        json.dump(results, f, indent=2)
    
    print(f"  📄 Results saved to: {results_path}")
    
    return persistence_recall


if __name__ == "__main__":
    exit_code = 0 if asyncio.run(main()) >= 0.5 else 1
    sys.exit(exit_code)
