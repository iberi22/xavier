import argparse
import json
import re
import sys

def plan_recovery(decision: dict, target: dict, retained: dict, dry_run: bool = True, probe=None) -> dict:
    action_result = {
        "status": "rejected",
        "detail": "",
        "action": decision.get("action", ""),
        "target_id": target.get("id", ""),
        "plan": None,
        "verified": False,
    }

    if target.get("kind") != "gh_pages":
        action_result["detail"] = "Reject: target kind is not gh_pages"
        return action_result

    if decision.get("action") != "rollback":
        action_result["detail"] = "Reject: decision action is not rollback"
        return action_result

    req_sha = decision.get("source_sha") or decision.get("sha")
    if req_sha and req_sha != retained.get("source_sha"):
        action_result["detail"] = "Reject: arbitrary SHA provided"
        return action_result

    if decision.get("to_deployment_id") != target.get("previous_known_good_id"):
        action_result["detail"] = "Reject: to_deployment_id is not previous_known_good_id"
        return action_result

    artifact_digest = retained.get("artifact_digest", "")
    if not isinstance(artifact_digest, str) or not re.match(r"^sha256:[0-9a-f]{64}$", artifact_digest):
        action_result["detail"] = "Reject: missing or malformed retained artifact"
        return action_result

    plan = {
        "to_deployment_id": decision.get("to_deployment_id"),
        "source_sha": retained.get("source_sha"),
        "artifact_digest": artifact_digest
    }
    action_result["plan"] = plan
    action_result["status"] = "planned" if dry_run else "applied"
    action_result["detail"] = "Recovery planned" if dry_run else "Recovery applied"

    if not dry_run and probe is not None:
        action_result["verified"] = probe(plan)

    return action_result

def main():
    parser = argparse.ArgumentParser(description="Docs recovery planner")
    parser.add_argument("decision", help="Decision JSON file")
    parser.add_argument("--inventory", required=True, help="Inventory JSON file")
    parser.add_argument("--state", required=True, help="State JSON file")
    parser.add_argument("--apply", action="store_true", help="Apply the recovery")
    parser.add_argument("--json", action="store_true", help="Output JSON")

    try:
        args = parser.parse_args()
        with open(args.decision, 'r') as f:
            decision = json.load(f)
        with open(args.inventory, 'r') as f:
            inventory = json.load(f)
        with open(args.state, 'r') as f:
            state = json.load(f)

        target = {}
        for t in inventory.get("targets", []):
            if t.get("id") == decision.get("target_id"):
                target = t
                break

        retained = state.get("retained", {}) or target.get("retained", {})

        res = plan_recovery(decision, target, retained, dry_run=not args.apply)
        if args.json:
            print(json.dumps(res))
        else:
            print(res)

    except Exception as e:
        sys.stderr.write(f"Malformed input: {e}\n")
        sys.exit(2)

if __name__ == "__main__":
    main()
