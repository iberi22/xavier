import sys
import json
import argparse
import urllib.request
import urllib.error
from datetime import datetime, timezone

# Shared interface contracts (PLAN.md §Interface contracts):
#   ProbeRecord     {target_id, kind, deployment_id, vantage, observed_at, status,
#                    checks:[{name,ok,detail}], error|null}
#                    status in ("healthy","unhealthy","unknown")
#   FailureEvidence {target_id, kind, deployment_id, detected_at, failed_probes,
#                    window_seconds, vantages:[str], correlated:bool,
#                    missing_metrics:[str], summary}
PROBE_STATUSES = ("healthy", "unhealthy", "unknown")

def _utc_z(dt: datetime | None = None) -> str:
    if dt is None:
        dt = datetime.now(timezone.utc)
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=timezone.utc)
    return dt.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")

def _default_fetch(url: str, timeout: float = 5.0) -> tuple[int, str]:
    # The URL is used only for the request; it is never echoed back.
    req = urllib.request.Request(url, headers={'User-Agent': 'xavier-guardian-probe/1.0'})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as response:
            return response.getcode(), response.read().decode('utf-8')
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode('utf-8')
    except Exception:
        # Provider outage: the metric is unavailable, which is never healthy.
        return 0, "probe unavailable"

def probe_targets(inventory: dict, vantage: str, fetch=None) -> list[dict]:
    if fetch is None:
        fetch = _default_fetch

    records = []

    if not isinstance(inventory, dict):
        return []
    targets = inventory.get("targets", [])
    if not isinstance(targets, list):
        return []

    observed_at = _utc_z()

    for t in targets:
        if not isinstance(t, dict):
            continue

        if t.get("enabled") is True:
            target_id = t.get("id")
            kind = t.get("kind")
            url = t.get("url")
            expected_did = t.get("production_id")
            checks = []
            error = None

            if not url:
                # An unavailable metric is 'unknown', never healthy.
                status = "unknown"
                actual_did = expected_did
                checks.append({"name": "endpoint", "ok": False, "detail": "no probe url configured"})
            else:
                code, body = fetch(url, timeout=5.0)
                if code == 0:
                    status = "unknown"
                    actual_did = expected_did
                    error = "probe unavailable"
                    checks.append({"name": "endpoint", "ok": False, "detail": "probe unavailable"})
                else:
                    body_data = {}
                    try:
                        body_data = json.loads(body)
                    except Exception:
                        body_data = {}

                    if isinstance(body_data, dict):
                        actual_did = body_data.get("deployment_id", expected_did)
                    else:
                        actual_did = expected_did

                    # Tie smoke, health and error-rate evidence to the actual deployment ID.
                    if 200 <= code < 300:
                        if actual_did and str(actual_did) != str(expected_did):
                            status = "unhealthy"
                            checks.append({"name": "deployment_id", "ok": False, "detail": "deployment id mismatch"})
                        else:
                            status = "healthy"
                            checks.append({"name": "smoke", "ok": True, "detail": "ok"})
                    else:
                        status = "unhealthy"
                        checks.append({"name": "smoke", "ok": False, "detail": f"http {code}"})

            records.append({
                "target_id": target_id,
                "kind": kind,
                "deployment_id": actual_did,
                "vantage": vantage,
                "observed_at": observed_at,
                "status": status,
                "checks": checks,
                "error": error,
            })

    return records

def correlate(records: list[dict], now: datetime, policy: dict | None = None) -> list[dict]:
    # Return FailureEvidence records only, deduplicated by (target_id, deployment_id).
    # An unavailable metric is 'unknown', never green; 'unhealthy' is the failure signal.
    if not isinstance(records, list):
        return []

    policy = policy or {}
    window_seconds = int(policy.get("window_seconds", 300))

    groups = {}
    for r in records:
        if not isinstance(r, dict):
            continue
        key = (r.get("target_id"), r.get("deployment_id"))
        groups.setdefault(key, []).append(r)

    evidence = []
    for (tid, did), group_records in groups.items():
        kind = next((r.get("kind") for r in group_records if r.get("kind")), None)
        failing = [r for r in group_records if r.get("status") in ("unhealthy", "unknown")]
        if not failing:
            continue

        failure_vantages = sorted({r.get("vantage") for r in failing if r.get("vantage")})
        failed_probes = sorted({r.get("vantage") for r in failing if r.get("status") == "unhealthy"})
        missing_metrics = sorted({r.get("vantage") for r in failing if r.get("status") == "unknown"})
        # correlated is True only with at least two agreeing vantages or agreeing telemetry.
        telemetry = any(r.get("vantage") == "telemetry" and r.get("status") == "unhealthy" for r in group_records)
        correlated = len(failure_vantages) >= 2 or telemetry

        evidence.append({
            "target_id": tid,
            "kind": kind,
            "deployment_id": did,
            "detected_at": _utc_z(now),
            "failed_probes": failed_probes,
            "window_seconds": window_seconds,
            "vantages": failure_vantages,
            "correlated": correlated,
            "missing_metrics": missing_metrics,
            "summary": f"{len(failing)} failing probe(s) on {tid}/{did}",
        })

    return evidence

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("inventory")
    parser.add_argument("--vantage")
    parser.add_argument("--correlate", action="store_true")
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()

    try:
        with open(args.inventory, "r", encoding="utf-8") as f:
            inventory = json.load(f)

        records = probe_targets(inventory, args.vantage)

        if args.correlate:
            now = datetime.now(timezone.utc)
            results = correlate(records, now)
            if args.json:
                print(json.dumps(results, indent=2))
            else:
                print(results)

            if any(ev.get("correlated") for ev in results):
                sys.exit(1)
            else:
                sys.exit(0)
        else:
            if args.json:
                print(json.dumps(records, indent=2))
            else:
                print(records)

            sys.exit(0)
    except SystemExit:
        raise
    except Exception as e:
        print(f"Error reading or processing inventory: {e}", file=sys.stderr)
        sys.exit(2)

if __name__ == "__main__":
    main()
