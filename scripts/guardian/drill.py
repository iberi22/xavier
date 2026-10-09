from datetime import datetime, timezone

# Simulated fault onset to detection latency used by the offline drills.
DETECTION_DELAY_SECONDS = 5


def _utc_z(dt: datetime | None) -> str | None:
    if dt is None:
        return None
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=timezone.utc)
    return dt.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def run_drill(target: dict, scenario: str, adapters: dict | None = None, now: datetime | None = None) -> dict:
    if now is None:
        now = datetime.now(timezone.utc)

    if adapters is None:
        adapters = {}

    target_id = target.get("id", "unknown_target")

    probe_fn = adapters.get("probe", lambda s: s != "false_alarm")
    decide_fn = adapters.get("decide", lambda s: s not in ("missing_known_good", "guardian_outage"))
    act_fn = adapters.get("act", lambda s: s != "rollback_failure")
    notify_fn = adapters.get("notify", lambda: None)

    detected_at = now
    decided_at = None
    restored_at = None
    escalated = False
    false_positive = False
    outcome = "failed"

    is_real_fault = probe_fn(scenario)

    if scenario == "false_alarm" or not is_real_fault:
        decided_at = now
        false_positive = True
        escalated = False
        outcome = "restored"
        notify_fn()
    else:
        decided_at = now
        can_rollback = decide_fn(scenario)
        if not can_rollback:
            escalated = True
            outcome = "escalated"
            notify_fn()
        else:
            success = act_fn(scenario)
            if success:
                restored_at = now
                outcome = "restored"
                notify_fn()
            else:
                escalated = True
                outcome = "failed"
                notify_fn()

    if scenario == "guardian_outage":
        decided_at = None
        restored_at = None
        escalated = True
        outcome = "failed"

    # DrillResult must carry exactly the contract keys; the simulated fault onset
    # is not a contract field, so consumers derive it from detected_at plus
    # DETECTION_DELAY_SECONDS (0 for a false positive).
    return {
        "target_id": target_id,
        "scenario": scenario,
        "detected_at": _utc_z(detected_at),
        "decided_at": _utc_z(decided_at),
        "restored_at": _utc_z(restored_at),
        "escalated": escalated,
        "false_positive": false_positive,
        "outcome": outcome,
    }
