import re
import os

files = [
    "src/memory/manager/eviction.rs",
    "src/memory/manager/management.rs",
    "src/memory/manager/tests.rs"
]

for f in files:
    if os.path.exists(f):
        print(f"--- {f} ---")
        with open(f, 'r') as fd:
            content = fd.read()
            # find lines where the missing methods are called
            for line_no, line in enumerate(content.splitlines(), 1):
                if any(m in line for m in ["flatten_reorganize", "nearest_neighbors_query"]):
                    print(f"{line_no}: {line.strip()}")
