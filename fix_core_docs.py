import os

filepath = 'src/memory/manager/core.rs'
with open(filepath, 'r') as f:
    lines = f.readlines()

new_lines = []
for line in lines:
    if line.startswith('//!'):
        # Just remove them or keep them at the top
        new_lines.append(line)
    else:
        new_lines.append(line)

# The issue is I added imports BEFORE the //! comments.
# Let's move imports after comments.

comments = [l for l in lines if l.startswith('//!') or l.startswith('/*!')]
others = [l for l in lines if not (l.startswith('//!') or l.startswith('/*!'))]

with open(filepath, 'w') as f:
    f.writelines(comments)
    f.writelines(others)
