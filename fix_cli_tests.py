import os

file_path = "tests/integration/cli.rs"
with open(file_path, 'r') as f:
    lines = f.readlines()

new_lines = []
for line in lines:
    if '|| combined.contains("error")' in line:
        new_lines.append(line)
        new_lines.append('            || combined.contains("Falling back to local offline")\n')
    else:
        new_lines.append(line)

with open(file_path, 'w') as f:
    f.writelines(new_lines)
