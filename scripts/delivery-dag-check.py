import argparse
import json
import sys
import os
from pathlib import Path

def validate(dag: dict, repo_root: Path) -> list[str]:
    reasons = []

    if not isinstance(dag, list):
        return ["DAG root must be a list of tasks"]

    task_ids = set()
    for task in dag:
        if not isinstance(task, dict):
            reasons.append("Task is not a dictionary")
            continue

        task_id = task.get("id")
        if task_id is None:
            reasons.append("Missing task id")
            continue

        if task_id in task_ids:
            reasons.append(f"{task_id}: Duplicate task id")

        task_ids.add(task_id)

    # Secondary loop to validate properties now that task_ids is known
    for task in dag:
        if not isinstance(task, dict):
            continue

        task_id = task.get("id")
        if task_id is None:
            continue

        if "files" not in task:
            reasons.append(f"{task_id}: Missing files key")

        if "depends_on" not in task:
            reasons.append(f"{task_id}: Missing depends_on key")
        else:
            depends_on = task.get("depends_on", [])
            for dep in depends_on:
                if dep not in task_ids:
                    reasons.append(f"{task_id}: Unknown dependency {dep}")

        # Check files count
        files = task.get("files", [])
        if len(files) > 4:
            reasons.append(f"{task_id}: More than 4 files declared")

        # Check changed lines
        changed_lines = task.get("changed_lines", 0)
        if changed_lines is not None and int(changed_lines) > 400:
            reasons.append(f"{task_id}: declared changed_lines above 400")

        # Check status
        if task.get("status") == "complete":
            reasons.append(f"{task_id}: Task status is complete")

        # Check path escapes
        for file in files:
            path_str = file.get("path") if isinstance(file, dict) else str(file)
            if not path_str:
                continue

            p = Path(path_str)
            if p.is_absolute():
                reasons.append(f"{task_id}: Absolute path not allowed ({path_str})")
                continue

            if ".." in p.parts:
                reasons.append(f"{task_id}: Path contains .. segment ({path_str})")
                continue

            try:
                full_path = repo_root / p
                # Canonicalize using resolve(strict=False) to avoid failing if file doesn't exist yet,
                # but we MUST check if it tries to escape.
                # However, Python's resolve on non-existent symlinks is tricky.
                # We can construct the absolute path and check if the repo root is a parent.
                # We'll fail closed if we can't figure it out, or if it resolves outside.
                resolved = full_path.resolve()
                if not resolved.is_relative_to(repo_root.resolve()):
                    reasons.append(f"{task_id}: Path escapes repo root ({path_str})")
            except Exception as e:
                reasons.append(f"{task_id}: Path resolution error ({path_str})")

    # Cycle detection
    visited = set()
    in_progress = set()

    def dfs(node, depth):
        if depth > 1000:
            reasons.append(f"{node}: Cycle depth cap exceeded")
            return

        if node in in_progress:
            reasons.append(f"{node}: Cycle detected")
            return

        if node in visited:
            return

        in_progress.add(node)

        task = next((t for t in dag if t.get("id") == node), None)
        if task:
            for dep in task.get("depends_on", []):
                dfs(dep, depth + 1)

        in_progress.remove(node)
        visited.add(node)

    for task in dag:
        task_id = task.get("id")
        if task_id and task_id not in visited:
            dfs(task_id, 0)

    return reasons

def main():
    parser = argparse.ArgumentParser(description="Validate pipeline DAG")
    parser.add_argument("dag", help="Path to DAG JSON file")
    parser.add_argument("--json", action="store_true", help="Machine-readable output")

    args = parser.parse_args()

    try:
        with open(args.dag, "r") as f:
            dag = json.load(f)
    except Exception as e:
        if args.json:
            print(json.dumps({"valid": False, "reasons": [f"Failed to read DAG: {e}"]}))
        else:
            print(f"Failed to read DAG: {e}", file=sys.stderr)
        sys.exit(1)

    repo_root = Path(__file__).resolve().parent.parent

    reasons = validate(dag, repo_root)

    if reasons:
        if args.json:
            print(json.dumps({"valid": False, "reasons": reasons}))
        else:
            for r in reasons:
                print(r, file=sys.stderr)
        sys.exit(1)
    else:
        if args.json:
            print(json.dumps({"valid": True, "reasons": []}))
        sys.exit(0)

if __name__ == "__main__":
    main()
