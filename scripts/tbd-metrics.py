#!/usr/bin/env python3
import argparse
import json
import subprocess
import re
import sys
from datetime import datetime, timezone, timedelta

def run_gh_cmd(cmd):
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, check=True)
        # return empty string if no output to avoid JSON parse error, or parse JSON
        if result.stdout.strip() == "":
            return None
        return json.loads(result.stdout)
    except subprocess.CalledProcessError as e:
        stderr_first_line = e.stderr.strip().split('\n')[0] if e.stderr else "Unknown error"
        print(f"ERROR: {' '.join(cmd)} failed: {stderr_first_line}", file=sys.stderr)
        sys.exit(2)

def parse_time(ts):
    if not ts:
        return None
    return datetime.fromisoformat(ts.replace("Z", "+00:00"))

def format_metric(val):
    if val is None:
        return "n/a"
    return f"{val:.1f}"

def percentile(data, p):
    if not data:
        return None
    s = sorted(data)
    idx = int((len(s) - 1) * (p / 100.0))
    if idx < 0:
        return None
    return s[idx]

def get_week_cohort(ts, now):
    if not ts:
        return -1
    delta = now - ts
    return int(delta.days // 7)

def extract_incident_data(issue_body):
    offending_sha_match = re.search(r'Offending SHA:\s*([a-f0-9]+)', issue_body, re.IGNORECASE)
    run_url_match = re.search(r'Run URL:\s*(\S+)', issue_body, re.IGNORECASE)

    # Also support P11 marker: <!-- tbd-incident sha=<FULL_SHA> workflow=<NAME> -->
    p11_marker_match = re.search(r'<!--\s*tbd-incident\s+sha=([a-f0-9]+)\s+workflow=\S+\s*-->', issue_body, re.IGNORECASE)

    sha = offending_sha_match.group(1) if offending_sha_match else None
    if not sha and p11_marker_match:
        sha = p11_marker_match.group(1)

    run_url = run_url_match.group(1) if run_url_match else None
    return sha, run_url

def fetch_first_commit_date(repo_owner, repo_name, pr_number):
    cmd = ["gh", "api", f"repos/{repo_owner}/{repo_name}/pulls/{pr_number}/commits?per_page=1", "--jq", ".[0].commit.author.date"]
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, check=True)
        date_str = result.stdout.strip()
        if date_str:
            return parse_time(date_str)
    except Exception:
        pass
    return None

def analyze_cohorts(prs, issues, runs_data, weeks, now, use_first_commit, repo_owner, repo_name):
    cohorts = {i: {'prs': [], 'incidents': [], 'lead_times': [], 'mttrs': [], 'fast_gates': [], 'failed_shas': set()} for i in range(weeks)}

    # Map run data for fast lookup by URL or SHA
    run_by_url = {}
    fast_gate_runs_by_sha = {}

    if runs_data:
        for run in runs_data:
            url = run.get('url') or run.get('html_url')
            if url:
                run_by_url[url] = run

            sha = run.get('headSha') or run.get('head_sha')
            name = run.get('name', '')

            if sha and name == 'Fast Gate (Shadow)':
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
                cohorts[week]['failed_shas'].add(sha.lower())

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

            # Lead Time: defaults to PR createdAt, optionally use first commit
            created_at = parse_time(pr.get('createdAt'))
            start_time = created_at

            if use_first_commit and repo_owner and repo_name and 'number' in pr:
                first_commit_time = fetch_first_commit_date(repo_owner, repo_name, pr['number'])
                if first_commit_time:
                    start_time = first_commit_time

            if start_time:
                lead_time = (merged_at - start_time).total_seconds() / 3600.0
                cohorts[week]['lead_times'].append(max(0, lead_time))

            # Fast Gate Latency
            merge_commit_sha = pr.get('mergeCommit', {}).get('oid')
            head_sha = pr.get('headRefOid')
            shas_to_check = [head_sha, merge_commit_sha]

            for sha in shas_to_check:
                if not sha:
                    continue
                # Exact match or prefix match for runs
                for run_sha, runs in fast_gate_runs_by_sha.items():
                    if len(run_sha) >= 7 and sha.startswith(run_sha):
                        for run in runs:
                            run_created = parse_time(run.get('createdAt') or run.get('created_at'))
                            run_completed = parse_time(run.get('updatedAt') or run.get('updated_at'))
                            if run_created and run_completed:
                                latency = (run_completed - run_created).total_seconds() / 60.0
                                cohorts[week]['fast_gates'].append(max(0, latency))
                        break # Use the first matching SHA's runs
                else:
                    continue
                break # If inner loop broke, break outer

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

            # Match if it's an exact match or if f_sha is a 7+ char prefix
            is_failed = False
            if merge_commit:
                merge_commit = merge_commit.lower()
                for f_sha in c['failed_shas']:
                    if len(f_sha) >= 7 and merge_commit.startswith(f_sha):
                        is_failed = True
                        break

            if is_failed:
                failed_prs += 1

        cfr_str = f"{(failed_prs / n_prs * 100):.1f}%" if n_prs > 0 else "n/a"

        lt_p50 = percentile(c['lead_times'], 50)
        lt_p90 = percentile(c['lead_times'], 90)

        mttr_p50 = percentile(c['mttrs'], 50)
        mttr_p90 = percentile(c['mttrs'], 90)

        fg_p50 = percentile(c['fast_gates'], 50)
        fg_p90 = percentile(c['fast_gates'], 90)

        print(f"{w:<6} | {n_prs:<4} | {n_inc:<9} | {cfr_str:<6} | {format_metric(lt_p50):<9} | {format_metric(lt_p90):<9} | {format_metric(mttr_p50):<11} | {format_metric(mttr_p90):<11} | {format_metric(fg_p50):<9} | {format_metric(fg_p90):<9}")

def get_repo_info():
    try:
        result = subprocess.run(["gh", "repo", "view", "--json", "owner,name"], capture_output=True, text=True, check=True)
        data = json.loads(result.stdout)
        return data.get('owner', {}).get('login'), data.get('name')
    except Exception:
        return None, None

def main():
    parser = argparse.ArgumentParser(description="Report weekly GitHub flow metrics. Lead time defaults to PR createdAt to mergedAt.")
    parser.add_argument("--weeks", type=int, default=4, help="Number of weeks to report")
    parser.add_argument("--first-commit", action="store_true", help="Fetch first commit date for lead time instead of using PR created at")
    parser.add_argument("--mock-prs", type=str, help="Mock PR JSON file")
    parser.add_argument("--mock-issues", type=str, help="Mock Issues JSON file")
    parser.add_argument("--mock-runs", type=str, help="Mock Workflow Runs JSON file")
    parser.add_argument("--now", type=str, help="Mock now timestamp (ISO)")
    args = parser.parse_args()

    if args.now:
        now = parse_time(args.now)
    else:
        now = datetime.now(timezone.utc)

    date_filter = (now - timedelta(days=args.weeks * 7)).strftime("%Y-%m-%d")

    repo_owner, repo_name = None, None
    if args.first_commit and not args.mock_prs:
        repo_owner, repo_name = get_repo_info()

    if args.mock_prs:
        with open(args.mock_prs) as f:
            prs = json.load(f)
    else:
        prs = run_gh_cmd(["gh", "pr", "list", "--state", "merged", "--search", f"merged:>={date_filter}", "--limit", "1000", "--json", "number,mergedAt,createdAt,labels,mergeCommit,headRefOid"])
        if prs and len(prs) >= 1000:
            print("WARNING: TRUNCATED PR result limit reached.", file=sys.stderr)
        elif prs is None:
            prs = []

    if args.mock_issues:
        with open(args.mock_issues) as f:
            issues = json.load(f)
    else:
        issues = run_gh_cmd(["gh", "issue", "list", "--label", "tbd-incident", "--state", "all", "--search", f"created:>={date_filter}", "--limit", "1000", "--json", "number,createdAt,closedAt,state,body"])
        if issues and len(issues) >= 1000:
            print("WARNING: TRUNCATED issues result limit reached.", file=sys.stderr)
        elif issues is None:
            issues = []

    if args.mock_runs:
        with open(args.mock_runs) as f:
            runs_data = json.load(f)
    else:
        try:
            # Only querying runs after the date filter and specifically for the fast-gate workflow
            runs_data = run_gh_cmd(["gh", "run", "list", "--workflow", "fast-gate.yml", "--created", f">={date_filter}", "--limit", "1000", "--json", "databaseId,name,headSha,url,createdAt,updatedAt,conclusion"])
            if runs_data and len(runs_data) >= 1000:
                print("WARNING: TRUNCATED workflow runs result limit reached.", file=sys.stderr)
            elif runs_data is None:
                runs_data = []
        except Exception: # Normally run_gh_cmd will sys.exit on error, but we want to fail fast.
            runs_data = []
            pass

    cohorts = analyze_cohorts(prs, issues, runs_data, args.weeks, now, args.first_commit, repo_owner, repo_name)
    print_metrics(cohorts, args.weeks)

if __name__ == "__main__":
    main()
