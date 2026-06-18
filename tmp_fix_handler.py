with open('E:/cortex/xavier/src/server/mcp/tools_memory.rs', 'r', encoding='utf-8') as f:
    content = f.read()

old = '        "search_memory" => {'
new = '        "mem_search" | "search_memory" => {'

count = content.count(old)
print(f'Found {count}')
if count == 1:
    content = content.replace(old, new, 1)

with open('E:/cortex/xavier/src/server/mcp/tools_memory.rs', 'w', encoding='utf-8') as f:
    f.write(content)
print('Done')
