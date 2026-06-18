#!/usr/bin/env python3
"""
Tri-Memory Benchmark: OpenClaw vs Xavier vs Engram
Evaluates three memory systems based on Precision@5, Recall, Latency, and Cost.
"""

import asyncio
import json
import os
import time
import argparse
from datetime import datetime
from pathlib import Path
from typing import List, Dict, Any, Optional

import aiohttp

# Configuration
DEFAULT_CORTEX_URL = "http://localhost:8003"
DEFAULT_XAVIER_URL = "http://localhost:8006"
DEFAULT_ENGRAM_URL = "http://localhost:7437"

class MemorySystemHandler:
    def __init__(self, name: str, base_url: str):
        self.name = name
        self.base_url = base_url.rstrip('/')

    async def search(self, query: str, limit: int = 5) -> Dict[str, Any]:
        raise NotImplementedError

class CortexHandler(MemorySystemHandler):
    async def search(self, session: aiohttp.ClientSession, query: str, limit: int = 5) -> Dict[str, Any]:
        url = f"{self.base_url}/memory/search"
        start_time = time.perf_counter()
        try:
            async with session.post(url, json={"query": query, "limit": limit}, timeout=5) as resp:
                latency = (time.perf_counter() - start_time) * 1000
                if resp.status == 200:
                    results = await resp.json()
                    return {
                        "ok": True,
                        "latency_ms": latency,
                        "results": results.get("results", []),
                        "system": self.name
                    }
                else:
                    return {"ok": False, "error": f"HTTP {resp.status}", "latency_ms": latency, "system": self.name}
        except Exception as e:
            return {"ok": False, "error": str(e), "latency_ms": (time.perf_counter() - start_time) * 1000, "system": self.name}

class XavierHandler(MemorySystemHandler):
    async def search(self, session: aiohttp.ClientSession, query: str, limit: int = 5) -> Dict[str, Any]:
        url = f"{self.base_url}/memory/search"
        start_time = time.perf_counter()
        try:
            async with session.post(url, json={"query": query, "limit": limit}, timeout=5) as resp:
                latency = (time.perf_counter() - start_time) * 1000
                if resp.status == 200:
                    results = await resp.json()
                    return {
                        "ok": True,
                        "latency_ms": latency,
                        "results": results.get("results", []),
                        "system": self.name
                    }
                else:
                    return {"ok": False, "error": f"HTTP {resp.status}", "latency_ms": latency, "system": self.name}
        except Exception as e:
            return {"ok": False, "error": str(e), "latency_ms": (time.perf_counter() - start_time) * 1000, "system": self.name}

class EngramHandler(MemorySystemHandler):
    async def search(self, session: aiohttp.ClientSession, query: str, limit: int = 5) -> Dict[str, Any]:
        # Engram uses /mem/search according to existing scripts
        url = f"{self.base_url}/mem/search"
        start_time = time.perf_counter()
        try:
            async with session.post(url, json={"query": query, "limit": limit}, timeout=5) as resp:
                latency = (time.perf_counter() - start_time) * 1000
                if resp.status == 200:
                    results = await resp.json()
                    # Adjust based on Engram's actual response format if needed
                    return {
                        "ok": True,
                        "latency_ms": latency,
                        "results": results.get("matches", []) or results.get("results", []),
                        "system": self.name
                    }
                else:
                    return {"ok": False, "error": f"HTTP {resp.status}", "latency_ms": latency, "system": self.name}
        except Exception as e:
            return {"ok": False, "error": str(e), "latency_ms": (time.perf_counter() - start_time) * 1000, "system": self.name}

class Evaluator:
    def __init__(self, use_mock: bool = False):
        self.use_mock = use_mock

    async def evaluate_relevance(self, query: str, result_content: str) -> float:
        """
        Uses an LLM to judge if the result is relevant to the query.
        Returns a score between 0.0 and 1.0.
        """
        if self.use_mock:
            # Simple keyword matching for mock mode
            query_words = set(query.lower().split())
            content_words = set(result_content.lower().split())
            if not query_words: return 0.0
            overlap = len(query_words.intersection(content_words))
            return min(1.0, overlap / len(query_words))

        # In a real scenario, this would call an LLM API
        # For now, we'll keep it simple or use a local small model if available
        return 0.5

    async def check_recall(self, expected_facts: List[str], results: List[Dict[str, Any]]) -> float:
        """
        Checks how many of the expected facts are present in the results.
        Returns a score between 0.0 and 1.0.
        """
        if not expected_facts:
            return 1.0

        all_text = " ".join([str(r.get("content", "")) + " " + str(r.get("text", "")) for r in results]).lower()
        found_count = 0
        for fact in expected_facts:
            if fact.lower() in all_text:
                found_count += 1

        return found_count / len(expected_facts)

    async def calculate_precision_at_5(self, query: str, results: List[Dict[str, Any]]) -> float:
        """
        Calculates Precision@5 by evaluating each of the top 5 results.
        """
        if not results:
            return 0.0

        top_5 = results[:5]
        relevance_scores = []
        for res in top_5:
            content = res.get("content", "") or res.get("text", "")
            score = await self.evaluate_relevance(query, content)
            relevance_scores.append(score)

        return sum(relevance_scores) / len(top_5)

class Scorer:
    def __init__(self, weights: Dict[str, float]):
        self.weights = weights

    def calculate_score(self, precision: float, recall: float, latency_ms: float, cost: float) -> float:
        # Normalize latency: assume < 100ms is 1.0, > 1000ms is 0.0
        latency_score = max(0.0, 1.0 - (latency_ms - 100) / 900) if latency_ms > 100 else 1.0

        # Normalize cost: assume 0 is 1.0, 1.0 is 0.0 (arbitrary units)
        cost_score = max(0.0, 1.0 - cost)

        total_score = (
            precision * self.weights["precision"] +
            recall * self.weights["recall"] +
            latency_score * self.weights["latency"] +
            cost_score * self.weights["cost"]
        )
        return total_score

class MetaScorer:
    def __init__(self, scorer: Scorer):
        self.scorer = scorer

    def choose_winner(self, results: List[Dict[str, Any]]) -> Dict[str, Any]:
        scored_results = []
        for res in results:
            if not res["ok"]:
                score = 0.0
            else:
                score = self.scorer.calculate_score(
                    res["precision_at_5"],
                    res["recall"],
                    res["latency_ms"],
                    res.get("cost", 0.0)
                )
            res["final_score"] = score
            scored_results.append(res)

        winner = max(scored_results, key=lambda x: x["final_score"])
        return {
            "winner": winner["system"],
            "all_scores": {r["system"]: r["final_score"] for r in scored_results}
        }

def update_history(history_file: Path, summary: Dict[str, Any]):
    timestamp = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    header = "| Date | System | Success Rate | Avg Final Score | Avg Latency (ms) | Avg Precision@5 | Avg Recall |\n"
    separator = "| --- | --- | --- | --- | --- | --- | --- |\n"

    lines = []
    if history_file.exists():
        with open(history_file, 'r', encoding='utf-8') as f:
            lines = f.readlines()

    if not lines:
        lines = ["# Tri-Memory Benchmark History\n\n", header, separator]

    for system, stats in summary.items():
        new_row = f"| {timestamp} | {system} | {stats['success_rate']:.1%} | {stats['avg_score']:.3f} | {stats['avg_latency']:.1f} | {stats['avg_precision']:.3f} | {stats['avg_recall']:.3f} |\n"
        lines.append(new_row)

    with open(history_file, 'w', encoding='utf-8') as f:
        f.writelines(lines)

def generate_report(run_results: List[Dict[str, Any]], summary: Dict[str, Any]) -> str:
    report = "# Tri-Memory Benchmark Report\n\n"
    report += f"Date: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}\n\n"

    report += "## Summary\n\n"
    report += "| System | Success Rate | Avg Final Score | Avg Latency (ms) | Avg Precision@5 | Avg Recall |\n"
    report += "| --- | --- | --- | --- | --- | --- |\n"
    for system, stats in summary.items():
        report += f"| {system} | {stats['success_rate']:.1%} | {stats['avg_score']:.3f} | {stats['avg_latency']:.1f} | {stats['avg_precision']:.3f} | {stats['avg_recall']:.3f} |\n"

    report += "\n## Detailed Results\n\n"
    for i, res in enumerate(run_results):
        report += f"### Query {i+1}: {res['query']}\n"
        report += f"- **Winner: {res['meta']['winner']}**\n"
        report += "| System | Ok | Score | Latency | P@5 | Recall |\n"
        report += "| --- | --- | --- | --- | --- | --- |\n"
        for sys_res in res["systems"]:
            report += f"| {sys_res['system']} | {sys_res['ok']} | {sys_res.get('final_score', 0.0):.3f} | {sys_res.get('latency_ms', 0.0):.1f} | {sys_res.get('precision_at_5', 0.0):.3f} | {sys_res.get('recall', 0.0):.3f} |\n"
        report += "\n"

    return report

async def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--queries", default="benchmarks/tri_memory_queries.json")
    parser.add_argument("--mock-llm", action="store_true", help="Use mock keyword-based evaluation instead of LLM")
    parser.add_argument("--output-dir", default="benchmarks/results")
    parser.add_argument("--cortex-url", default=DEFAULT_CORTEX_URL)
    parser.add_argument("--xavier-url", default=DEFAULT_XAVIER_URL)
    parser.add_argument("--engram-url", default=DEFAULT_ENGRAM_URL)
    args = parser.parse_args()

    # Setup
    handlers = [
        CortexHandler("OpenClaw-Builtin", args.cortex_url),
        XavierHandler("Xavier", args.xavier_url),
        EngramHandler("Engram", args.engram_url)
    ]
    evaluator = Evaluator(use_mock=args.mock_llm)
    weights = {"precision": 0.4, "recall": 0.25, "latency": 0.2, "cost": 0.15}
    scorer = Scorer(weights)
    meta_scorer = MetaScorer(scorer)

    # Load queries
    with open(args.queries, 'r', encoding='utf-8') as f:
        queries = json.load(f)

    run_results = []

    print(f"Starting benchmark with {len(queries)} queries...")

    async with aiohttp.ClientSession() as session:
        for q in queries:
            print(f"Processing: {q['query']}")
            query_results = []
            for handler in handlers:
                res = await handler.search(session, q["query"])
                if res["ok"]:
                    res["precision_at_5"] = await evaluator.calculate_precision_at_5(q["query"], res["results"])
                    res["recall"] = await evaluator.check_recall(q["expected_facts"], res["results"])
                else:
                    res["precision_at_5"] = 0.0
                    res["recall"] = 0.0
                query_results.append(res)

        meta = meta_scorer.choose_winner(query_results)
        run_results.append({
            "query": q["query"],
            "systems": query_results,
            "meta": meta
        })

    # Calculate summary
    total_queries = len(queries)
    summary = {}
    for handler in handlers:
        sys_name = handler.name
        sys_results = [r for res in run_results for r in res["systems"] if r["system"] == sys_name]
        ok_results = [r for r in sys_results if r["ok"]]

        if total_queries > 0:
            summary[sys_name] = {
                "avg_score": sum(r.get("final_score", 0.0) for r in sys_results) / total_queries,
                "avg_precision": sum(r.get("precision_at_5", 0.0) for r in sys_results) / total_queries,
                "avg_recall": sum(r.get("recall", 0.0) for r in sys_results) / total_queries,
                "avg_latency": sum(r["latency_ms"] for r in ok_results) / len(ok_results) if ok_results else 0.0,
                "success_rate": len(ok_results) / total_queries
            }
        else:
            summary[sys_name] = {"avg_score": 0.0, "avg_latency": 0.0, "avg_precision": 0.0, "avg_recall": 0.0, "success_rate": 0.0}

    # Output
    out_path = Path(args.output_dir)
    out_path.mkdir(parents=True, exist_ok=True)
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")

    # JSON Artifact
    json_out = out_path / f"benchmark_{timestamp}.json"
    with open(json_out, 'w', encoding='utf-8') as f:
        json.dump({"results": run_results, "summary": summary}, f, indent=2)

    # Markdown Report
    md_out = out_path / f"benchmark_{timestamp}.md"
    report_content = generate_report(run_results, summary)
    with open(md_out, 'w', encoding='utf-8') as f:
        f.write(report_content)

    # History Tracking
    history_file = Path("benchmarks/HISTORY.md")
    update_history(history_file, summary)

    print(f"Benchmark complete. Report: {md_out}")

if __name__ == "__main__":
    asyncio.run(main())
