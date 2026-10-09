import json
import sys
import argparse
from datetime import datetime, timezone

def classify(event: dict, decision: dict, incident: dict | None = None) -> dict:
    kind = "source_revert"
    event_target = event.get("target", "")
    event_kind = event.get("kind", "")

    if "tag" in event:
        kind = "release_tag"
    if event_kind == "release_tag":
        kind = "release_tag"

    release_incident = {
        "incident_key": f"{event.get('target', 'unknown')}:{event.get('deployment_id', event.get('failing_sha', 'unknown'))}",
        "kind": kind,
        "recommended_version": None,
        "tag_moved": False,
        "source_revert": None,
        "state": "escalated",
        "opened_at": event.get("opened_at") or (datetime.now(timezone.utc).isoformat() + "Z")
    }

    evidence = (incident.get("evidence", {}) if incident else {}) or decision.get("evidence", {}) or event.get("evidence", {})

    last_known_good = evidence.get("previous_known_good_id")
    if last_known_good:
        release_incident["recommended_version"] = last_known_good
    elif incident and incident.get("previous_known_good_id"):
        release_incident["recommended_version"] = incident.get("previous_known_good_id")
    elif decision and decision.get("previous_known_good_id"):
        release_incident["recommended_version"] = decision.get("previous_known_good_id")

    if kind == "source_revert":
        is_migration = event.get("migration_signal") or decision.get("migration_signal") or (incident and incident.get("migration_signal"))
        is_schema = event.get("schema_signal") or decision.get("schema_signal") or (incident and incident.get("schema_signal"))
        is_security = event.get("security_signal") or decision.get("security_signal") or (incident and incident.get("security_signal"))

        attribution = decision.get("attribution", {})
        is_confident = attribution.get("confident", False)
        commits = attribution.get("commits", [])
        reversible = attribution.get("reversible", False)

        if is_migration or is_schema or is_security or not is_confident or len(commits) != 1 or not reversible:
            release_incident["state"] = "escalated"
            release_incident["source_revert"] = None
        else:
            release_incident["state"] = "escalated"
            release_incident["source_revert"] = {
                "sha": commits[0],
                "revert_pr": None,
                "panel_approved": False
            }

    return release_incident

def main():
    parser = argparse.ArgumentParser(description="Classify a release incident")
    parser.add_argument("event_file", help="Path to event JSON")
    parser.add_argument("--decision", required=True, help="Path to decision JSON")
    parser.add_argument("--incident", help="Path to incident JSON")
    parser.add_argument("--json", action="store_true", help="Output as JSON")

    try:
        args = parser.parse_args()
    except Exception as e:
        sys.stderr.write("Malformed input arguments\n")
        sys.exit(2)

    try:
        with open(args.event_file, "r") as f:
            event = json.load(f)
        with open(args.decision, "r") as f:
            decision = json.load(f)
        incident = None
        if args.incident:
            with open(args.incident, "r") as f:
                incident = json.load(f)

        if not isinstance(event, dict) or not isinstance(decision, dict):
            raise ValueError("event and decision must be JSON objects")

        result = classify(event, decision, incident)
        if args.json:
            print(json.dumps(result, indent=2))
        else:
            print(json.dumps(result))
        sys.exit(0)
    except Exception as e:
        sys.stderr.write("Malformed input\n")
        sys.exit(2)

if __name__ == "__main__":
    try:
        main()
    except SystemExit:
        raise
    except:
        sys.stderr.write("Malformed input\n")
        sys.exit(2)
