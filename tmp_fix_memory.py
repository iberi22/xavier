#!/usr/bin/env python3
"""Fix compilation errors in tools_memory.rs"""
import re

with open('E:/cortex/xavier/src/server/mcp/tools_memory.rs', 'r') as f:
    content = f.read()

# Fix 1: v.u64() -> v.as_u64()
count1 = content.count('.and_then(|v| v.u64())')
content = content.replace('.and_then(|v| v.u64())', '.and_then(|v| v.as_u64())')
print(f"Fix 1: u64 -> as_u64: {count1} replacement(s)")

# Fix 2: get_memory - MCPTextContent -> MCPContent::Text(MCPTextContent ...)
# Pattern: content: vec![MCPTextContent { ... }],
# We need to find lines with MCPTextContent inside MCPToolResult content
lines = content.split('\n')
fixed_lines = 0
for i, line in enumerate(lines):
    stripped = line.strip()
    if stripped.startswith('content: vec![MCPTextContent {'):
        indent = line[:len(line) - len(line.lstrip())]
        lines[i] = indent + 'content: vec![MCPContent::Text(MCPTextContent {'
        fixed_lines += 1
        
        # Match the closing }], for this vec
        # Find the closing )], of MCPToolResult
        # Actually for the simpler case, we look for lines starting with }], and indent
        # But let's be smarter - find pairs

print(f"Fix 2: MCPTextContent -> MCPContent::Text wrappers: (to be done in next step)")

# Let's do it differently - replace line by line for the MCPTextContent in vec macros
# Using regex to replace `vec![MCPTextContent {` with `vec![MCPContent::Text(MCPTextContent {`
# and then close the extra parens
content = re.sub(
    r'(content:\s*vec!\[\s*)MCPTextContent\s*\{',
    r'\1MCPContent::Text(MCPTextContent {',
    content
)
print(f"Fix 2a: vec![MCPTextContent -> vec![MCPContent::Text(MCPTextContent: regex done")

# Now we need to close the extra parens for MCPContent::Text(MCPTextContent { ... })
# The closing is: }], -> })]
# But we need to be careful not to double-close
# Each replaced pattern: the `}]` that closes the vec should become `})]`
# Let's find all MCPContent::Text(MCPTextContent { ... }]), patterns
# and count paren depth to close properly

# Actually simpler: after replacing `vec![MCPTextContent {` with `vec![MCPContent::Text(MCPTextContent {`,
# each occurrence of `}]` that closes the vec content for MCPToolResult needs `})]`
# Let's use a different approach: find MCPToolResult blocks

open('E:/cortex/xavier/src/server/mcp/tools_memory.rs', 'w').write(content)
print("Intermediate write done. Now fixing closing parens...")

# Now fix the MCPTextContent inside .map() iterators
# Pattern: .map(|doc| MCPTextContent {
# These need to become .map(|doc| MCPContent::Text(MCPTextContent {
# and the closing }), needs to become }))
# But this is complex. Let's do it carefully.

content2 = open('E:/cortex/xavier/src/server/mcp/tools_memory.rs', 'r').read()

# For .map() closures, the pattern is:
# .map(|doc| MCPTextContent {
#   ...
# })
# We need: .map(|doc| MCPContent::Text(MCPTextContent {
#   ...
# }))
# So: add MCPContent::Text( before MCPTextContent, 
# and change }) at end of map to }))

# Find all .map(|doc| MCPTextContent { and count how many
map_count = content2.count(".map(|doc| MCPTextContent {")
map2_count = content2.count(".map(|record| MCPTextContent {")
print(f"Fix 3: .map(|doc| MCPTextContent {{: {map_count} occurrence(s)")
print(f"Fix 4: .map(|record| MCPTextContent {{: {map2_count} occurrence(s)")

# Replace .map(|doc| MCPTextContent { -> .map(|doc| MCPContent::Text(MCPTextContent {
content2 = content2.replace(
    ".map(|doc| MCPTextContent {",
    ".map(|doc| MCPContent::Text(MCPTextContent {"
)
content2 = content2.replace(
    ".map(|record| MCPTextContent {",
    ".map(|record| MCPContent::Text(MCPTextContent {"
)

# Now fix closing: need to find each map's closing }) and make it }))
# The .map() closures end with }) (closing the closure parens)
# But MCPContent::Text(MCPTextContent { ... }) also needs closing parens
# So }) becomes }))

# We need to be careful: the existing closures for .map(|doc| ... are:
# .map(|doc| MCPContent::Text(MCPTextContent {
#     content_type: "text".to_string(),
#     text: format!(...),
# })
# The }) closes both the struct and the closure. But now we need an extra ):
# }))

# Let's find }) that are followed by .collect() 
# These are the ones we want
content2 = content2.replace(
    "                })\n                .collect();",
    "                }))\n                .collect();"
)

open('E:/cortex/xavier/src/server/mcp/tools_memory.rs', 'w').write(content2)
print("All fixes applied")
