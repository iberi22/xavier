import re
import os

files = [
    "src/consolidation/mod.rs",
    "src/server/http/api.rs",
    "src/workspace/state.rs",
    "src/memory/manager/eviction.rs",
    "src/memory/manager/management.rs"
]

for f in files:
    if os.path.exists(f):
        print(f"--- {f} ---")
        with open(f, 'r') as fd:
            content = fd.read()
            # find lines where the missing methods are called
            for line_no, line in enumerate(content.splitlines(), 1):
                if any(m in line for m in ["get_all_memories", "execute_actions", "auto_manage", "decay_memories", "flatten_reorganize", "nearest_neighbors_query"]):
                    print(f"{line_no}: {line.strip()}")
