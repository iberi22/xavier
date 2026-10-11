import os
import json
import re
import sys
import argparse
import copy
from datetime import datetime, timezone

def redact(payload: dict) -> dict:
    result = copy.deepcopy(payload)
    key_pattern = re.compile(r'token|secret|password|api[_-]?key|authorization|cookie|credential', re.IGNORECASE)
    val_pattern = re.compile(r'Bearer\s+|ghp_|sk-')

    def _redact(obj):
        if isinstance(obj, dict):
            for k, v in obj.items():
                if key_pattern.search(k):
                    obj[k] = '[REDACTED]'
                elif isinstance(v, str) and val_pattern.search(v):
                    obj[k] = '[REDACTED]'
                else:
                    _redact(v)
        elif isinstance(obj, list):
            for i, item in enumerate(obj):
                if isinstance(item, str) and val_pattern.search(item):
                    obj[i] = '[REDACTED]'
                else:
                    _redact(item)
    
    _redact(result)
    return result

CONTRACT_SEVERITIES = ("critical", "warning", "info")
CONTRACT_STATES = ("open", "resolved", "escalated")


def _deployment_id(event: dict) -> str:
    """Derive the deployment id for the incident key.

    ActionResult has no deployment_id, so fall back to its from/to ids. A real
    event therefore never degrades to "unknown_deployment".
    """
    for key in ("deployment_id", "from_deployment_id", "to_deployment_id",
                "previous_known_good_id", "failing_sha"):
        value = event.get(key)
        if value:
            return str(value)
    return "unknown_deployment"


def record_incident(existing: list[dict], event: dict, now: datetime) -> tuple[dict, list[dict]]:
    new_list = copy.deepcopy(existing)

    target_id = event.get('target_id') or 'unknown_target'
    deployment_id = _deployment_id(event)
    incident_key = f"{target_id}:{deployment_id}"

    now_str = now.isoformat()

    severity = event.get('severity')
    if severity not in CONTRACT_SEVERITIES:
        severity = 'warning'

    state = event.get('state')
    if state not in CONTRACT_STATES:
        state = 'open'

    # The contract IncidentRecord carries evidence as a dict.
    evidence = dict(event)

    record = None
    for r in new_list:
        if r.get('incident_key') == incident_key:
            record = r
            break

    if record:
        record['updated_at'] = now_str
        record['severity'] = severity
        record['state'] = state
        record['evidence'] = evidence
        returned_record = record
    else:
        returned_record = {
            'incident_key': incident_key,
            'target_id': target_id,
            'kind': event.get('kind') or 'unknown',
            'severity': severity,
            'state': state,
            'opened_at': event.get('opened_at') or now_str,
            'updated_at': now_str,
            'evidence': evidence,
            'notification': {'attempts': 0, 'delivered': False, 'channel': 'default'},
        }
        new_list.append(returned_record)

    return returned_record, new_list

def notify(record: dict, send=None, attempts: int = 2) -> dict:
    if send is None:
        def default_send(rec):
            pass
        send = default_send
        
    delivered = False
    for attempt in range(attempts):
        try:
            send(record)
            delivered = True
            break
        except Exception:
            pass
            
    record['notification'] = {
        'attempts': attempts if not delivered else attempt + 1,
        'delivered': delivered,
        'channel': 'default'
    }
    return record

def write_state(path: str, records: list[dict]) -> None:
    tmp_path = path + '.tmp'
    with open(tmp_path, 'w', encoding='utf-8') as f:
        json.dump(records, f)
    os.replace(tmp_path, path)

def main():
    parser = argparse.ArgumentParser(description="Record incident and notify")
    parser.add_argument("event", help="Path to event JSON file")
    parser.add_argument("--state", help="Path to state JSON file", required=True)
    parser.add_argument("--json", action="store_true", help="Output JSON")
    
    args = parser.parse_args()
    
    try:
        with open(args.event, 'r', encoding='utf-8') as f:
            event = json.load(f)
    except Exception as e:
        print(f"Malformed event input", file=sys.stderr)
        sys.exit(2)
        
    if os.path.exists(args.state):
        try:
            with open(args.state, 'r', encoding='utf-8') as f:
                existing_records = json.load(f)
        except Exception:
            print(f"Malformed state input", file=sys.stderr)
            sys.exit(2)
    else:
        existing_records = []
        
    now = datetime.now(timezone.utc)
    
    redacted_event = redact(event)
    record, new_records = record_incident(existing_records, redacted_event, now)
    final_record = notify(record)
    
    write_state(args.state, new_records)
    
    if args.json:
        print(json.dumps(final_record))
    else:
        print(f"Recorded incident {final_record.get('incident_key')} (delivered: {final_record.get('notification', {}).get('delivered')})")

if __name__ == "__main__":
    main()
