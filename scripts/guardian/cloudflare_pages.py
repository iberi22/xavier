import json
import sys
import argparse
import re

def plan_rollback(decision: dict, target: dict, state: dict, api=None, dry_run: bool = True) -> dict:
    if target.get("kind") != "cf_pages":
        return {"result": "rejected", "verified": False, "detail": "target kind not cf_pages"}
    if decision.get("action") != "rollback":
        return {"result": "rejected", "verified": False, "detail": "action not rollback"}
    if decision.get("deployment_id") != target.get("production_id"):
        return {"result": "rejected", "verified": False, "detail": "deployment mismatch"}
    if decision.get("to_deployment_id") != decision.get("previous_known_good_id"):
        return {"result": "rejected", "verified": False, "detail": "to_deployment_id mismatch"}

    to_id = decision.get("to_deployment_id")
    retained = state.get("retained", {})
    if to_id not in retained:
        return {"result": "rejected", "verified": False, "detail": "unretained deployment ID"}

    digest = retained[to_id].get("artifact_digest", "")
    if not digest or not re.match(r"^sha256:[0-9a-f]{64}$", digest):
        return {"result": "rejected", "verified": False, "detail": "malformed artifact_digest"}

    if dry_run:
        return {"result": "planned", "verified": False, "to_deployment_id": to_id, "detail": "dry run"}

    if not api:
        return {"result": "failed", "verified": False, "detail": "no api"}

    try:
        new_prod_id = api(to_id)
        if new_prod_id == to_id:
            return {"result": "planned", "verified": True, "to_deployment_id": to_id, "detail": "rollback verified"}
        else:
            return {"result": "failed", "verified": False, "detail": "rollback probe mismatch"}
    except Exception:
        return {"result": "failed", "verified": False, "detail": "provider error"}

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

    # Simplified lookup assuming single target matching decision
    # In a real scenario, we'd lookup by id
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
