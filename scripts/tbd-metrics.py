#!/usr/bin/env python3
import argparse
import json
import subprocess
import re
from datetime import datetime, timezone

def run_gh_cmd(cmd):
    result = subprocess.run(cmd, capture_output=True, text=True, check=True)
    return json.loads(result.stdout)

def parse_time(ts):
    if not ts:
        return None
    return datetime.fromisoformat(ts.replace("Z", "+00:00"))

def percentile(data, p):
    if not data:
        return 0.0
    s = sorted(data)
    idx = int((len(s) - 1) * (p / 100.0))
    if idx < 0:
        return 0.0
    return s[idx]

def get_week_cohort(ts, now):
    if not ts:
        return -1
    delta = now - ts
    return int(delta.days // 7)

def extract_incident_data(issue_body):
    offending_sha_match = re.search(r'Offending SHA:\s*([a-f0-9]+)', issue_body, re.IGNORECASE)
    run_url_match = re.search(r'Run URL:\s*(\S+)', issue_body, re.IGNORECASE)

    sha = offending_sha_match.group(1) if offending_sha_match else None
    run_url = run_url_match.group(1) if run_url_match else None
    return sha, run_url

def extract_pr_first_commit(pr_commits):
    if not pr_commits:
        return None
    # Assuming pr_commits is ordered, first commit is at index 0.
    if isinstance(pr_commits, list) and len(pr_commits) > 0:
        commit_date = pr_commits[0].get('commit', {}).get('author', {}).get('date')
        if not commit_date:
            commit_date = pr_commits[0].get('authoredDate') # gh pr view json output
        return parse_time(commit_date)
    return None

def analyze_cohorts(prs, issues, runs_data, weeks, now):
    cohorts = {i: {'prs': [], 'incidents': [], 'lead_times': [], 'mttrs': [], 'fast_gates': [], 'failed_shas': set()} for i in range(weeks)}

    # Map run data for fast lookup by URL or SHA
    run_by_url = {}
    fast_gate_runs_by_sha = {}

    if runs_data:
        for run in runs_data:
            url = run.get('url') or run.get('html_url')
            if url:
                run_by_url[url] = run

            sha = run.get('head_sha')
            name = run.get('name', '')
            if sha and 'fast-gate' in name.lower():
                if sha not in fast_gate_runs_by_sha:
                    fast_gate_runs_by_sha[sha] = []
                fast_gate_runs_by_sha[sha].append(run)

    # Process issues to find incidents and failed SHAs
    for issue in issues:
        created_at = parse_time(issue.get('createdAt'))
        closed_at = parse_time(issue.get('closedAt'))
        if not created_at:
            continue
        week = get_week_cohort(created_at, now)

        body = issue.get('body', '')
        sha, run_url = extract_incident_data(body)

        if 0 <= week < weeks:
            cohorts[week]['incidents'].append(issue)
            if sha:
                cohorts[week]['failed_shas'].add(sha)

            if closed_at:
                failed_run_completed_at = None
                if run_url and run_url in run_by_url:
                    completed_at_str = run_by_url[run_url].get('updatedAt') or run_by_url[run_url].get('updated_at')
                    failed_run_completed_at = parse_time(completed_at_str)

                # Fallback to issue creation time if we can't find the run
                start_time = failed_run_completed_at if failed_run_completed_at else created_at

                mttr = (closed_at - start_time).total_seconds() / 3600.0
                cohorts[week]['mttrs'].append(max(0, mttr))

    # Process PRs for lead time, CFR, and fast gate
    for pr in prs:
        merged_at = parse_time(pr.get('mergedAt'))
        if not merged_at:
            continue
        week = get_week_cohort(merged_at, now)
        if 0 <= week < weeks:
            cohorts[week]['prs'].append(pr)

            # Lead Time: first commit to merge
            first_commit_time = extract_pr_first_commit(pr.get('commits'))
            # Fallback to PR created at if commits aren't available
            created_at = parse_time(pr.get('createdAt'))
            start_time = first_commit_time if first_commit_time else created_at

            if start_time:
                lead_time = (merged_at - start_time).total_seconds() / 3600.0
                cohorts[week]['lead_times'].append(max(0, lead_time))

            # Fast Gate Latency
            merge_commit_sha = pr.get('mergeCommit', {}).get('oid')
            head_sha = pr.get('headRefOid')
            shas_to_check = [head_sha, merge_commit_sha]

            for sha in shas_to_check:
                if sha and sha in fast_gate_runs_by_sha:
                    for run in fast_gate_runs_by_sha[sha]:
                        run_created = parse_time(run.get('createdAt') or run.get('created_at'))
                        run_completed = parse_time(run.get('updatedAt') or run.get('updated_at'))
                        if run_created and run_completed:
                            latency = (run_completed - run_created).total_seconds() / 60.0
                            cohorts[week]['fast_gates'].append(max(0, latency))
                    break # Use the first matching SHA's runs

    return cohorts

def print_metrics(cohorts, weeks):
    print(f"{'Week':<6} | {'PRs':<4} | {'Incidents':<9} | {'CFR':<6} | {'LT p50(h)':<9} | {'LT p90(h)':<9} | {'MTTR p50(h)':<11} | {'MTTR p90(h)':<11} | {'FG p50(m)':<9} | {'FG p90(m)':<9}")
    print("-" * 105)
    for w in range(weeks - 1, -1, -1):
        c = cohorts[w]
        n_prs = len(c['prs'])
        n_inc = len(c['incidents'])

        # Change Failure Rate (CFR): PRs linked to an incident / merged PRs
        failed_prs = 0
        for pr in c['prs']:
            merge_commit = pr.get('mergeCommit', {}).get('oid')
            if merge_commit and merge_commit in c['failed_shas']:
                failed_prs += 1

        cfr = (failed_prs / n_prs * 100) if n_prs > 0 else 0.0

        lt_p50 = percentile(c['lead_times'], 50)
        lt_p90 = percentile(c['lead_times'], 90)

        mttr_p50 = percentile(c['mttrs'], 50)
        mttr_p90 = percentile(c['mttrs'], 90)

        fg_p50 = percentile(c['fast_gates'], 50)
        fg_p90 = percentile(c['fast_gates'], 90)

        print(f"{w:<6} | {n_prs:<4} | {n_inc:<9} | {cfr:<5.1f}% | {lt_p50:<9.1f} | {lt_p90:<9.1f} | {mttr_p50:<11.1f} | {mttr_p90:<11.1f} | {fg_p50:<9.1f} | {fg_p90:<9.1f}")

def main():
    parser = argparse.ArgumentParser(description="Report weekly GitHub flow metrics")
    parser.add_argument("--weeks", type=int, default=4, help="Number of weeks to report")
    parser.add_argument("--mock-prs", type=str, help="Mock PR JSON file")
    parser.add_argument("--mock-issues", type=str, help="Mock Issues JSON file")
    parser.add_argument("--mock-runs", type=str, help="Mock Workflow Runs JSON file")
    parser.add_argument("--now", type=str, help="Mock now timestamp (ISO)")
    args = parser.parse_args()

    if args.mock_prs:
        with open(args.mock_prs) as f:
            prs = json.load(f)
    else:
        prs = run_gh_cmd(["gh", "pr", "list", "--state", "merged", "--limit", "100", "--json", "number,mergedAt,createdAt,labels,body,commits,mergeCommit,headRefOid"])

    if args.mock_issues:
        with open(args.mock_issues) as f:
            issues = json.load(f)
    else:
        issues = run_gh_cmd(["gh", "issue", "list", "--label", "tbd-incident", "--state", "all", "--limit", "100", "--json", "number,createdAt,closedAt,state,body"])

    if args.mock_runs:
        with open(args.mock_runs) as f:
            runs_data = json.load(f)
    else:
        # Note: In a real implementation this might need pagination or specific filtering
        try:
            runs_data = run_gh_cmd(["gh", "run", "list", "--limit", "500", "--json", "databaseId,name,headSha,url,createdAt,updatedAt,conclusion"])
        except Exception:
            runs_data = []

    if args.now:
        now = parse_time(args.now)
    else:
        now = datetime.now(timezone.utc)

    cohorts = analyze_cohorts(prs, issues, runs_data, args.weeks, now)
    print_metrics(cohorts, args.weeks)

if __name__ == "__main__":
    main()
