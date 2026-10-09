import argparse
import json
import re
import sys
from datetime import datetime, timezone

# Shared interface contract (PLAN.md §Interface contracts):
#   ActionResult {target_id, kind, action:"rollback", dry_run:bool,
#                 from_deployment_id, to_deployment_id,
#                 result in ("applied","skipped","failed"), verified:bool,
#                 observed_at, detail}
ACTION_RESULT_KEYS = ("target_id", "kind", "action", "dry_run", "from_deployment_id",
                      "to_deployment_id", "result", "verified", "observed_at", "detail")


def _utc_z() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def plan_recovery(decision: dict, target: dict, retained: dict, dry_run: bool = True, probe=None) -> dict:
    target_id = target.get("id", decision.get("target_id"))
    from_deployment_id = target.get("production_id")
    # A DecisionRecord carries previous_known_good_id; to_deployment_id is a fallback.
    requested_to = decision.get("to_deployment_id") or decision.get("previous_known_good_id")
    to_deployment_id = requested_to

    def make_result(result_val: str, verified: bool, detail: str, plan=None) -> dict:
        return {
            "target_id": target_id,
            "kind": "gh_pages",
            "action": "rollback",
            "dry_run": dry_run,
            "from_deployment_id": from_deployment_id,
            "to_deployment_id": to_deployment_id,
            "result": result_val,
            "verified": verified,
            "observed_at": _utc_z(),
            "detail": detail,
            "plan": plan,
        }

    if target.get("kind") != "gh_pages":
        return make_result("skipped", False, "Reject: target kind is not gh_pages")

    if decision.get("action") != "rollback":
        return make_result("skipped", False, "Reject: decision action is not rollback")

    req_sha = decision.get("source_sha") or decision.get("sha")
    if req_sha and req_sha != retained.get("source_sha"):
        return make_result("skipped", False, "Reject: arbitrary SHA provided")

    if requested_to != target.get("previous_known_good_id"):
        return make_result("skipped", False, "Reject: to_deployment_id is not previous_known_good_id")

    artifact_digest = retained.get("artifact_digest", "")
    if not isinstance(artifact_digest, str) or not re.match(r"^sha256:[0-9a-f]{64}$", artifact_digest):
        return make_result("skipped", False, "Reject: missing or malformed retained artifact")

    plan = {
        "to_deployment_id": requested_to,
        "source_sha": retained.get("source_sha"),
        "artifact_digest": artifact_digest,
    }

    if dry_run:
        return make_result("skipped", False, "Recovery planned", plan)

    if probe is not None:
        if not probe(plan):
            return make_result("failed", False, "Recovery applied but public-URL probe failed", plan)
        return make_result("applied", True, "Recovery applied and public-URL probe verified", plan)

    return make_result("applied", False, "Recovery applied", plan)


def main():
    parser = argparse.ArgumentParser(description="Docs recovery planner")
    parser.add_argument("decision", help="Decision JSON file")
    parser.add_argument("--inventory", required=True, help="Inventory JSON file")
    parser.add_argument("--state", required=True, help="State JSON file")
    parser.add_argument("--apply", action="store_true", help="Apply the recovery")
    parser.add_argument("--json", action="store_true", help="Output JSON")

    try:
        args = parser.parse_args()
        with open(args.decision, "r", encoding="utf-8") as f:
            decision = json.load(f)
        with open(args.inventory, "r", encoding="utf-8") as f:
            inventory = json.load(f)
        with open(args.state, "r", encoding="utf-8") as f:
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
