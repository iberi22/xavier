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

def record_incident(existing: list[dict], event: dict, now: datetime) -> tuple[dict, list[dict]]:
    new_list = copy.deepcopy(existing)
    
    target_id = event.get('target_id', 'unknown_target')
    deployment_id = event.get('deployment_id', 'unknown_deployment')
    incident_key = f"{target_id}:{deployment_id}"
    
    now_str = now.isoformat()
    
    record = None
    for r in new_list:
        if r.get('incident_key') == incident_key:
            record = r
            break
            
    if record:
        record['updated_at'] = now_str
        if 'state' in event:
            record['state'] = event['state']
        if 'evidence' in event:
            record['evidence'] = event['evidence']
        returned_record = record
    else:
        new_record = copy.deepcopy(event)
        new_record['incident_key'] = incident_key
        new_record['created_at'] = now_str
        new_record['updated_at'] = now_str
        new_list.append(new_record)
        returned_record = new_record
        
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
