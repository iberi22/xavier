import re
with open('E:/cortex/xavier/src/server/mcp/tools_memory.rs', 'r') as f:
    content = f.read()
names = re.findall(r'name: "(.*?)"\.to_string\(\)', content)
for n in names:
    print(n)
