import argparse
import datetime
import json
import sys

# Contracts (PLAN.md §Interface contracts): consumes FailureEvidence
# {target_id, kind, deployment_id, detected_at, failed_probes, window_seconds,
#  vantages, correlated, missing_metrics, summary}; emits DecisionRecord
# {target_id, kind, deployment_id, decided_at, action, reason, reversible,
#  previous_known_good_id, attempts_used, cooldown_until, evidence}.
EVIDENCE_FIELDS = {"target_id", "kind", "deployment_id", "detected_at", "failed_probes",
                   "window_seconds", "vantages", "correlated", "missing_metrics", "summary"}
OPTIONAL_SIGNAL_FIELDS = {"migration_flag", "schema_flag", "security_flag",
                          "unknown_attribution", "status", "signal"}
REVERSIBLE_KINDS = ("cf_pages", "cf_worker", "gh_pages")


def _utc_z(dt):
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=datetime.timezone.utc)
    return dt.astimezone(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def _cooldown_until(state, policy):
    last = state.get("last_action_time") if isinstance(state, dict) else None
    if not last:
        return None
    try:
        last_dt = datetime.datetime.fromisoformat(str(last).replace("Z", "+00:00"))
    except Exception:
        return None
    if last_dt.tzinfo is None:
        last_dt = last_dt.replace(tzinfo=datetime.timezone.utc)
    return _utc_z(last_dt + datetime.timedelta(minutes=(policy or {}).get("cooldown_minutes", 15)))


def decide(evidence, inventory, state, now, policy=None):
    ev = evidence if isinstance(evidence, dict) else {}
    ctx = {"target_id": ev.get("target_id"), "kind": ev.get("kind"),
           "deployment_id": ev.get("deployment_id"), "attempts_used": 0,
           "previous_known_good_id": None, "cooldown_until": None, "reversible": False}

    def emit(action, reason):
        try:
            decided_at = _utc_z(now)
        except Exception:
            decided_at = None
        return {
            "target_id": ctx["target_id"], "kind": ctx["kind"],
            "deployment_id": ctx["deployment_id"], "decided_at": decided_at,
            "action": action, "reason": reason, "reversible": bool(ctx["reversible"]),
            "previous_known_good_id": ctx["previous_known_good_id"],
            "attempts_used": int(ctx["attempts_used"]),
            "cooldown_until": ctx["cooldown_until"], "evidence": ev,
        }

    try:
        if not isinstance(evidence, dict) or not isinstance(inventory, dict) or not isinstance(state, dict):
            raise ValueError("Inputs must be dicts")
        unknown = set(ev) - (EVIDENCE_FIELDS | OPTIONAL_SIGNAL_FIELDS)
        if unknown:
            return emit("escalate", f"Unknown fields in evidence: {sorted(unknown)}")
        missing = EVIDENCE_FIELDS - set(ev)
        if missing:
            return emit("escalate", f"Missing evidence fields: {sorted(missing)}")

        ctx["target_id"] = ev["target_id"]
        ctx["kind"] = ev["kind"]
        ctx["deployment_id"] = ev["deployment_id"]
        correlated = ev["correlated"]
        failed_probes = ev.get("failed_probes") or []
        missing_metrics = ev.get("missing_metrics") or []
        vantages = ev.get("vantages") or []
        status, signal = ev.get("status"), ev.get("signal")
        flags = [ev.get("migration_flag"), ev.get("schema_flag"), ev.get("security_flag"),
                 ev.get("unknown_attribution")]
        if any(flags):
            return emit("escalate", "Failure involves migration, schema, security, or unknown attribution")

        # "unhealthy" is the failure signal; only missing metrics leave the status unknown.
        unhealthy = bool(failed_probes) or status == "unhealthy" or signal == "unhealthy"
        unknown_only = (not unhealthy) and (status == "unknown" or bool(missing_metrics) or not vantages)
        if not correlated and unknown_only:
            return emit("hold", "Status is unknown and nothing correlates")

        targets = inventory.get("targets", [])
        if not isinstance(targets, list):
            targets = []
        target = next((t for t in targets if isinstance(t, dict)
                       and t.get("production_id") == ctx["deployment_id"]), None)
        if target is None and ctx["target_id"] is not None:
            target = next((t for t in targets if isinstance(t, dict) and t.get("id") == ctx["target_id"]), None)
        if target is None:
            return emit("escalate", f"Failing deployment {ctx['deployment_id']} not found in inventory")

        ctx["target_id"] = target.get("id", ctx["target_id"])
        ctx["kind"] = target.get("kind", ctx["kind"])
        ctx["reversible"] = ctx["kind"] in REVERSIBLE_KINDS and not any(flags)
        if ctx["kind"] in ("local_binary", "release_tag"):
            return emit("escalate", f"Target kind {ctx['kind']} cannot be automatically rolled back")
        if ctx["kind"] not in REVERSIBLE_KINDS:
            return emit("escalate", f"Target kind {ctx['kind']} is not recognized for rollback")

        ctx["previous_known_good_id"] = target.get("previous_known_good_id")
        if not ctx["previous_known_good_id"]:
            return emit("escalate", "Missing previous_known_good_id")
        prev = next((t for t in targets if isinstance(t, dict)
                     and t.get("id") == ctx["previous_known_good_id"]), None)
        if prev is None:
            return emit("escalate", f"Previous target {ctx['previous_known_good_id']} missing from inventory")
        if prev.get("health_status") == "unhealthy":
            return emit("escalate", f"Previous target {ctx['previous_known_good_id']} is unhealthy")

        ctx["attempts_used"] = int((state.get("attempts") or {}).get(ctx["deployment_id"], 0) or 0)
        if ctx["attempts_used"] >= 1:
            return emit("escalate",
                        f"Deployment {ctx['deployment_id']} already has {ctx['attempts_used']} rollback attempts")

        ctx["cooldown_until"] = _cooldown_until(state, policy)
        last_action_str = state.get("last_action_time")
        if last_action_str:
            try:
                last = datetime.datetime.fromisoformat(str(last_action_str).replace("Z", "+00:00"))
                if last.tzinfo is None:
                    last = last.replace(tzinfo=datetime.timezone.utc)
                now_cmp = now if now.tzinfo is not None else now.replace(tzinfo=datetime.timezone.utc)
                if now_cmp - last < datetime.timedelta(minutes=(policy or {}).get("cooldown_minutes", 15)):
                    return emit("escalate", "Rollback cooldown has not elapsed")
            except Exception:
                return emit("escalate", "Malformed state last_action_time")

        if correlated:
            return emit("rollback", f"Correlated failure on reversible target {ctx['deployment_id']}")
        return emit("escalate", "Uncorrelated failure with bad signal")
    except Exception as e:
        return emit("escalate", f"Parse or shape error: {e}")


def render_decision(record):
    return json.dumps(record)


def main():
    class Parser(argparse.ArgumentParser):
        def error(self, message):
            print("Malformed input", file=sys.stderr)
            sys.exit(2)

    parser = Parser(description="Rollback guardian decision module")
    parser.add_argument("evidence", help="Path to evidence JSON")
    parser.add_argument("--inventory", required=True, help="Path to inventory JSON")
    parser.add_argument("--state", required=True, help="Path to state JSON")
    parser.add_argument("--json", action="store_true", help="Output JSON line")
    args = parser.parse_args()

    try:
        with open(args.evidence, "r", encoding="utf-8") as f:
            evidence = json.load(f)
        with open(args.inventory, "r", encoding="utf-8") as f:
            inventory = json.load(f)
    except Exception:
        print("Malformed input", file=sys.stderr)
        sys.exit(2)

    try:
        with open(args.state, "r", encoding="utf-8") as f:
            state = json.load(f)
    except Exception:
        state = None  # cannot prove attempts_used == 0, so decide() escalates

    print(render_decision(decide(evidence, inventory, state, datetime.datetime.now(datetime.timezone.utc))))
    sys.exit(0)


if __name__ == "__main__":
    main()
