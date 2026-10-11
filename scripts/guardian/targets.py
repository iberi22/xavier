import json
import sys
import argparse
import re
from datetime import datetime, timedelta, timezone

TARGET_KINDS = ("local_binary", "release_tag", "cf_pages", "cf_worker", "gh_pages")

# Target record schema:
# id: str
# kind: str
# enabled: bool (default False)
# production_id: str
# source_sha: str
# artifact_digest: str
# health_checked_at: str (ISO-8601 UTC)
# promoted_at: str
# previous_known_good_id: str

def load_inventory(path):
    try:
        with open(path, "r", encoding="utf-8") as f:
            return json.load(f)
    except Exception as e:
        print(f"Error reading inventory: {e}", file=sys.stderr)
        sys.exit(1)

def validate_inventory(payload: dict, now: datetime, max_evidence_age: timedelta = timedelta(hours=24)) -> list[str]:
    reasons = []
    targets = payload.get("targets", [])

    seen_ids = set()
    for target in targets:
        target_id = target.get("id")
        if target_id in seen_ids:
            reasons.append(f"Duplicate id: {target_id}")
        if target_id is not None:
            seen_ids.add(target_id)

    for i, target in enumerate(targets):
        tid = target.get("id", f"index_{i}")
        kind = target.get("kind")
        enabled = target.get("enabled", False)  # default stays False

        if kind not in TARGET_KINDS:
            reasons.append(f"Target '{tid}' has unknown kind: {kind}")

        digest = target.get("artifact_digest", "")
        if not digest or not re.match(r"^sha256:[0-9a-f]{64}$", digest):
            reasons.append(f"Target '{tid}' has missing or malformed artifact_digest")

        source_sha = target.get("source_sha")
        if not source_sha or not str(source_sha).strip():
            reasons.append(f"Target '{tid}' has missing or blank source_sha")

        production_id = target.get("production_id")
        if not production_id or not str(production_id).strip():
            reasons.append(f"Target '{tid}' has missing or blank production_id")

        health_str = target.get("health_checked_at")
        if not health_str:
            reasons.append(f"Target '{tid}' is missing health_checked_at")
        else:
            try:
                dt_str = health_str.replace("Z", "+00:00")
                health_dt = datetime.fromisoformat(dt_str)
                if health_dt.tzinfo is None:
                    health_dt = health_dt.replace(tzinfo=timezone.utc)
                if now.tzinfo is None:
                    now = now.replace(tzinfo=timezone.utc)

                age = now - health_dt
                if age > max_evidence_age:
                    reasons.append(f"Target '{tid}' health evidence is older than max_evidence_age")
                if health_dt > now:
                    reasons.append(f"Target '{tid}' health evidence is dated in the future")
            except ValueError:
                reasons.append(f"Target '{tid}' has malformed health_checked_at")

        prev_id = target.get("previous_known_good_id")
        if prev_id and prev_id not in seen_ids:
            reasons.append(f"Target '{tid}' previous_known_good_id '{prev_id}' does not resolve to another record in the payload")

    return reasons

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("inventory", help="Path to the inventory JSON file")
    args = parser.parse_args()

    payload = load_inventory(args.inventory)
    now = datetime.now(timezone.utc)

    reasons = validate_inventory(payload, now)

    if not reasons:
        print("ok")
        sys.exit(0)
    else:
        for r in reasons:
            print(r)
        sys.exit(1)

if __name__ == "__main__":
    main()
