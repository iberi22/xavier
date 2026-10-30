#!/usr/bin/env python3
"""Ledger gap issues manager.

Finds gaps from the ledger promoter report and manages GitHub issues.
"""
import argparse
import json
import os
import subprocess
import sys
from pathlib import Path


class GithubRunner:
    def __init__(self, repo, dry_run=False):
        self.repo = repo
        self.dry_run = dry_run

    def gh(self, *args, **kwargs):
        check = kwargs.pop("check", True)
        if self.dry_run and args[0] in ("issue", "label") and args[1] in ("create", "edit", "close", "reopen"):
            print(f"[DRY-RUN] gh {' '.join(map(str, args))}")
            return ""

        cmd = ["gh", *args]
        result = subprocess.run(cmd, capture_output=True, text=True, check=check, **kwargs)
        return result.stdout

    def create_label_if_missing(self, name, description, color):
        try:
            self.gh("label", "create", name, "--description", description, "--color", color, "--repo", self.repo, check=False)
        except Exception:
            pass


def get_gap_reasons(reasons):
    gap_prefixes = (
        "declared tests failed",
        "declared test missing from index",
        "no linked integration/E2E evidence",
        "linked test is BUG-ignored",
        "declared test evidence differs from ledger",
    )
    return [r for r in reasons if any(r.startswith(p) for p in gap_prefixes)]


def find_gaps(features_json, report_json):
    features_doc = json.loads(Path(features_json).read_text(encoding="utf-8"))
    report = json.loads(Path(report_json).read_text(encoding="utf-8"))

    gaps = {}
    security_needs_owner = []

    for fid, feat in features_doc.get("features", {}).items():
        if feat.get("status") not in ("beta", "in_progress", "planned"):
            if feat.get("category") == "security":
                security_needs_owner.append(fid)
            continue

        rep_feat = report.get("features", {}).get(fid, {})
        reasons = rep_feat.get("reasons", [])
        gap_reasons = get_gap_reasons(reasons)

        if not gap_reasons:
            if feat.get("category") == "security":
                security_needs_owner.append(fid)
            continue

        if feat.get("category") == "security":
            security_needs_owner.append(fid)
            continue

        gaps[fid] = {
            "name": feat.get("name", ""),
            "implemented_in": feat.get("implemented_in", []),
            "tests": feat.get("tests", []),
            "reasons": reasons,
            "first_gap_reason": gap_reasons[0][:60],
            "sha": report.get("sha", "unknown"),
        }

    return gaps, set(security_needs_owner), report.get("evidence_complete", False), report.get("sha", "unknown")


def format_issue_body(fid, gap):
    marker = f"<!-- ledger-gap:{fid} -->"
    body = [
        marker,
        f"**Feature:** {gap['name']}",
        f"**Implemented in:** {', '.join(gap['implemented_in']) if gap['implemented_in'] else 'none'}",
        f"**Declared tests:** {', '.join(gap['tests']) if gap['tests'] else 'none'}",
        "",
        "**Reasons for gap:**",
        *[f"- {r}" for r in gap["reasons"]],
        "",
        f"**Evaluated SHA:** {gap['sha']}",
        "",
        "### How to fix",
        "- Add or fix an integration/E2E test registered in `tests/suite_NN.rs` via `#[path]` + `mod`",
        "- New top-level test files contain `xavier::isolate_test_process!();`",
        "- Append test ids to the feature's `tests` array only",
        "- Never edit status/progress_pct/passes/last_tested",
        "- Use temp dirs only",
        "- No network except 127.0.0.1:0"
    ]
    return "\n".join(body)


def manage_issues(runner, gaps, security_fids, evidence_complete, sha, max_jules, main_red):
    runner.create_label_if_missing("ledger-gap", "Ledger feature gap", "ff0000")

    issues_out = runner.gh("issue", "list", "--repo", runner.repo, "--label", "ledger-gap",
                           "--state", "all", "--limit", "500", "--json", "number,state,body,labels", check=True)
    existing_issues = json.loads(issues_out) if issues_out else []

    jules_applied = 0
    handled_fids = set()

    for issue in existing_issues:
        body = issue.get("body", "")
        marker = "<!-- ledger-gap:"
        start = body.find(marker)
        if start == -1:
            continue
        start += len(marker)
        end = body.find(" -->", start)
        if end == -1:
            continue
        fid = body[start:end]

        labels = [l["name"] for l in issue.get("labels", [])]

        # Absolute skip for security features (label or feature map based)
        if "security" in labels or "security-report" in labels or fid in security_fids:
            continue

        is_open = issue["state"] == "OPEN"
        has_jules = "jules" in labels

        if fid in gaps:
            handled_fids.add(fid)
            gap = gaps[fid]
            new_body = format_issue_body(fid, gap)

            if is_open:
                if body != new_body:
                    runner.gh("issue", "edit", str(issue["number"]), "--body", new_body, "--repo", runner.repo)

                # Apply jules label if missing and under cap
                if not has_jules and jules_applied < max_jules:
                    runner.gh("issue", "edit", str(issue["number"]), "--add-label", "jules", "--repo", runner.repo)
                    jules_applied += 1
            else:
                # Closed but now a gap again
                runner.gh("issue", "reopen", str(issue["number"]), "--repo", runner.repo)
                runner.gh("issue", "edit", str(issue["number"]), "--body", new_body, "--repo", runner.repo)
                if jules_applied < max_jules:
                    runner.gh("issue", "edit", str(issue["number"]), "--add-label", "jules", "--repo", runner.repo)
                    jules_applied += 1
                elif has_jules:
                    runner.gh("issue", "edit", str(issue["number"]), "--remove-label", "jules", "--repo", runner.repo)
        else:
            if is_open:
                runner.gh("issue", "close", str(issue["number"]), "--comment",
                          f"Feature is now stable or no longer has gaps on SHA {sha}.", "--repo", runner.repo)

    if not evidence_complete or main_red:
        return

    # Create new issues for unhandled gaps
    for fid in sorted(gaps.keys()):
        if fid in handled_fids:
            continue

        if jules_applied >= max_jules:
            break

        gap = gaps[fid]
        title = f"ledger gap: {fid} — {gap['first_gap_reason']}"
        body = format_issue_body(fid, gap)

        runner.gh("issue", "create", "--repo", runner.repo, "--title", title,
                  "--body", body, "--label", "ledger-gap,jules")
        jules_applied += 1

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--report", type=Path, required=True, help="Path to ledger report JSON")
    parser.add_argument("--features", type=Path, default=Path(".gitcore/features.json"), help="Path to features JSON")
    parser.add_argument("--repo", default=os.environ.get("GITHUB_REPOSITORY"), help="GitHub repository (owner/repo)")
    parser.add_argument("--max-jules", type=int, default=5, help="Max jules labels to apply")
    parser.add_argument("--main-red", action="store_true", help="Flag if main is red (prevents new issue creation)")
    parser.add_argument("--dry-run", action="store_true", help="Do not perform modifications")

    args = parser.parse_args()

    if not args.repo:
        parser.error("--repo or GITHUB_REPOSITORY is required")

    runner = GithubRunner(args.repo, args.dry_run)

    gaps, sec_needs_owner, evidence_complete, sha = find_gaps(args.features, args.report)

    manage_issues(runner, gaps, sec_needs_owner, evidence_complete, sha, args.max_jules, args.main_red)

    if sec_needs_owner:
        print("## Security features needing owner")
        for fid in sorted(sec_needs_owner):
            print(f"- {fid}")


if __name__ == "__main__":
    main()
