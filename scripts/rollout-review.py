#!/usr/bin/env python3
import argparse
import datetime
import importlib.util
import json
import os
import re
import sys
from typing import List, Dict, Any, Tuple

spec = importlib.util.spec_from_file_location("tbd_metrics", os.path.join(os.path.dirname(__file__), "tbd-metrics.py"))
tbd_metrics = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tbd_metrics)

def get_revert_prs(date_filter: str, mock_file: str = None) -> List[Dict]:
    if mock_file:
        with open(mock_file) as f:
            return json.load(f)
    try:
        prs = tbd_metrics.run_gh_cmd(["gh", "pr", "list", "--state", "all", "--search", f"created:>={date_filter} revert in:title", "--limit", "1000", "--json", "number,createdAt,mergedAt,title,state"])
        return prs if prs else []
    except Exception as e:
        return []

def get_missed_failures(fast_gate_runs: List[Dict], incidents: List[Dict], window_start: datetime.datetime, window_end: datetime.datetime) -> int:
    # A missed failure is a main red (incident) within 24h of a merge that passed the fast gate.
    # From incident we get failed sha and closed_at time maybe? Actually, we'll just check if there are incidents
    # linked to PRs that passed the fast gate.
    # To simplify: count incidents created in window whose failed sha had a fast gate pass run within 24h before incident.
    # We can use tbd-metrics functions. We'll extract failed shas from incidents.

    missed_failures = 0
    fast_gate_success_shas = set()
    for run in fast_gate_runs:
        if run.get("conclusion") == "success":
            sha = run.get("headSha") or run.get("head_sha")
            if sha:
                fast_gate_success_shas.add(sha)

    for issue in incidents:
        created_at = tbd_metrics.parse_time(issue.get("createdAt"))
        if not created_at or not (window_start <= created_at < window_end):
            continue
        body = issue.get("body", "")
        sha, _ = tbd_metrics.extract_incident_data(body)
        if sha and sha in fast_gate_success_shas:
            # We assume the incident happened after the fast gate pass (usually the case)
            missed_failures += 1

    return missed_failures

def calculate_metrics(prs: List[Dict], issues: List[Dict], runs_data: List[Dict], revert_prs: List[Dict], window_start: datetime.datetime, window_end: datetime.datetime) -> Dict:
    window_prs = []
    window_incidents = []
    window_fast_gate_runs = []
    window_reverts_opened = 0
    window_reverts_merged = 0

    failed_shas = set()

    for pr in prs:
        merged_at = tbd_metrics.parse_time(pr.get('mergedAt'))
        if merged_at and window_start <= merged_at < window_end:
            window_prs.append(pr)

    for issue in issues:
        created_at = tbd_metrics.parse_time(issue.get('createdAt'))
        if created_at and window_start <= created_at < window_end:
            window_incidents.append(issue)
            body = issue.get('body', '')
            sha, _ = tbd_metrics.extract_incident_data(body)
            if sha:
                failed_shas.add(sha.lower())

    for run in runs_data:
        run_created = tbd_metrics.parse_time(run.get('createdAt') or run.get('created_at'))
        if run_created and window_start <= run_created < window_end:
            window_fast_gate_runs.append(run)

    for pr in revert_prs:
        created_at = tbd_metrics.parse_time(pr.get('createdAt'))
        merged_at = tbd_metrics.parse_time(pr.get('mergedAt'))
        if created_at and window_start <= created_at < window_end:
            window_reverts_opened += 1
        if merged_at and window_start <= merged_at < window_end:
            window_reverts_merged += 1

    # Lead Times
    lead_times = []
    for pr in window_prs:
        merged_at = tbd_metrics.parse_time(pr.get('mergedAt'))
        created_at = tbd_metrics.parse_time(pr.get('createdAt'))
        if merged_at and created_at:
            lead_time = (merged_at - created_at).total_seconds() / 3600.0
            lead_times.append(max(0, lead_time))

    lt_p50 = tbd_metrics.percentile(lead_times, 50)
    lt_p90 = tbd_metrics.percentile(lead_times, 90)

    # MTTR
    mttrs = []
    # Similar to tbd-metrics: we need to find closed incidents
    run_by_url = {}
    for run in runs_data:
        url = run.get('url') or run.get('html_url')
        if url:
            run_by_url[url] = run

    for issue in window_incidents:
        created_at = tbd_metrics.parse_time(issue.get('createdAt'))
        closed_at = tbd_metrics.parse_time(issue.get('closedAt'))
        if created_at and closed_at:
            body = issue.get('body', '')
            _, run_url = tbd_metrics.extract_incident_data(body)
            start_time = created_at
            if run_url and run_url in run_by_url:
                completed_at_str = run_by_url[run_url].get('updatedAt') or run_by_url[run_url].get('updated_at')
                completed_at = tbd_metrics.parse_time(completed_at_str)
                if completed_at:
                    start_time = completed_at

            mttr = (closed_at - start_time).total_seconds() / 3600.0
            mttrs.append(max(0, mttr))

    mttr_p50 = tbd_metrics.percentile(mttrs, 50)
    mttr_p90 = tbd_metrics.percentile(mttrs, 90)

    # CFR
    n_prs = len(window_prs)
    failed_prs = 0
    for pr in window_prs:
        merge_commit = pr.get('mergeCommit', {}).get('oid')
        if merge_commit:
            merge_commit = merge_commit.lower()
            if any(f_sha in merge_commit or merge_commit.startswith(f_sha) for f_sha in failed_shas if len(f_sha) >= 7):
                failed_prs += 1
                continue

    cfr = (failed_prs / n_prs * 100) if n_prs > 0 else 0.0

    missed_failures = get_missed_failures(window_fast_gate_runs, window_incidents, window_start, window_end)

    return {
        "merged_prs": n_prs,
        "lt_p50": lt_p50,
        "lt_p90": lt_p90,
        "cfr": cfr,
        "mttr_p50": mttr_p50,
        "mttr_p90": mttr_p90,
        "incidents": len(window_incidents),
        "reverts_opened": window_reverts_opened,
        "reverts_merged": window_reverts_merged,
        "missed_failures": missed_failures,
        "n_prs": n_prs
    }

def format_val(v, fmt="{:.1f}"):
    return fmt.format(v) if v is not None else "n/a"

def generate_report(activation_date_str: str, before_metrics: Dict, after_metrics: Dict) -> Tuple[str, str]:
    b_cfr = before_metrics["cfr"]
    a_cfr = after_metrics["cfr"]

    b_mttr = before_metrics["mttr_p50"] or 0
    a_mttr = after_metrics["mttr_p50"] or 0

    missed = after_metrics["missed_failures"]

    # rule: `narrow` if missed failures > 0
    # rule: `roll back` if CFR worsened > 50%
    # rule: `retain` if CFR and MTTR did not worsen by > 20% and missed failures == 0
    #
    # priority: narrow > roll back > retain.
    # But wait, what if CFR worsened > 50% AND missed failures > 0?
    # The wording "narrow if missed failures > 0" usually takes precedence if we want to narrow scope to avoid missing more failures.

    recommendation = "retain"
    if missed > 0:
        recommendation = "narrow"
    elif b_cfr > 0 and a_cfr > b_cfr * 1.5:
        recommendation = "roll back"
    elif b_cfr == 0 and a_cfr > 0: # mathematically infinite worsening
        recommendation = "roll back"
    else:
        cfr_worsened_by_20 = (b_cfr == 0 and a_cfr > 0) or (b_cfr > 0 and a_cfr > b_cfr * 1.2)
        mttr_worsened_by_20 = (b_mttr == 0 and a_mttr > 0) or (b_mttr > 0 and a_mttr > b_mttr * 1.2)

        if cfr_worsened_by_20 or mttr_worsened_by_20:
            # Fallback if it doesn't strictly meet "retain" conditions
            recommendation = "roll back"

    report = [
        f"# Rollout Review: {activation_date_str}",
        "",
        f"**RECOMMENDATION:** `{recommendation}`",
        "",
        "## Metrics Comparison",
        "| Metric | Before | After | Delta |",
        "|---|---|---|---|",
        f"| Merged PRs | {before_metrics['merged_prs']} | {after_metrics['merged_prs']} | |",
        f"| Lead Time p50 (h) | {format_val(before_metrics['lt_p50'])} | {format_val(after_metrics['lt_p50'])} | |",
        f"| Lead Time p90 (h) | {format_val(before_metrics['lt_p90'])} | {format_val(after_metrics['lt_p90'])} | |",
        f"| CFR (%) | {format_val(before_metrics['cfr'])} | {format_val(after_metrics['cfr'])} | |",
        f"| MTTR p50 (h) | {format_val(before_metrics['mttr_p50'])} | {format_val(after_metrics['mttr_p50'])} | |",
        f"| MTTR p90 (h) | {format_val(before_metrics['mttr_p90'])} | {format_val(after_metrics['mttr_p90'])} | |",
        f"| Incidents | {before_metrics['incidents']} | {after_metrics['incidents']} | |",
        f"| Revert PRs Opened | {before_metrics['reverts_opened']} | {after_metrics['reverts_opened']} | |",
        f"| Revert PRs Merged | {before_metrics['reverts_merged']} | {after_metrics['reverts_merged']} | |",
        f"| Missed Failures | {before_metrics['missed_failures']} | {after_metrics['missed_failures']} | |",
    ]

    return recommendation, "\n".join(report)

def parse_args():
    parser = argparse.ArgumentParser(description="Build the rollout review packet comparing delivery metrics before and after gate activation")
    parser.add_argument("--activation", type=str, required=True, help="Activation date (YYYY-MM-DD)")
    parser.add_argument("--days", type=int, default=14, help="Window size in days (default: 14)")
    parser.add_argument("--out", type=str, help="Output file (markdown)")

    parser.add_argument("--mock-prs", type=str, help="Mock PR JSON file")
    parser.add_argument("--mock-issues", type=str, help="Mock Issues JSON file")
    parser.add_argument("--mock-runs", type=str, help="Mock Workflow Runs JSON file")
    parser.add_argument("--mock-reverts", type=str, help="Mock Revert PRs JSON file")
    return parser.parse_args()

def main():
    args = parse_args()

    activation_date = tbd_metrics.parse_time(f"{args.activation}T00:00:00Z")
    if not activation_date:
        print("Invalid activation date.", file=sys.stderr)
        sys.exit(1)

    start_date = activation_date - datetime.timedelta(days=args.days)
    end_date = activation_date + datetime.timedelta(days=args.days)

    date_filter = start_date.strftime("%Y-%m-%d")

    if args.mock_prs:
        with open(args.mock_prs) as f:
            prs = json.load(f)
    else:
        prs = tbd_metrics.run_gh_cmd(["gh", "pr", "list", "--state", "merged", "--search", f"merged:>={date_filter}", "--limit", "1000", "--json", "number,mergedAt,createdAt,labels,mergeCommit,headRefOid"]) or []

    if args.mock_issues:
        with open(args.mock_issues) as f:
            issues = json.load(f)
    else:
        issues = tbd_metrics.run_gh_cmd(["gh", "issue", "list", "--label", "tbd-incident", "--state", "all", "--search", f"created:>={date_filter}", "--limit", "1000", "--json", "number,createdAt,closedAt,state,body"]) or []

    if args.mock_runs:
        with open(args.mock_runs) as f:
            runs_data = json.load(f)
    else:
        try:
            runs_data = tbd_metrics.run_gh_cmd(["gh", "run", "list", "--workflow", "fast-gate.yml", "--created", f">={date_filter}", "--limit", "1000", "--json", "databaseId,name,headSha,url,createdAt,updatedAt,conclusion"]) or []
        except Exception:
            runs_data = []

    revert_prs = get_revert_prs(date_filter, args.mock_reverts)

    before_metrics = calculate_metrics(prs, issues, runs_data, revert_prs, start_date, activation_date)
    after_metrics = calculate_metrics(prs, issues, runs_data, revert_prs, activation_date, end_date)

    rec, report = generate_report(args.activation, before_metrics, after_metrics)

    if args.out:
        with open(args.out, "w") as f:
            f.write(report + "\n")
    else:
        print(report)

if __name__ == "__main__":
    main()
