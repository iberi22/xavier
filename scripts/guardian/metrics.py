import sys
import json
import argparse
from datetime import datetime

def parse_iso(val: str | None) -> datetime | None:
    if not val:
        return None
    return datetime.fromisoformat(val)

def mttr_series(results: list[dict]) -> dict:
    series = {}

    for r in results:
        tid = r.get("target_id")
        if not tid:
            continue

        if tid not in series:
            series[tid] = {
                "time_to_detect": 0.0,
                "detection_to_decision": 0.0,
                "rollback_time": 0.0,
                "production_mttr": 0.0,
                "false_positives": 0,
                "failed_rollbacks": 0,
                "unresolved": 0,
            }

        outcome = r.get("outcome")
        escalated = r.get("escalated", False)
        false_pos = r.get("false_positive", False)

        if false_pos:
            series[tid]["false_positives"] += 1

        if outcome == "failed":
            series[tid]["failed_rollbacks"] += 1

        if escalated:
            series[tid]["unresolved"] += 1

        detected = parse_iso(r.get("detected_at"))
        decided = parse_iso(r.get("decided_at"))
        restored = parse_iso(r.get("restored_at"))

        # In a real implementation we would have an "occurred_at" to compute time_to_detect.
        # Since we don't have start time in schema, we can assume time_to_detect = 0 or
        # derive it if available. Given we just use what we have:
        if detected and decided:
            series[tid]["detection_to_decision"] += (decided - detected).total_seconds()

        if decided and restored:
            series[tid]["rollback_time"] += (restored - decided).total_seconds()

        if outcome == "restored" and restored and detected:
            series[tid]["production_mttr"] += (restored - detected).total_seconds()

    return series

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("results_file", help="Path to results JSON")
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()

    try:
        with open(args.results_file, "r", encoding="utf-8") as f:
            data = json.load(f)
    except Exception as e:
        print(f"Error reading results: {e}", file=sys.stderr)
        sys.exit(2)

    if not isinstance(data, list):
        print("Malformed input: expected a list of results", file=sys.stderr)
        sys.exit(2)

    series = mttr_series(data)

    if args.json:
        print(json.dumps(series, indent=2))
    else:
        for tid, metrics in series.items():
            print(f"Target: {tid}")
            for k, v in metrics.items():
                print(f"  {k}: {v}")

if __name__ == "__main__":
    main()
