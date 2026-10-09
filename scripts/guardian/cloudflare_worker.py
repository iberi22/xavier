import sys
import json
import argparse

def plan_rollback(decision: dict, target: dict, state: dict, api=None, dry_run: bool = True) -> dict:
    if target.get("kind") != "cf_worker":
        return {"result": "rejected", "verified": False, "detail": "Target kind is not cf_worker"}

    if decision.get("action") != "rollback":
        return {"result": "rejected", "verified": False, "detail": "Action is not rollback"}

    if decision.get("deployment_id") != target.get("production_id"):
        return {"result": "rejected", "verified": False, "detail": "Deployment ID mismatch"}

    version_id = decision.get("version_id", decision.get("target_version_id", decision.get("artifact_digest")))
    if not version_id:
        return {"result": "rejected", "verified": False, "detail": "Missing version ID"}

    retained_versions = state.get("retained_versions", {})
    if version_id not in retained_versions:
        return {"result": "rejected", "verified": False, "detail": "Target version ID absent from retained_versions"}

    retained = retained_versions[version_id]
    traffic = retained.get("traffic", retained.get("traffic_percentage", 0))
    if str(traffic) != "100":
        return {"result": "rejected", "verified": False, "detail": "Traffic percentage is not 100"}

    if dry_run:
        return {"result": "planned", "verified": False, "version_id": version_id, "detail": "Dry run"}

    if api is None:
        return {"result": "failed", "verified": False, "version_id": version_id, "detail": "No API injected"}

    try:
        api("rollback", version_id)
        probe = api("probe", version_id)

        if probe.get("version_id") == version_id and str(probe.get("traffic", "0")) == "100":
            return {"result": "verified", "verified": True, "version_id": version_id, "detail": "Verified route and deployment ID"}
        else:
            return {"result": "failed", "verified": False, "version_id": version_id, "detail": "Probe traffic mismatch or wrong version"}
    except Exception as e:
        err_msg = str(e)
        if "?" in err_msg:
            err_msg = err_msg.split("?")[0]
        if "token" in err_msg.lower() or "secret" in err_msg.lower():
            err_msg = "provider error (redacted)"
        return {"result": "failed", "verified": False, "version_id": version_id, "detail": err_msg}

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
