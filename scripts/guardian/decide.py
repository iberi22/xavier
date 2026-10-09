import argparse
import datetime
import json
import sys

def decide(evidence: dict, inventory: dict, state: dict, now: datetime.datetime, policy: dict | None = None) -> dict:
    """
    Evaluates failure evidence against an inventory and state to produce a deterministic decision record.
    Returns exactly one DecisionRecord dict without mutating inputs.
    """
    try:
        if not isinstance(evidence, dict) or not isinstance(inventory, dict) or not isinstance(state, dict):
            raise ValueError("Inputs must be dicts")

        # Unknown fields check
        allowed_fields = {"deployment_id", "correlated", "signal", "migration_flag", "schema_flag", "security_flag", "unknown_attribution", "status"}
        unknown_fields = set(evidence.keys()) - allowed_fields
        if unknown_fields:
            return {"action": "escalate", "reason": f"Unknown fields in evidence: {unknown_fields}", "summary": "Escalate: unknown evidence fields"}

        deployment_id = evidence.get("deployment_id")
        if not deployment_id:
            return {"action": "escalate", "reason": "Missing deployment_id in evidence", "summary": "Escalate: bad evidence schema"}

        correlated = evidence.get("correlated")
        signal = evidence.get("signal")
        migration_flag = evidence.get("migration_flag", False)
        schema_flag = evidence.get("schema_flag", False)
        security_flag = evidence.get("security_flag", False)
        unknown_attribution = evidence.get("unknown_attribution", False)
        status = evidence.get("status")

        if correlated is False and status == "unknown":
            return {"action": "hold", "reason": "Status is unknown and not correlated", "summary": "Hold: unknown status"}

        if not correlated and signal == "unhealthy":
             return {"action": "escalate", "reason": "Uncorrelated unhealthy signal", "summary": "Escalate: uncorrelated unhealthy signal"}

        if migration_flag or schema_flag or security_flag or unknown_attribution:
            return {"action": "escalate", "reason": "Failure involves migration, schema, security, or unknown attribution", "summary": "Escalate: unsafe flags set"}

        targets = inventory.get("targets", [])
        target = next((t for t in targets if t.get("production_id") == deployment_id), None)

        if not target:
            return {"action": "escalate", "reason": f"Failing deployment {deployment_id} not found in inventory", "summary": "Escalate: missing target"}

        kind = target.get("kind")
        if kind in ("local_binary", "release_tag"):
            return {"action": "escalate", "reason": f"Target kind {kind} cannot be automatically rolled back", "summary": "Escalate: irreversible kind"}

        if kind not in ("cf_pages", "cf_worker", "gh_pages"):
             return {"action": "escalate", "reason": f"Target kind {kind} is not recognized for rollback", "summary": "Escalate: unknown kind"}

        prev_id = target.get("previous_known_good_id")
        if not prev_id:
            return {"action": "escalate", "reason": "Missing previous_known_good_id", "summary": "Escalate: missing previous target"}

        prev_target = next((t for t in targets if t.get("id") == prev_id), None)
        if not prev_target:
            return {"action": "escalate", "reason": f"Previous target {prev_id} missing from inventory", "summary": "Escalate: missing previous target"}

        # Check unhealthy previous target
        prev_health = prev_target.get("health_status")
        # In real targets inventory health evidence might just be absence of unhealthy signals,
        # but to be sure we ensure it doesn't explicitly flag as unhealthy.
        if prev_health == "unhealthy":
            return {"action": "escalate", "reason": f"Previous target {prev_id} is unhealthy", "summary": "Escalate: unhealthy previous target"}

        attempts_dict = state.get("attempts", {})
        attempts_used = attempts_dict.get(deployment_id, 0)

        if attempts_used >= 1:
            return {"action": "escalate", "reason": f"Deployment {deployment_id} already has {attempts_used} rollback attempts", "summary": "Escalate: attempts exceeded"}

        last_action_str = state.get("last_action_time")
        if last_action_str:
            try:
                last_action = datetime.datetime.fromisoformat(last_action_str.replace("Z", "+00:00"))
                if last_action.tzinfo is None:
                    last_action = last_action.replace(tzinfo=datetime.timezone.utc)
                if now.tzinfo is None:
                    now = now.replace(tzinfo=datetime.timezone.utc)

                cooldown_minutes = (policy or {}).get("cooldown_minutes", 15)
                cooldown = datetime.timedelta(minutes=cooldown_minutes)

                if now - last_action < cooldown:
                    return {"action": "escalate", "reason": "Rollback cooldown has not elapsed", "summary": "Escalate: cooldown active"}
            except Exception:
                return {"action": "escalate", "reason": "Malformed state last_action_time", "summary": "Escalate: parse error"}

        if correlated:
            return {"action": "rollback", "reason": f"Correlated failure on reversible target {deployment_id}", "summary": "Rollback: proceeding"}

        return {"action": "escalate", "reason": "Uncorrelated failure with bad signal", "summary": "Escalate: uncategorized failure"}

    except Exception as e:
        return {"action": "escalate", "reason": f"Parse or shape error: {e}", "summary": "Escalate: parse error"}


def render_decision(record: dict) -> str:
    return json.dumps(record)


def main():
    # Override exit to return 2 and print single line on stderr
    class ArgumentParserWithCustomError(argparse.ArgumentParser):
        def error(self, message):
            print("Malformed input", file=sys.stderr)
            sys.exit(2)

    parser = ArgumentParserWithCustomError(description="Rollback guardian decision module")
    parser.add_argument("evidence", help="Path to evidence JSON")
    parser.add_argument("--inventory", required=True, help="Path to inventory JSON")
    parser.add_argument("--state", required=True, help="Path to state JSON")
    parser.add_argument("--json", action="store_true", help="Output JSON line")

    args = parser.parse_args()

    try:
        with open(args.evidence, "r") as f:
            evidence = json.load(f)
    except Exception:
        print("Malformed input", file=sys.stderr)
        sys.exit(2)

    try:
        with open(args.inventory, "r") as f:
            inventory = json.load(f)
    except Exception:
        print("Malformed input", file=sys.stderr)
        sys.exit(2)

    try:
        with open(args.state, "r") as f:
            state = json.load(f)
    except Exception:
        # A missing or unreadable state file means attempts_used cannot be proven 0, must escalate.
        state = None

    now = datetime.datetime.now(datetime.timezone.utc)

    if state is None:
        record = {"action": "escalate", "reason": "State file missing or unreadable", "summary": "Escalate: missing state"}
    else:
        record = decide(evidence, inventory, state, now)

    print(render_decision(record))
    sys.exit(0)

if __name__ == "__main__":
    main()
