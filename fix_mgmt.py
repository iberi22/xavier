import re
import os

filepath = 'src/memory/manager/management.rs'
with open(filepath, 'r') as f:
    lines = f.readlines()

new_lines = []
skip = False
for line in lines:
    if 'impl MemoryManager {' in line and 'Execute legacy action types' in lines[lines.index(line)-1]:
        # This is the second impl block which we want to remove because we moved its content
        skip = True
        # remove the previous line which is the comment
        if new_lines and 'Execute legacy action types' in new_lines[-1]:
             new_lines.pop()
        continue

    if skip:
        if line.strip() == '}':
            skip = False
        continue

    new_lines.append(line)

# Wait, let's just remove the whole second impl block
content = "".join(new_lines)
match = list(re.finditer(r'impl MemoryManager \{', content))
if len(match) > 1:
    # There is more than one impl block. The last one is likely the one I want to remove.
    last_match = match[-1]
    content = content[:last_match.start()].rstrip()
    # Need to make sure I don't leave hanging comments if any
    if content.endswith("/// Execute legacy action types for backwards compatibility"):
        content = content[:-len("/// Execute legacy action types for backwards compatibility")].rstrip()

with open(filepath, 'w') as f:
    f.write(content)
