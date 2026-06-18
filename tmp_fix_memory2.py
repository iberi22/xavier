#!/usr/bin/env python3
"""Fix closing parens in tools_memory.rs after MCPContent::Text wrapper was added"""
import re

with open('E:/cortex/xavier/src/server/mcp/tools_memory.rs', 'r') as f:
    content = f.read()

# Strategy: find all blocks that now have MCPContent::Text(MCPTextContent {
# and fix the corresponding closing lines

# In the `get_memory` handler (~line 305): `}],` -> `})],`
# Pattern: inside MCPToolResult content: vec![MCPContent::Text(MCPTextContent { ... }],
# The old `}],` closes the MCPTextContent + vec. Now need `})],` to close MCPContent::Text + MCPTextContent + vec

# For `content: vec![MCPContent::Text(MCPTextContent {` blocks, 
# the closing `}],` should become `})],`

# Count how many we have
pairs = list(re.finditer(r'content:\s*vec!\[MCPContent::Text\(MCPTextContent \{', content))
print(f"Found {len(pairs)} vec![MCPContent::Text(MCPTextContent blocks")

# For each match, find the matching closing }], 
# Strategy: track brace depth from the MCPTextContent { line

for m in pairs:
    start = m.end() - 1  # position of the { in MCPTextContent {
    
    # Find the corresponding closing }], for this specific MCPTextContent
    # Scan forward from start, counting brace depth
    depth = 1
    pos = start + 1
    while pos < len(content) and depth > 0:
        ch = content[pos]
        if ch == '{':
            depth += 1
        elif ch == '}':
            depth -= 1
        pos += 1
    
    # Now pos is after the closing }
    # Check if followed by `],` (close vec)
    if pos < len(content) and content[pos:pos+2] == '],':
        # This closes the struct + vec
        # But we need to also close the MCPContent::Text wrapper
        # Insert `)` before `],`
        content = content[:pos] + ')' + content[pos:]
        print(f"  Fixed closing at position {pos}: inserted ')' before '],'")

open('E:/cortex/xavier/src/server/mcp/tools_memory.rs', 'w').write(content)
print("Done fixing vec closing parens")

# Now check .map() closures
# They have MCPContent::Text(MCPTextContent { ... })
# The old closing was }), .collect() 
# Now need })), .collect()  (extra ) for MCPContent::Text)
# Count occurrences
with open('E:/cortex/xavier/src/server/mcp/tools_memory.rs', 'r') as f:
    content2 = f.read()

# Find the }).collect() patterns after MCPContent::Text
# The old ) closes the closure. Now we need )) to close MCPContent::Text + closure
pairs2 = list(re.finditer(r'                }\)\n                \.collect\(\);', content2))
num2 = len(pairs2)
print(f"Found {num2} patterns to fix")

for m in pairs2:
    # Check if preceded by MCPContent::Text
    before = content2[max(0, m.start()-200):m.start()]
    if 'MCPContent::Text' in before:
        # The } closes MCPTextContent struct
        # First ) closes the closure 
        # We need an extra ) for MCPContent::Text between them
        # So: }}) .collect()
        # Instead of })\n.collect() -> }))\n.collect()
        pos = m.start() + 1  # position after the first }
        content2 = content2[:pos] + ')' + content2[pos:]
        print(f"  Fixed map closing at position {pos}")

# Also check the }) that may be on one line
pairs3 = list(re.finditer(r'                }\)\n                \.collect\(\);', content2))
num3 = len(pairs3)
print(f"Remaining patterns: {num3}")

open('E:/cortex/xavier/src/server/mcp/tools_memory.rs', 'w').write(content2)
print("All done")
