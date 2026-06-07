import os

filepath = 'src/memory/manager/mod.rs'
with open(filepath, 'r') as f:
    lines = f.readlines()

new_lines = []
for line in lines:
    new_lines.append(line)
    if 'pub mod quality;' in line:
        new_lines.append("pub mod management;\n")
        new_lines.append("pub mod eviction;\n")
        new_lines.append("pub mod decay;\n")
        new_lines.append("pub mod consolidation;\n")
        new_lines.append("pub mod compression;\n")
        new_lines.append("pub mod tracking;\n")

with open(filepath, 'w') as f:
    f.writelines(new_lines)
