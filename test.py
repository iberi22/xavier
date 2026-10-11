import os
import sys

changed_files = ["docs/x.md", "panel-ui/src/main/lib/anything.ts", ".github/ISSUE_TEMPLATE/bug.md", "README.md", "nested/path/to/readme.md"]
all_non_rust = True
for filepath in changed_files:
    if filepath.startswith('docs/') or filepath.endswith('.md') or \
       (filepath.startswith('panel-ui/') and not filepath.startswith('panel-ui/src-tauri/')) or \
       filepath.startswith('.github/ISSUE_TEMPLATE/'):
        pass
    else:
        all_non_rust = False
        break

print(all_non_rust)
