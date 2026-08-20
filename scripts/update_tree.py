import os
import re

def generate_tree(dir_path, prefix=""):
    try:
        files = sorted(os.listdir(dir_path))
    except:
        return []
    lines = []
    for i, file in enumerate(files):
        if file.startswith('.'): continue
        is_last = i == len(files) - 1
        lines.append(prefix + ("└── " if is_last else "├── ") + file)
        path = os.path.join(dir_path, file)
        if os.path.isdir(path):
            lines.extend(generate_tree(path, prefix + ("    " if is_last else "│   ")))
    return lines

full_tree = ["├── src/"]
full_tree.extend(["│   " + line for line in generate_tree("src")])
full_tree_str = "\n".join(full_tree)

with open("SRC.md", "r") as f:
    content = f.read()

new_content = re.sub(r"├── src/.*?└── telegram/", full_tree_str, content, flags=re.DOTALL)

with open("SRC.md", "w") as f:
    f.write(new_content)
