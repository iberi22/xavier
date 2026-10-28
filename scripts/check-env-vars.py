#!/usr/bin/env python3
import sys
import os
import re

# use tomllib for python 3.11+
try:
    import tomllib as toml
except ImportError:
    try:
        import tomli as toml
    except ImportError:
        import toml

def check():
    with open("docs/reference/env_vars.toml", "rb" if "tomllib" in sys.modules or "tomli" in sys.modules else "r") as f:
        data = toml.load(f)

    registry_vars = {v["name"] for v in data.get("variables", [])}

    codebase_vars = set()

    # regex for exact string literal like "XAVIER_something" or 'XAVIER_something'
    # we'll still search for general occurrences but ensure they are string literals
    for directory in ["src", "crates"]:
        for root, dirs, files in os.walk(directory):
            for file in files:
                if file.endswith(".rs") or file.endswith(".sh") or file.endswith(".js") or file.endswith(".py"):
                    with open(os.path.join(root, file), 'r', encoding='utf-8') as f:
                        content = f.read()
                        matches = re.findall(r'["\'](XAVIER_[A-Z0-9_]+)["\']', content)
                        for match in matches:
                            # Must not end in _ as those are usually prefixes for other things in codebase
                            if not match.endswith("_") and match != "XAVIER_":
                                codebase_vars.add(match)

    codebase_vars.add("XAVIER_ENV_FILE")
    codebase_vars.add("XAVIER_CODE_SYNC_INTERVAL_SECS")

    missing = codebase_vars - registry_vars
    if missing:
        print("Error: The following environment variables are found in code but missing from docs/reference/env_vars.toml:")
        for m in sorted(missing):
            print(f"  - {m}")
        sys.exit(1)

    print("All environment variables are documented.")

    with open("docs/reference/ENV_VARS.md", "r") as f:
        current_md = f.read()

    import subprocess
    subprocess.run([sys.executable, "scripts/generate-env-docs.py"], check=True)

    with open("docs/reference/ENV_VARS.md", "r") as f:
        expected_md = f.read()

    if current_md != expected_md:
        print("Error: docs/reference/ENV_VARS.md is out of date. Run scripts/generate-env-docs.py to update it.")
        sys.exit(1)

    print("ENV_VARS.md is up to date.")

if __name__ == "__main__":
    check()
