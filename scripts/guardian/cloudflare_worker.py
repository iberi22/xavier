import sys
import json
import argparse
from datetime import datetime, timezone

def plan_rollback(decision: dict, target: dict, state: dict, api=None, dry_run: bool = True) -> dict:
    target_id = target.get("id", decision.get("target_id"))
    from_deployment_id = target.get("production_id")

    def make_result(result_val: str, verified: bool, to_deployment_id: str, detail: str) -> dict:
        return {
            "target_id": target_id,
            "kind": "cf_worker",
            "action": "rollback",
            "dry_run": dry_run,
            "from_deployment_id": from_deployment_id,
            "to_deployment_id": to_deployment_id,
            "result": result_val,
            "verified": verified,
            "observed_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "detail": detail
        }

    if target.get("kind") != "cf_worker":
        return make_result("skipped", False, None, "Target kind is not cf_worker")

    if decision.get("action") != "rollback":
        return make_result("skipped", False, None, "Action is not rollback")

    if decision.get("deployment_id") != from_deployment_id:
        return make_result("skipped", False, None, "Deployment ID mismatch")

    # Contract field first: a DecisionRecord carries previous_known_good_id.
    version_id = (decision.get("previous_known_good_id")
                  or decision.get("to_deployment_id")
                  or decision.get("version_id")
                  or decision.get("target_version_id"))
    if not version_id:
        return make_result("skipped", False, None, "Missing version ID")

    retained_versions = state.get("retained_versions", {})
    if version_id not in retained_versions:
        return make_result("skipped", False, None, "Target version ID absent from retained_versions")

    retained = retained_versions[version_id]
    traffic = retained.get("traffic", retained.get("traffic_percentage", 0))
    if str(traffic) != "100":
        return make_result("skipped", False, None, "Traffic percentage is not 100")

    if dry_run:
        return make_result("skipped", False, version_id, "Dry run")

    if api is None:
        return make_result("failed", False, version_id, "No API injected")

    try:
        api("rollback", version_id)
        probe = api("probe", version_id)

        if probe.get("version_id") == version_id and str(probe.get("traffic", "0")) == "100":
            return make_result("applied", True, version_id, "Verified route and deployment ID")
        else:
            return make_result("failed", False, version_id, "Probe traffic mismatch or wrong version")
    except Exception as e:
        err_msg = str(e)
        if "?" in err_msg:
            err_msg = err_msg.split("?")[0]
        if "token" in err_msg.lower() or "secret" in err_msg.lower():
            err_msg = "provider error (redacted)"
        return make_result("failed", False, version_id, err_msg)

def main():
    parser = argparse.ArgumentParser(description="Cloudflare Worker version recovery adapter")
    parser.add_argument("decision")
    parser.add_argument("--inventory", required=True)
    parser.add_argument("--state", required=True)
    parser.add_argument("--apply", action="store_true")
    parser.add_argument("--json", action="store_true")

    try:
        args = parser.parse_args()

        with open(args.decision, "r", encoding="utf-8") as f:
            decision = json.load(f)
        with open(args.inventory, "r", encoding="utf-8") as f:
            inventory = json.load(f)
        with open(args.state, "r", encoding="utf-8") as f:
            state = json.load(f)

        target_id = decision.get("target_id")
        targets = inventory.get("targets", [])
        target = next((t for t in targets if t.get("id") == target_id), {})

        dry_run = not args.apply
        res = plan_rollback(decision, target, state, api=None, dry_run=dry_run)

        if args.json:
            print(json.dumps(res))
        else:
            print(res)

        sys.exit(0)
    except SystemExit:
        raise
    except Exception as e:
        print(f"Error: malformed input or missing file", file=sys.stderr)
        sys.exit(2)

if __name__ == "__main__":
    main()
