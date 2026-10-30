import json
import sys

def verify_stable_tests(features_data, log_text):
    raw = features_data.get("features", [])
    feats = list(raw.values()) if isinstance(raw, dict) else raw

    executed_tests = set()
    current_test = None
    for line in log_text.split('\n'):
        line = line.strip()
        if line.startswith("▶ "):
            t = line[2:].strip()
            if t.endswith(" [candidate]"):
                t = t[:-12].strip()
            current_test = t
        elif current_test and "✅" in line and "passed=" in line:
            executed_tests.add(current_test)

    missing_errors = []
    for f in feats:
        if f.get("status") == "stable":
            tests = f.get("tests")
            if tests is None:
                tests = []
            elif isinstance(tests, str):
                tests = [tests]

            for t in tests:
                if t not in executed_tests:
                    fid = f.get('id', f.get('name', 'unknown'))
                    missing_errors.append(f"❌ Feature '{fid}' declared test '{t}' which was not executed or is missing.")

    return missing_errors

def main():
    if len(sys.argv) != 3:
        print("Usage: python scripts/check_stable_tests.py <features.json> <ledger.log>")
        sys.exit(2)

    features_path = sys.argv[1]
    log_path = sys.argv[2]

    with open(features_path, 'r') as f:
        features_data = json.load(f)

    with open(log_path, 'r', encoding='utf-8', errors='ignore') as f:
        log_text = f.read()

    errors = verify_stable_tests(features_data, log_text)
    if errors:
        for err in errors:
            print(err)
        sys.exit(1)
    else:
        print("✅ All stable feature tests were executed.")
        sys.exit(0)

if __name__ == "__main__":
    main()
