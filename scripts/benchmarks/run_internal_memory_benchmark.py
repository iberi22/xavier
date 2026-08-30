import argparse
import json
import os
import socket
import subprocess
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
DEFAULT_DATASET = ROOT / "scripts" / "benchmarks" / "datasets" / "internal_swal_openclaw_memory.json"
HTTP_TIMEOUT_SECONDS = 60


def get_required_xavier_token() -> str:
    for env_var in ("XAVIER_TOKEN", "XAVIER_API_KEY", "AUTH_TOKEN"):
        token = os.environ.get(env_var, "").strip()
        if token:
            return token
    dotenv_path = ROOT / ".env"
    if dotenv_path.exists():
        for line in dotenv_path.read_text(encoding="utf-8", errors="ignore").splitlines():
            line = line.strip()
            if line.startswith("XAVIER_TOKEN=") or line.startswith("XAVIER_TOKEN_SECRET="):
                val = line.split("=", 1)[1].strip().strip('"').strip("'")
                if val:
                    return val
    raise RuntimeError("Missing Xavier token. Set XAVIER_TOKEN, XAVIER_API_KEY, or configure .env.")


TOKEN = get_required_xavier_token()


def http_json(url: str, payload: dict = None, method: str = "POST") -> dict:
    data = json.dumps(payload).encode("utf-8") if payload is not None else None
    request = urllib.request.Request(
        url,
        data=data,
        method=method,
        headers={
            "Content-Type": "application/json",
            "X-Xavier-Token": TOKEN,
            "Authorization": f"Bearer {TOKEN}",
        },
    )
    with urllib.request.urlopen(request, timeout=HTTP_TIMEOUT_SECONDS) as response:
        return json.loads(response.read().decode("utf-8"))


def wait_for_health(base_url: str) -> None:
    for _ in range(60):
        try:
            with urllib.request.urlopen(f"{base_url}/health", timeout=5) as response:
                if response.status == 200:
                    return
        except Exception:
            time.sleep(1)
    raise RuntimeError("Xavier did not become healthy in time")


def add_documents(base_url: str, dataset: dict) -> list:
    latencies = []
    for document in dataset["documents"]:
        payload = {
            "path": document["path"],
            "content": document["content"],
            "metadata": document.get("metadata", {}),
            "kind": document.get("kind"),
            "evidence_kind": document.get("evidence_kind"),
            "namespace": document.get("namespace"),
            "provenance": document.get("provenance"),
        }
        t0 = time.perf_counter()
        http_json(f"{base_url}/v1/memories", payload)
        latencies.append((time.perf_counter() - t0) * 1000.0)
    return latencies


def reserve_base_url(base_url: str) -> str:
    parsed = urllib.parse.urlparse(base_url)
    host = parsed.hostname or "127.0.0.1"
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind((host, 0))
        port = sock.getsockname()[1]
    return urllib.parse.urlunparse(
        (
            parsed.scheme or "http",
            f"{host}:{port}",
            parsed.path or "",
            "",
            "",
            "",
        )
    )


def evaluate_case(base_url: str, case: dict) -> dict:
    endpoint = case["endpoint"]
    payload = {
        "query": case["query"],
        "limit": 5,
        "filters": case.get("filters"),
    }
    if "system3_mode" in case:
        payload["system3_mode"] = case["system3_mode"]

    t0 = time.perf_counter()
    if endpoint == "search":
        try:
            response = http_json(f"{base_url}/v1/memories/search", payload)
        except Exception:
            response = http_json(f"{base_url}/memory/search", payload)
        latency_ms = (time.perf_counter() - t0) * 1000.0
        top_path = None
        results = response.get("results", [])
        if results:
            first = results[0]
            top_path = (
                first.get("path")
                or first.get("user_id")
                or (first.get("metadata", {}).get("path") if isinstance(first.get("metadata"), dict) else None)
            )
        return {
            "id": case["id"],
            "endpoint": endpoint,
            "success": top_path == case["expected_path"],
            "expected_path": case["expected_path"],
            "actual_path": top_path,
            "latency_ms": round(latency_ms, 2),
        }

    route = "/memory/query" if endpoint == "query" else "/agents/run"
    try:
        response = http_json(f"{base_url}{route}", payload)
    except Exception:
        response = {}
    latency_ms = (time.perf_counter() - t0) * 1000.0
    answer = response.get("response", "")
    expected = case["expected_substring"]
    return {
        "id": case["id"],
        "endpoint": endpoint,
        "success": expected.lower() in answer.lower(),
        "expected_substring": expected,
        "actual_response": answer,
        "latency_ms": round(latency_ms, 2),
    }


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base-url", default="http://127.0.0.1:8003")
    parser.add_argument("--dataset", default=str(DEFAULT_DATASET))
    parser.add_argument("--output-dir", required=True)
    parser.add_argument("--use-existing-server", action="store_true")
    args = parser.parse_args()

    base_url = args.base_url
    if not args.use_existing_server:
        base_url = reserve_base_url(base_url)

    dataset = json.loads(Path(args.dataset).read_text(encoding="utf-8"))
    output_dir = Path(args.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)

    child = None
    if not args.use_existing_server:
        parsed = urllib.parse.urlparse(base_url)
        env = os.environ.copy()
        if parsed.hostname:
            env["XAVIER_HOST"] = parsed.hostname
        if parsed.port:
            env["XAVIER_PORT"] = str(parsed.port)
        # Keep the internal suite deterministic and evidence-first.
        env["XAVIER_DISABLE_HYDE"] = "1"
        xavier_bin = os.environ.get("XAVIER_BIN")
        if not xavier_bin:
            candidate = Path.home() / ".local/bin/xavier-real"
            if candidate.exists():
                xavier_bin = str(candidate)
            else:
                xavier_bin = "xavier"

        env["XAVIER_TOKEN"] = TOKEN

        cmd = [xavier_bin, "http"]
        child = subprocess.Popen(
            cmd,
            cwd=ROOT,
            env=env,
            stdout=(output_dir / "xavier.stdout.log").open("wb"),
            stderr=(output_dir / "xavier.stderr.log").open("wb"),
        )

    try:
        wait_for_health(base_url)
        ingest_latencies = add_documents(base_url, dataset)
        records = [evaluate_case(base_url, case) for case in dataset["cases"]]
        passed_count = sum(1 for record in records if record["success"])
        eval_latencies = [r["latency_ms"] for r in records]
        summary = {
            "benchmark": "internal_swal_openclaw_memory",
            "dataset": str(Path(args.dataset)),
            "base_url": base_url,
            "cases": len(records),
            "passed": passed_count,
            "accuracy": passed_count / len(records) if records else 0.0,
            "avg_ingest_latency_ms": round(sum(ingest_latencies) / len(ingest_latencies), 2) if ingest_latencies else 0.0,
            "avg_eval_latency_ms": round(sum(eval_latencies) / len(eval_latencies), 2) if eval_latencies else 0.0,
            "p95_eval_latency_ms": round(sorted(eval_latencies)[int(len(eval_latencies) * 0.95)] if eval_latencies else 0.0, 2),
        }
        (output_dir / "summary.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")
        (output_dir / "records.json").write_text(json.dumps(records, indent=2), encoding="utf-8")
        print(json.dumps({"summary": summary, "records": records}, indent=2))
    finally:
        if child is not None:
            child.terminate()
            try:
                child.wait(timeout=10)
            except subprocess.TimeoutExpired:
                child.kill()


if __name__ == "__main__":
    main()
