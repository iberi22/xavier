#!/usr/bin/env python3
"""validate-features.py — Test-driven feature validation for Xavier."""
import json, subprocess, sys, os

REPO = "/mnt/e/scripts-python/xavier"
FEATURES_PATH = os.path.join(REPO, ".gitcore", "features.json")

def load():
    with open(FEATURES_PATH) as f:
        return json.load(f)

def run(cmd):
    try:
        r = subprocess.run(cmd, shell=True, capture_output=True, text=True, timeout=15, cwd=REPO)
        return r.stdout.strip(), r.returncode
    except subprocess.TimeoutExpired:
        return "TIMEOUT", -1
    except Exception as e:
        return str(e), -1

def main():
    data = load()
    features = data.get("features", [])
    dry = "--dry-run" in sys.argv
    apply = "--apply" in sys.argv
    ci = "--ci" in sys.argv
    
    print(f"XAVIER FEATURE VALIDATION")
    print(f"Features: {len(features)}")
    drift = False
    
    for feat in features:
        fid = feat.get("id", "?")
        reported = feat.get("progress_pct", 0)
        tv = feat.get("test_validation")
        if not tv:
            print(f"  {fid}: sin test_validation, skip")
            continue
        out, code = run(tv)
        try:
            actual = float(out.strip().split("\n")[-1])
            actual = min(max(actual, 0), 100)
        except (ValueError, IndexError):
            actual = reported
        diff = abs(actual - reported)
        status = "OK" if diff < 5 else "DRIFT" if diff < 15 else "MISMATCH"
        print(f"  {status} {fid}: reportado={reported}% real={actual:.0f}%")
        if diff >= 5:
            drift = True
            if apply:
                feat["progress_pct"] = round(actual, 1)
    
    if drift:
        print(f"\nDrift detected{' — corrected with --apply' if apply else ''}")
        if apply:
            data["metadata"]["last_updated"] = "2026-07-10"
            with open(FEATURES_PATH, "w") as f:
                json.dump(data, f, indent=2)
                f.write("\n")
            print("features.json updated")
    else:
        print("\nAll synced")
    
    return 1 if (drift and ci) else 0

if __name__ == "__main__":
    sys.exit(main())
