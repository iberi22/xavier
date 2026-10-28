#!/usr/bin/env python3
import sys

# use tomllib for python 3.11+
try:
    import tomllib as toml
except ImportError:
    try:
        import tomli as toml
    except ImportError:
        import toml

def generate():
    with open("docs/reference/env_vars.toml", "rb" if "tomllib" in sys.modules or "tomli" in sys.modules else "r") as f:
        data = toml.load(f)

    variables = data.get("variables", [])

    categories = {}
    for v in variables:
        cat = v.get("category", "Uncategorized")
        if cat not in categories:
            categories[cat] = []
        categories[cat].append(v)

    out = []

    for cat in sorted(categories.keys()):
        out.append(f"## {cat}")
        out.append("")
        out.append("| Variable | Type | Default | Description | Secret |")
        out.append("|----------|------|---------|-------------|--------|")

        for v in sorted(categories[cat], key=lambda x: x["name"]):
            name = v["name"]
            typ = v.get("type", "string")
            default = v.get("default", "")
            desc = v.get("description", "")
            secret = "✅" if v.get("secret", False) else "❌"

            if default and not default.startswith("`") and not default.startswith("("):
                default = f"`{default}`"

            out.append(f"| `{name}` | `{typ}` | {default} | {desc} | {secret} |")
        out.append("")

    with open("docs/reference/ENV_VARS.md", "r") as f:
        md = f.read()

    start_marker = "<!-- BEGIN AUTOGEN -->"
    end_marker = "<!-- END AUTOGEN -->"

    start_idx = md.find(start_marker)
    end_idx = md.find(end_marker)

    if start_idx == -1 or end_idx == -1:
        print("Error: Missing autogen markers in ENV_VARS.md", file=sys.stderr)
        sys.exit(1)

    new_md = md[:start_idx + len(start_marker)] + "\n\n" + "\n".join(out) + "\n" + md[end_idx:]

    with open("docs/reference/ENV_VARS.md", "w") as f:
        f.write(new_md)

if __name__ == "__main__":
    generate()
