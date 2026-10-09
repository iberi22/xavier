import sys
import json
import argparse
import urllib.request
import urllib.error
from urllib.parse import urlparse
from datetime import datetime, timezone

def _redact_url(url: str) -> str:
    if not url:
        return ""
    try:
        parsed = urlparse(url)
        return f"{parsed.scheme}://{parsed.netloc}{parsed.path}"
    except Exception:
        return ""

def _default_fetch(url: str, timeout: float = 5.0) -> tuple[int, str]:
    # Redact url before using/printing, though we just use it safely here
    redacted = _redact_url(url)
    req = urllib.request.Request(url, headers={'User-Agent': 'xavier-guardian-probe/1.0'})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as response:
            return response.getcode(), response.read().decode('utf-8')
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode('utf-8')
    except Exception as e:
        return 0, str(e)

def probe_targets(inventory: dict, vantage: str, fetch=None) -> list[dict]:
    if fetch is None:
        fetch = _default_fetch

    records = []

    if not isinstance(inventory, dict):
        return []
    targets = inventory.get("targets", [])
    if not isinstance(targets, list):
        return []

    now_iso = datetime.now(timezone.utc).isoformat()

    for t in targets:
        if not isinstance(t, dict):
            continue

        if t.get("enabled") is True:
            target_id = t.get("id")
            url = t.get("url")
            expected_did = t.get("production_id")

            if not url:
                # An unavailable metric is 'unknown', never healthy
                status = "unknown"
                actual_did = expected_did
            else:
                code, body = fetch(url, timeout=5.0)
                if code == 0:
                    status = "unknown" # or failing? "unavailable metric is unknown and never healthy"
                    actual_did = expected_did
                else:
                    body_data = {}
                    try:
                        body_data = json.loads(body)
                    except Exception:
                        pass

                    actual_did = body_data.get("deployment_id", expected_did)

                    # Tie smoke, health and error-rate evidence to the actual deployment ID.
                    if 200 <= code < 300:
                        if actual_did and str(actual_did) != str(expected_did):
                            status = "failing"
                        else:
                            status = "healthy"
                    else:
                        status = "failing"

            records.append({
                "target_id": target_id,
                "deployment_id": actual_did,
                "vantage": vantage,
                "status": status,
                "timestamp": now_iso
            })

    return records

def correlate(records: list[dict], now: datetime, policy: dict | None = None) -> list[dict]:
    # return FailureEvidence records only
    # deduplicate by (target_id, deployment_id)
    groups = {}
    for r in records:
        key = (r.get("target_id"), r.get("deployment_id"))
        if key not in groups:
            groups[key] = []
        groups[key].append(r)

    evidence = []
    for (tid, did), group_records in groups.items():
        # correlated is True only with at least two agreeing vantages or agreeing telemetry
        # a missing metric is unknown, never green
        statuses = {r.get("status") for r in group_records}
        vantages = {r.get("vantage") for r in group_records if r.get("status") in ("failing", "unknown")}

        # We consider a failure if any record is failing or unknown
        # Actually, "unknown" is never healthy. Let's just track "failing" or "unknown" as evidence
        if "failing" in statuses or "unknown" in statuses:
            correlated = len(vantages) >= 2 or "telemetry" in vantages

            evidence.append({
                "target_id": tid,
                "deployment_id": did,
                "status": "failing" if "failing" in statuses else "unknown",
                "correlated": correlated
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
