#!/usr/bin/env python3
"""Fix remaining closure issues in tools_memory.rs"""
with open('E:/cortex/xavier/src/server/mcp/tools_memory.rs', 'r', encoding='utf-8') as f:
    content = f.read()

# Fix memory_search closure
old2 = '                })\n                .collect();\n\n            Ok(serde_json::to_value(MCPToolResult {\n                content,\n                is_error: Some(false),\n            })?)\n        }\n        "memory_context" =>'
new2 = '                }))\n                .collect();\n\n            Ok(serde_json::to_value(MCPToolResult {\n                content,\n                is_error: Some(false),\n            })?)\n        }\n        "memory_context" =>'

count = content.count(old2)
print(f'Pattern 2 found: {count}')
content = content.replace(old2, new2, 1)

# Also fix search_fragments if needed
old3 = '    }))\n                .collect();'
new3 = '                }))\n                .collect();'
content = content.replace(old3, new3, 1)

with open('E:/cortex/xavier/src/server/mcp/tools_memory.rs', 'w', encoding='utf-8') as f:
    f.write(content)
print('Done')
