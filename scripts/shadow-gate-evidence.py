#!/usr/bin/env python3
import argparse
import json
import subprocess
import sys
import os
import re
from datetime import datetime, timezone, timedelta

def default_gh_runner(cmd, as_text=False):
    """Executes a gh command and parses JSON output if applicable."""
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, check=True)
        if result.stdout.strip() == "":
            return None
        if ("--json" in cmd or "--jq" in cmd) and not as_text:
            try:
                return json.loads(result.stdout)
            except json.JSONDecodeError:
                return result.stdout.strip()
        return result.stdout.strip()
    except subprocess.CalledProcessError as e:
        stderr_first_line = e.stderr.strip().split('\n')[0] if e.stderr else "Unknown error"
        print(f"ERROR: {' '.join(cmd)} failed: {stderr_first_line}", file=sys.stderr)
        sys.exit(2)

def get_args():
    parser = argparse.ArgumentParser(description="Generate shadow fast-gate evidence packet.")
    parser.add_argument("--since", required=True, help="Start date (YYYY-MM-DD)")
    parser.add_argument("--until", help="End date (YYYY-MM-DD)", default=None)
    parser.add_argument("--out", help="Output file path", default=None)
    parser.add_argument("--json", action="store_true", help="Output JSON instead of Markdown")
    return parser.parse_args()

def parse_time(ts):
    if not ts:
        return None
    return datetime.fromisoformat(ts.replace("Z", "+00:00"))

def percentile(data, p):
    if not data:
        return None
    s = sorted(data)
    idx = int((len(s) - 1) * (p / 100.0))
    if idx < 0:
        return None
    return s[idx]

def fetch_data(args, gh_runner):
    since_date = args.since
    until_date = args.until

    search_query = f"merged:>={since_date}"
    if until_date:
        search_query += f" merged:<={until_date}"

    prs = gh_runner(["gh", "pr", "list", "--state", "merged", "--search", search_query, "--limit", "1000", "--json", "number,headRefOid"])
    if not prs:
        prs = []

    shadow_runs = gh_runner(["gh", "run", "list", "--workflow", "fast-gate.yml", "--created", f">={since_date}", "--limit", "1000", "--json", "databaseId,headSha,url,createdAt,updatedAt,startedAt"])
    if not shadow_runs:
        shadow_runs = []

    ci_runs = gh_runner(["gh", "run", "list", "--workflow", "ci.yml", "--created", f">={since_date}", "--limit", "1000", "--json", "databaseId,headSha,conclusion"])
    if not ci_runs:
        ci_runs = []

    return prs, shadow_runs, ci_runs

def get_run_jobs(run_id, gh_runner):
    try:
        if not run_id:
            return []

        jobs_data = gh_runner(["gh", "run", "view", str(run_id), "--json", "jobs"])
        if isinstance(jobs_data, dict):
            return jobs_data.get("jobs", [])
        return []
    except Exception:
        return []

def extract_notice_payload(prefixes, log_text):
    if not log_text:
        return None
    if isinstance(prefixes, str):
        prefixes = [prefixes]
    pos = 0
    decoder = json.JSONDecoder()
    for prefix in prefixes:
        pos = 0
        while True:
            idx = log_text.find(prefix, pos)
            if idx == -1:
                break
            start = idx + len(prefix)
            brace = log_text.find('{', start)
            if brace != -1 and log_text[start:brace].strip() == '':
                try:
                    obj, _ = decoder.raw_decode(log_text, brace)
                    return obj
                except Exception:
                    pass
            pos = start
    return None

def extract_log_notices(run_id, gh_runner):
    log_text = gh_runner(["gh", "run", "view", str(run_id), "--log"], as_text=True)
    if not log_text:
        return None, None

    classify = extract_notice_payload(['::notice::classify=', '##[notice]classify='], log_text)
    rust_impact = extract_notice_payload(['::notice::rust-impact=', '##[notice]rust-impact='], log_text)

    if classify is None:
        match_c = re.search(r'(?:::notice::|##\[notice\])classify=({.*?})', log_text)
        if match_c:
            try:
                classify = json.loads(match_c.group(1))
            except Exception:
                pass

    if rust_impact is None:
        match_r = re.search(r'(?:::notice::|##\[notice\])rust-impact=({.*?})', log_text)
        if match_r:
            try:
                rust_impact = json.loads(match_r.group(1))
            except Exception:
                pass

    return classify, rust_impact

def main(args, gh_runner=default_gh_runner):
    prs, shadow_runs, ci_runs = fetch_data(args, gh_runner)

    # Group runs by headSha
    shadow_by_sha = {}
    for run in shadow_runs:
        sha = run.get("headSha")
        if sha:
            shadow_by_sha.setdefault(sha, []).append(run)

    ci_by_sha = {}
    for run in ci_runs:
        sha = run.get("headSha")
        if sha:
            ci_by_sha.setdefault(sha, []).append(run)

    prs_seen = len(prs)
    unclassified_count = 0
    skip_rust_count = 0
    missed_failures = []

    durations_with_queue = []
    durations_without_queue = []
    cold_cache_cohort = []

    for pr in prs:
        sha = pr.get("headRefOid")
        if not sha:
            continue

        s_runs = shadow_by_sha.get(sha, [])
        c_runs = ci_by_sha.get(sha, [])

        if not s_runs:
            # PR has no shadow run?
            unclassified_count += 1
            continue

        # Sort shadow runs by createdAt descending (latest first)
        s_runs.sort(key=lambda x: parse_time(x.get("createdAt", "1970-01-01T00:00:00Z")) or datetime.min.replace(tzinfo=timezone.utc), reverse=True)
        latest_shadow = s_runs[0]

        # Sort ci runs
        c_runs.sort(key=lambda x: parse_time(x.get("createdAt", "1970-01-01T00:00:00Z")) or datetime.min.replace(tzinfo=timezone.utc), reverse=True)
        latest_ci = c_runs[0] if c_runs else None

        run_id = latest_shadow.get("databaseId")
        classify, rust_impact = extract_log_notices(run_id, gh_runner)

        if not classify or not rust_impact:
            unclassified_count += 1
            continue

        is_skip_rust = rust_impact.get("skip_rust", False)
        if is_skip_rust:
            skip_rust_count += 1

            # Check for missed failure
            if latest_ci and latest_ci.get("conclusion") == "failure":
                ci_run_id = latest_ci.get("databaseId")
                jobs = get_run_jobs(ci_run_id, gh_runner)

                # Check if it failed in a Rust job
                failed_rust_job = False
                for job in jobs:
                    if job.get("conclusion") == "failure":
                        name = job.get("name", "").lower()
                        # Simple heuristic for Rust job
                        if "rust" in name or "cargo" in name or "clippy" in name or "fmt" in name or "test" in name:
                            failed_rust_job = True
                            break

                if failed_rust_job:
                    missed_failures.append({
                        "pr": pr.get("number"),
                        "sha": sha,
                        "ci_run": latest_ci.get("databaseId")
                    })

        # Latency calculation
        # The prompt asks for "cold-cache cohort (first run of a branch)". We compute
        # fast-gate duration p50/p90 with and without queue time for the latest run,
        # and we look at the very first run for this PR to include in cold-cache cohort.

        # Latest run latency
        created_at = parse_time(latest_shadow.get("createdAt"))
        updated_at = parse_time(latest_shadow.get("updatedAt"))
        run_started_at = parse_time(latest_shadow.get("startedAt"))

        if created_at and updated_at:
            duration = (updated_at - created_at).total_seconds()
            durations_with_queue.append(duration)

        if run_started_at and updated_at:
            duration = (updated_at - run_started_at).total_seconds()
            durations_without_queue.append(duration)

        # Cold cache cohort (first run of the PR)
        first_shadow = s_runs[-1]
        first_started = parse_time(first_shadow.get("startedAt"))
        first_updated = parse_time(first_shadow.get("updatedAt"))

        if first_started and first_updated:
            duration_cold = (first_updated - first_started).total_seconds()
            cold_cache_cohort.append(duration_cold)

    p50_with = percentile(durations_with_queue, 50)
    p90_with = percentile(durations_with_queue, 90)

    p50_without = percentile(durations_without_queue, 50)
    p90_without = percentile(durations_without_queue, 90)

    p50_cold = percentile(cold_cache_cohort, 50)
    p90_cold = percentile(cold_cache_cohort, 90)

    # Determine PANEL_INPUT
    now = datetime.now(timezone.utc)
    since = parse_time(args.since + "T00:00:00Z")

    window_days = (now - since).days

    is_insufficient = False
    if window_days < 14 or unclassified_count > 0:
        is_insufficient = True

    panel_input = "insufficient" if is_insufficient else "ready"

    result_dict = {
        "window": f"{args.since} to {args.until or 'now'}",
        "prs_seen": prs_seen,
        "unclassified_count": unclassified_count,
        "skip_rust_decisions": skip_rust_count,
        "missed_failures": missed_failures,
        "latency": {
            "with_queue_p50": p50_with,
            "with_queue_p90": p90_with,
            "without_queue_p50": p50_without,
            "without_queue_p90": p90_without,
            "cold_cache_p50": p50_cold,
            "cold_cache_p90": p90_cold
        },
        "panel_input": panel_input
    }

    if args.json:
        output_str = json.dumps(result_dict, indent=2)
    else:
        output_str = f"""# Shadow Fast-Gate Evidence Packet

## Window
{result_dict['window']}

## Coverage
- PRs seen: {prs_seen}
- Unclassified count: {unclassified_count}
- Skip Rust decisions: {skip_rust_count}

## Missed failures
"""
        if missed_failures:
            output_str += "| PR | SHA | CI Run |\n|---|---|---|\n"
            for f in missed_failures:
                output_str += f"| {f['pr']} | {f['sha']} | {f['ci_run']} |\n"
        else:
            output_str += "None detected.\n"

        output_str += f"""
## Latency (seconds)
- With Queue: p50={p50_with or 'n/a'} p90={p90_with or 'n/a'}
- Without Queue: p50={p50_without or 'n/a'} p90={p90_without or 'n/a'}
- Cold Cache Cohort: p50={p50_cold or 'n/a'} p90={p90_cold or 'n/a'}

## Verdict inputs
- Window >= 14 days: {'Yes' if window_days >= 14 else 'No'}
- Zero unclassified PRs: {'Yes' if unclassified_count == 0 else 'No'}

PANEL_INPUT: {panel_input}
"""

    if args.out:
        with open(args.out, 'w') as f:
            f.write(output_str)
    else:
        print(output_str)

if __name__ == "__main__":
    main(get_args())
