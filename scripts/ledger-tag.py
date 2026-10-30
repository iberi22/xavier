#!/usr/bin/env python3
import argparse
import json
import math
import subprocess
import sys
import re

def git_fetch_tags():
    subprocess.check_call(['git', 'fetch', '--tags'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)

def git_list_tags():
    return subprocess.check_output(['git', 'tag', '-l'], text=True).splitlines()

def git_create_tag(tag, sha, message):
    subprocess.check_call(['git', 'tag', '-a', tag, sha, '-m', message])

def git_push_tag(tag):
    subprocess.check_call(['git', 'push', 'origin', tag])

def git_show_file_at_tag(tag, filepath):
    return subprocess.check_output(['git', 'show', f"{tag}:{filepath}"], text=True)

class Runner:
    def fetch_tags(self):
        git_fetch_tags()

    def list_tags(self):
        return git_list_tags()

    def create_tag(self, tag, sha, message):
        git_create_tag(tag, sha, message)

    def push_tag(self, tag):
        git_push_tag(tag)

    def show_file_at_tag(self, tag, filepath):
        return git_show_file_at_tag(tag, filepath)

def check_tag_name(tag):
    if tag.startswith('v'):
        sys.exit(2)
    if not re.match(r'^ledger-(stable|milestone)-[0-9]+$', tag):
        sys.exit(2)

def run(args, runner=None):
    if runner is None:
        runner = Runner()

    with open(args.features, 'r') as f:
        data = json.load(f)

    stable_count = 0
    total_count = 0
    current_stable_ids = set()

    if 'features' in data:
        for fid, f in data['features'].items():
            total_count += 1
            if f.get('status') == 'stable':
                stable_count += 1
                current_stable_ids.add(fid)

    if total_count == 0:
        return

    pct = (stable_count / total_count) * 100

    runner.fetch_tags()
    existing_tags = runner.list_tags()

    highest_stable = -1
    highest_stable_tag = None
    highest_milestone = -1

    for tag in existing_tags:
        m = re.match(r'^ledger-stable-([0-9]+)$', tag)
        if m:
            val = int(m.group(1))
            if val > highest_stable:
                highest_stable = val
                highest_stable_tag = tag
        m = re.match(r'^ledger-milestone-([0-9]+)$', tag)
        if m:
            val = int(m.group(1))
            if val > highest_milestone:
                highest_milestone = val

    tags_to_create = []

    if stable_count > highest_stable:
        new_tag = f"ledger-stable-{stable_count}"
        check_tag_name(new_tag)

        newly_stable = list(current_stable_ids)
        if highest_stable_tag:
            try:
                prev_data = json.loads(runner.show_file_at_tag(highest_stable_tag, args.features))
                prev_stable_ids = set()
                if 'features' in prev_data:
                    for fid, f in prev_data['features'].items():
                        if f.get('status') == 'stable':
                            prev_stable_ids.add(fid)
                newly_stable = list(current_stable_ids - prev_stable_ids)
            except Exception:
                pass

        newly_stable.sort()
        ids_str = ", ".join(newly_stable)
        msg = f"{stable_count}/{total_count} stable ({pct:.1f}%)\nNewly stable: {ids_str}"
        tags_to_create.append((new_tag, msg))

    milestone = math.floor(pct / 10) * 10
    if milestone > highest_milestone and pct >= 10:
        new_tag = f"ledger-milestone-{milestone}"
        check_tag_name(new_tag)
        tags_to_create.append((new_tag, f"Crossed {milestone}% stable"))


    if not args.dry_run:
        for tag, msg in tags_to_create:
            runner.create_tag(tag, args.sha, msg)
            runner.push_tag(tag)

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--features', required=True)
    parser.add_argument('--sha', required=True)
    parser.add_argument('--dry-run', action='store_true')
    args = parser.parse_args()
    run(args)
