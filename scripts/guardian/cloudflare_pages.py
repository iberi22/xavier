import json
import sys
import argparse
import re
from datetime import datetime, timezone

def plan_rollback(decision: dict, target: dict, state: dict, api=None, dry_run: bool = True) -> dict:
    now = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")

    # Contract field first: a DecisionRecord carries previous_known_good_id.
    to_id = decision.get("previous_known_good_id") or decision.get("to_deployment_id")

    base_result = {
        "target_id": target.get("id"),
        "kind": "cf_pages",
        "action": "rollback",
        "dry_run": dry_run,
        "from_deployment_id": decision.get("deployment_id"),
        "to_deployment_id": to_id,
        "result": "skipped",
        "verified": False,
        "observed_at": now,
        "detail": ""
    }

    if target.get("kind") != "cf_pages":
        base_result["detail"] = "target kind not cf_pages"
        return base_result
    if decision.get("action") != "rollback":
        base_result["detail"] = "action not rollback"
        return base_result
    if decision.get("deployment_id") != target.get("production_id"):
        base_result["detail"] = "deployment mismatch"
        return base_result
    if not to_id:
        base_result["detail"] = "missing previous_known_good_id"
        return base_result

    retained = state.get("retained", {})
    if to_id not in retained:
        base_result["detail"] = "unretained deployment ID"
        return base_result

    digest = retained[to_id].get("artifact_digest", "")
    if not digest or not re.match(r"^sha256:[0-9a-f]{64}$", digest):
        base_result["detail"] = "malformed artifact_digest"
        return base_result

    if dry_run:
        base_result["detail"] = "dry run"
        return base_result

    if not api:
        base_result["result"] = "failed"
        base_result["detail"] = "no api"
        return base_result

    try:
        new_prod_id = api(to_id)
        if new_prod_id == to_id:
            base_result["result"] = "applied"
            base_result["verified"] = True
            base_result["detail"] = "rollback verified"
        else:
            base_result["result"] = "failed"
            base_result["detail"] = "rollback probe mismatch"
    except Exception:
        base_result["result"] = "failed"
        base_result["detail"] = "provider error"

    return base_result

def main():
    parser = argparse.ArgumentParser(description="Cloudflare Pages recovery adapter")
    parser.add_argument("decision", help="Path to decision.json")
    parser.add_argument("--inventory", required=True, help="Path to inventory.json")
    parser.add_argument("--state", required=True, help="Path to state.json")
    parser.add_argument("--apply", action="store_true", help="Apply rollback (default is dry run)")
    parser.add_argument("--json", action="store_true", help="Output JSON")

    try:
        args = parser.parse_args()

        with open(args.decision, 'r', encoding='utf-8') as f:
            decision = json.load(f)
        with open(args.inventory, 'r', encoding='utf-8') as f:
            inventory = json.load(f)
        with open(args.state, 'r', encoding='utf-8') as f:
            state = json.load(f)

    except Exception as e:
        sys.stderr.write("malformed input\n")
        sys.exit(2)

    targets = inventory.get("targets", [])
    target = {}
    for t in targets:
        if t.get("production_id") == decision.get("deployment_id"):
            target = t
            break

    dry_run = not args.apply

    result = plan_rollback(decision, target, state, api=None, dry_run=dry_run)

    if args.json:
        print(json.dumps(result))
    else:
        print(result)

    sys.exit(0)

if __name__ == "__main__":
    main()
