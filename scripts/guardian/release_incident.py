import json
import sys
import argparse
def classify(event: dict, decision: dict, incident: dict | None = None) -> dict:
    """
    Consumes event, decision, and incident dicts and emits a ReleaseIncident dict.
    Does not mutate inputs, does not write files, does not log personal paths.
    """

    # Do not mutate input dicts
    # In Python, we can just build a new dict.

    # Determine the kind
    kind = "source_revert"
    event_target = event.get("target", "")
    event_kind = event.get("kind", "")

    # "kind is "release_tag" when the event names a published tag, otherwise "source_revert""
    if event_kind == "release_tag":
        kind = "release_tag"
    elif "tag" in event_target.lower() or event.get("tag"): # simplistic heuristic if 'kind' isn't explicitly release_tag but implies it
         # The spec says "when the event names a published tag", we assume this means event["kind"] == "release_tag"
         pass

    # Actually, the instructions say:
    # "kind is `"release_tag"` when the event names a published tag, otherwise `"source_revert"`."
    if "tag" in event:
        kind = "release_tag"
    if event_kind == "release_tag":
        kind = "release_tag"
    release_incident = {
        "kind": kind,
        "tag_moved": False, # "tag_moved is always False and no field ever proposes moving a tag"
        "state": "escalated",
        "revert_plan": None,
        "recommended_version": None
    }

    # "recommended_version is the last known-good version from the incident evidence"
    evidence = incident.get("evidence", {}) if incident else {}
    if not evidence and decision.get("evidence"):
        evidence = decision.get("evidence", {})
    if not evidence and event.get("evidence"):
         evidence = event.get("evidence", {})

    last_known_good = evidence.get("previous_known_good_id")
    if last_known_good:
        release_incident["recommended_version"] = last_known_good
    elif incident and incident.get("previous_known_good_id"):
        release_incident["recommended_version"] = incident.get("previous_known_good_id")
    elif decision and decision.get("previous_known_good_id"):
         release_incident["recommended_version"] = decision.get("previous_known_good_id")

    if kind == "source_revert":
        is_migration = event.get("migration_signal") or decision.get("migration_signal") or incident and incident.get("migration_signal")
        is_schema = event.get("schema_signal") or decision.get("schema_signal") or incident and incident.get("schema_signal")
        is_security = event.get("security_signal") or decision.get("security_signal") or incident and incident.get("security_signal")

        # Check attribution
        attribution = decision.get("attribution", {})
        is_confident = attribution.get("confident", False)
        commits = attribution.get("commits", [])
        reversible = attribution.get("reversible", False)

        if is_migration or is_schema or is_security or not is_confident or len(commits) != 1 or not reversible:
            release_incident["state"] = "escalated"
            release_incident["revert_plan"] = None
        else:
            release_incident["state"] = "revert_planned"
            release_incident["revert_plan"] = {
                "commit": commits[0],
                "panel_approved": False,
                "auto_merge": False
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
