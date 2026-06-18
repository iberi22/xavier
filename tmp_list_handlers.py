import re
with open('E:/cortex/xavier/src/server/mcp/tools_memory.rs', 'r', encoding='utf-8') as f:
    content = f.read()
idx = content.find('fn handle_memory_tool')
section = content[idx:idx+3000]
matches = re.findall(r'"(.+?)"\s*=>\s*\{', section)
for m in matches:
    print(m)
