from datetime import datetime, timezone

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

    if scenario == "missing_known_good":
        restored_at = None
        escalated = True
        outcome = "escalated"

    return {
        "target_id": target_id,
        "scenario": scenario,
        "detected_at": detected_at.isoformat() if detected_at else None,
        "decided_at": decided_at.isoformat() if decided_at else None,
        "restored_at": restored_at.isoformat() if restored_at else None,
        "escalated": escalated,
        "false_positive": false_positive,
        "outcome": outcome
    }
