#!/usr/bin/env python3
"""Fix the remaining memoryfragment_get closing paren"""
with open('E:/cortex/xavier/src/server/mcp/tools_memory.rs', 'rb') as f:
    content = f.read()

# Find the pattern: `}],` after a MCPContent::Text(MCPTextContent { that's on one line
# Specifically: the memoryfragment_get handler
old = b'                }],\n                is_error: Some(false),'
new = b'                })],\n                is_error: Some(false),'

count = content.count(old)
print(f"Found {count} occurrences of the closing pattern")

content = content.replace(old, new, 1)

with open('E:/cortex/xavier/src/server/mcp/tools_memory.rs', 'wb') as f:
    f.write(content)

print("Fixed")
