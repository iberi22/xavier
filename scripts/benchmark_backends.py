import asyncio
import json
import os
import time
from datetime import datetime
from typing import List, Dict, Any, Optional

# Standard dataset for testing
TEST_DATA = [
    {
        "path": "docs/architecture.md",
        "content": "Xavier uses a B-Tree based memory structure for fast retrieval.",
        "metadata": {"tags": ["architecture", "memory"]}
    },
    {
        "path": "docs/mesh.md",
        "content": "The sovereign mesh boundary separates internal replication from external governance.",
        "metadata": {"tags": ["mesh", "networking"]}
    },
    {
        "path": "docs/tgd.md",
        "content": "TGD refinement optimizes memory based on learning rates and confidence thresholds.",
        "metadata": {"tags": ["tgd", "optimization"]}
    },
    {
        "path": "docs/mcp.rs",
        "content": "The MCP server handles tool calls and session management.",
        "metadata": {"tags": ["mcp", "server"]}
    },
    {
        "path": "docs/license.md",
        "content": "Xavier uses a dual-license model: MIT for core and MESH for peer-to-peer features.",
        "metadata": {"tags": ["license", "legal"]}
    }
]

# Queries to run
QUERIES = [
    {
        "query": "What is the memory structure of Xavier?",
        "expected_path": "docs/architecture.md",
        "expected_fact": "B-Tree"
    },
    {
        "query": "How is the sovereign mesh boundary defined?",
        "expected_path": "docs/mesh.md",
        "expected_fact": "separates internal replication"
    },
    {
        "query": "Tell me about TGD refinement parameters.",
        "expected_path": "docs/tgd.md",
        "expected_fact": "learning rates"
    },
    {
        "query": "Which tool handles MCP calls?",
        "expected_path": "docs/mcp.rs",
        "expected_fact": "tool calls"
    },
    {
        "query": "What are the licenses used by Xavier?",
        "expected_path": "docs/license.md",
        "expected_fact": "dual-license"
    }
]

class BackendInterface:
    def __init__(self, name: str, url: str, token: str):
        self.name = name
        self.url = url
        self.token = token

    async def clear_data(self):
        # Implementation depends on how we can clear the backend
        pass

    async def add_memory(self, item: Dict[str, Any]) -> float:
        # returns latency in ms
        import aiohttp
        start = time.perf_counter()
        try:
            async with aiohttp.ClientSession() as session:
                async with session.post(
                    f"{self.url}/memory/add",
                    headers={"X-Xavier-Token": self.token},
                    json=item,
                    timeout=10
                ) as resp:
                    await resp.text()
                    return (time.perf_counter() - start) * 1000
        except Exception as e:
            print(f"Error adding to {self.name}: {e}")
            return -1

    async def search(self, query: str) -> Dict[str, Any]:
        # returns search result and latency
        import aiohttp
        start = time.perf_counter()
        try:
            async with aiohttp.ClientSession() as session:
                async with session.post(
                    f"{self.url}/memory/search",
                    headers={"X-Xavier-Token": self.token},
                    json={"query": query, "limit": 5},
                    timeout=10
                ) as resp:
                    data = await resp.json()
                    return {
                        "latency_ms": (time.perf_counter() - start) * 1000,
                        "results": data.get("results", []),
                        "token_count": len(json.dumps(data).split()) # Rough estimate
                    }
        except Exception as e:
            print(f"Error searching {self.name}: {e}")
            return {"latency_ms": -1, "results": [], "token_count": 0}

async def run_benchmark():
    # Xavier must be running in the target mode
    # For this script to work fully, we'd need 3 instances or switch backends
    # In a real environment, we'd configure XAVIER_MEMORY_BACKEND and restart

    # Placeholder for URLs - in practice these would be different instances or the same if testing sequentially
    backends = [
        BackendInterface("Local (SQLite)", "http://localhost:8006", "test-token"),
        BackendInterface("Supabase (REST)", "http://localhost:8106", "test-token"), # Assuming separate ports for demo
        BackendInterface("Neon (Direct PG)", "http://localhost:8206", "test-token")
    ]

    results = []

    print("🚀 Starting Backend Benchmark...")

    for backend in backends:
        print(f"\n📊 Testing {backend.name}...")

        # Populate
        total_add_latency = 0
        for item in TEST_DATA:
            lat = await backend.add_memory(item)
            if lat > 0:
                total_add_latency += lat

        avg_add_latency = total_add_latency / len(TEST_DATA) if TEST_DATA else 0
        print(f"   ✅ Data populated. Avg Add Latency: {avg_add_latency:.2f}ms")

        # Search
        queries_results = []
        for q in QUERIES:
            res = await backend.search(q["query"])

            # Evaluate Recall & Precision
            top_hit = res["results"][0] if res["results"] else None
            recall = 1.0 if any(q["expected_path"] == r.get("path") for r in res["results"]) else 0.0
            precision = 1.0 if top_hit and top_hit.get("path") == q["expected_path"] else 0.0

            queries_results.append({
                "query": q["query"],
                "latency_ms": res["latency_ms"],
                "recall": recall,
                "precision": precision,
                "token_count": res["token_count"]
            })
            print(f"   🔍 Query: '{q['query'][:30]}...' -> Latency: {res['latency_ms']:.2f}ms, P: {precision}, R: {recall}")

        # Summary for backend
        summary = {
            "backend": backend.name,
            "avg_latency_ms": sum(qr["latency_ms"] for qr in queries_results) / len(queries_results),
            "avg_recall": sum(qr["recall"] for qr in queries_results) / len(queries_results),
            "avg_precision": sum(qr["precision"] for qr in queries_results) / len(queries_results),
            "total_tokens": sum(qr["token_count"] for qr in queries_results),
            "queries": queries_results
        }
        results.append(summary)

    # Save Report
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    report_path = f"exports/benchmark-{timestamp}.json"

    with open(report_path, "w") as f:
        json.dump(results, f, indent=2)

    print(f"\n💾 Benchmark complete. Report saved to {report_path}")

if __name__ == "__main__":
    # Since we can't easily run 3 separate servers in this environment,
    # we simulate the results if they are not reachable, or try to connect.
    # In a real test, the user would run this after setting up the endpoints.
    try:
        asyncio.run(run_benchmark())
    except KeyboardInterrupt:
        pass
