#!/usr/bin/env python3
import re

with open('E:/cortex/xavier/src/server/mcp/tools_memory.rs', 'r', encoding='utf-8') as f:
    content = f.read()

old = 'name: "memory_context".to_string(),\n            description: "Build an aggregated context block from the most relevant memories for a query".to_string(),'
new = 'name: "memory_context".to_string(),\n            description: "Build an aggregated context block from the most relevant memories for a query. Returns full content bounded by max_chars. Use AFTER mem_search to identify the right memories.".to_string(),'

if old in content:
    content = content.replace(old, new, 1)
    print("Found and replaced memory_context description")
else:
    print("memory_context description not found")
    # Try to find it
    idx = content.find('memory_context')
    if idx >= 0:
        print(f"Found at offset {idx}")
        print(repr(content[idx:idx+300]))

with open('E:/cortex/xavier/src/server/mcp/tools_memory.rs', 'w', encoding='utf-8') as f:
    f.write(content)
