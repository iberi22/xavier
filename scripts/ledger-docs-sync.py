import argparse
import json
import os
import sys

def main():
    parser = argparse.ArgumentParser(description="Sync ledger docs")
    parser.add_argument("--features", default=".gitcore/features.json", help="Path to features.json")
    parser.add_argument("--doc", default="docs/FEATURE_STATUS.md", help="Path to FEATURE_STATUS.md")
    group = parser.add_mutually_exclusive_group(required=True)
    group.add_argument("--write", action="store_true", help="Write to doc")
    group.add_argument("--check", action="store_true", help="Check if doc is up to date")

    args = parser.parse_args()

    try:
        with open(args.features, "r", encoding="utf-8") as f:
            features_data = json.load(f)
    except Exception as e:
        print(f"Error reading features file {args.features}: {e}", file=sys.stderr)
        sys.exit(1)

    features = features_data.get("features", {})

    categories = {}
    total = 0
    total_stable = 0
    total_beta = 0
    total_in_progress = 0
    total_planned = 0
    has_other = False

    for f_id, f_data in features.items():
        cat = f_data.get("category", "uncategorized")
        if not cat:
            cat = "uncategorized"

        if cat not in categories:
            categories[cat] = {"stable": 0, "beta": 0, "in_progress": 0, "planned": 0, "other": 0, "total": 0}

        status = f_data.get("status", "unknown")
        if status in ["stable", "beta", "in_progress", "planned"]:
            categories[cat][status] += 1
            if status == "stable": total_stable += 1
            elif status == "beta": total_beta += 1
            elif status == "in_progress": total_in_progress += 1
            elif status == "planned": total_planned += 1
        else:
            categories[cat]["other"] += 1
            has_other = True

        categories[cat]["total"] += 1
        total += 1

    pct = round((total_stable / total) * 100, 1) if total > 0 else 0.0

    lines = []
    lines.append(f"Ledger: {total_stable} stable, {total_beta} beta, {total_in_progress} in_progress, {total_planned} planned ({total} total, {pct}% stable). Generated from .gitcore/features.json by scripts/ledger-docs-sync.py; do not edit by hand.")
    lines.append("")

    headers = ["category", "stable", "beta", "in_progress", "planned", "total"]
    if has_other:
        headers.insert(5, "other")

    lines.append("| " + " | ".join(headers) + " |")
    lines.append("|" + "|".join(["---"] * len(headers)) + "|")

    for cat in sorted(categories.keys()):
        row = [
            cat,
            str(categories[cat]["stable"]),
            str(categories[cat]["beta"]),
            str(categories[cat]["in_progress"]),
            str(categories[cat]["planned"])
        ]
        if has_other:
            row.append(str(categories[cat]["other"]))
        row.append(str(categories[cat]["total"]))
        lines.append("| " + " | ".join(row) + " |")

    new_block = "\n".join(lines)

    try:
        with open(args.doc, "r", encoding="utf-8") as f:
            doc_content = f.read()
    except Exception as e:
        print(f"Error reading doc file {args.doc}: {e}", file=sys.stderr)
        sys.exit(1)

    start_marker = "<!-- ledger:summary:start -->"
    end_marker = "<!-- ledger:summary:end -->"

    start_idx = doc_content.find(start_marker)
    end_idx = doc_content.find(end_marker)

    if start_idx == -1 or end_idx == -1 or end_idx < start_idx:
        print(f"Markers not found in {args.doc}", file=sys.stderr)
        sys.exit(2)

    prefix = doc_content[:start_idx + len(start_marker)]
    suffix = doc_content[end_idx:]

    current_block = doc_content[start_idx + len(start_marker):end_idx].strip()

    if args.check:
        if current_block == new_block:
            sys.exit(0)
        else:
            print("Doc is stale.", file=sys.stderr)
            sys.exit(1)

    if args.write:
        new_content = prefix + "\n" + new_block + "\n" + suffix
        with open(args.doc, "w", encoding="utf-8") as f:
            f.write(new_content)
        sys.exit(0)

if __name__ == "__main__":
    main()
