#!/usr/bin/env python3
import argparse
import sys
import json
import subprocess
import re

def run_cmd(cmd, runner=subprocess.run):
    try:
        result = runner(cmd, capture_output=True, text=True, check=True)
        if result.stdout.strip() == "":
            return None
        if "gh" in cmd and "--json" in cmd:
            return json.loads(result.stdout)
        return result.stdout.strip()
    except subprocess.CalledProcessError as e:
        stderr_first_line = e.stderr.strip().split('\n')[0] if e.stderr else "Unknown error"
        print(f"ERROR: {' '.join(cmd)} failed: {stderr_first_line}", file=sys.stderr)
        sys.exit(2)

def get_tag_date(tag, runner=subprocess.run):
    cmd = ["git", "log", "-1", "--format=%cI", tag]
    return run_cmd(cmd, runner)

def get_prs(repo, tag_date, runner=subprocess.run):
    cmd = ["gh", "pr", "list", "--state", "merged", "--search", f"merged:>={tag_date}", "--repo", repo, "--json", "number,title,author", "--limit", "1000"]
    prs = run_cmd(cmd, runner)
    return prs if prs else []

def parse_conventional_commit(title):
    match = re.match(r"^([a-z]+)(?:\(([^)]+)\))?(?:!)?: (.*)$", title)
    if match:
        type_ = match.group(1)
        scope = match.group(2)
        subject = match.group(3)
        return type_, scope, subject
    return None, None, title

def map_type_to_section(type_):
    mapping = {
        "feat": "Added",
        "fix": "Fixed",
        "perf": "Changed",
        "refactor": "Changed",
        "docs": "Documentation",
        "test": "Maintenance",
        "ci": "Maintenance",
        "build": "Maintenance",
        "chore": "Maintenance",
    }
    return mapping.get(type_, "Other")

def format_prs(prs):
    deduped = {}
    for pr in prs:
        if pr["number"] not in deduped:
            deduped[pr["number"]] = pr

    sections = {
        "Added": [],
        "Changed": [],
        "Fixed": [],
        "Security": [],
        "Documentation": [],
        "Maintenance": [],
        "Other": []
    }

    dependabot_count = 0

    for pr in deduped.values():
        author = pr.get("author", {}).get("login", "")
        title = pr["title"]
        number = pr["number"]

        if "dependabot" in author.lower() or title.startswith("build(deps)"):
            dependabot_count += 1
            continue

        type_, scope, subject = parse_conventional_commit(title)
        section = map_type_to_section(type_)

        if scope:
            bullet = f"- **{scope}**: {subject} (#{number})"
        else:
            bullet = f"- {subject} (#{number})"

        if section not in sections:
            section = "Other"

        sections[section].append(bullet)

    if dependabot_count > 0:
        sections["Maintenance"].append(f"- Dependencies: {dependabot_count} updates")

    return sections

def generate_markdown(sections):
    lines = []

    order = ["Added", "Changed", "Fixed", "Security", "Documentation", "Maintenance", "Other"]
    for sec in order:
        if sections.get(sec):
            lines.append(f"### {sec}")
            lines.extend(sections[sec])

    return "\n".join(lines) + "\n"

def replace_unreleased_section(content, new_body):
    # Regex to match ## [Unreleased] and everything until the next ## [v... or EOF
    pattern = re.compile(r"(##\s+\[Unreleased\]\r?\n)(.*?)(?=\r?\n##\s+\[|\Z)", re.DOTALL)

    match = pattern.search(content)
    if not match:
        # If ## [Unreleased] not found, we shouldn't arbitrarily insert it
        return content

    prefix = match.group(1)

    # Ensure new body ends with one newline and add one empty line after prefix
    if new_body.strip():
        new_body = "\n" + new_body.strip() + "\n"
    else:
        new_body = ""

    replacement = prefix + new_body

    return pattern.sub(replacement, content, count=1)

def main():
    parser = argparse.ArgumentParser(description="Generate changelog section from merged PRs.")
    parser.add_argument("--since", required=True, help="Tag or commit to start from (e.g. v0.2.17)")
    parser.add_argument("--write", action="store_true", help="Write changes to CHANGELOG.md")
    parser.add_argument("--repo", default="iberi22/xavier", help="GitHub repository (owner/name)")

    try:
        args = parser.parse_args()
    except SystemExit as e:
        sys.exit(2 if e.code != 0 else 0)

    tag_date = get_tag_date(args.since)
    if not tag_date:
        print(f"ERROR: Could not get date for tag {args.since}", file=sys.stderr)
        sys.exit(2)

    prs = get_prs(args.repo, tag_date)
    sections = format_prs(prs)
    new_body = generate_markdown(sections)

    if args.write:
        try:
            with open("CHANGELOG.md", "r") as f:
                content = f.read()

            new_content = replace_unreleased_section(content, new_body)

            with open("CHANGELOG.md", "w") as f:
                f.write(new_content)
        except Exception as e:
            print(f"ERROR: Failed to update CHANGELOG.md: {e}", file=sys.stderr)
            sys.exit(2)
    else:
        print(new_body, end="")

if __name__ == "__main__":
    main()
