import subprocess, json

result = subprocess.run(['gh','issue','list','--limit','50','--json','number,title,state,labels'], capture_output=True, text=True, encoding='utf-8', errors='replace')
issues = json.loads(result.stdout)

existing_titles = {i['title'] for i in issues}

print("=== ISSUES OPEN ===")
for i in issues:
    if i['state'] == 'OPEN':
        labels = ','.join([l['name'] for l in i['labels']])
        print(f'  #{i["number"]} | {labels:40s} | {i["title"]}')

print()
print("=== ISSUES CLOSED ===")
for i in issues:
    if i['state'] != 'OPEN':
        labels = ','.join([l['name'] for l in i['labels']])
        print(f'  #{i["number"]} | {labels:40s} | {i["title"]}')

print()
print("=== NUEVOS FEATURES A CREAR ===")
new_features = [
    ("feat-governance-dao", "Bicameral DAO on-chain integration"),
    ("feat-runtime-health", "Native runtime health & self-monitoring loop"),
    ("feat-auto-improvement", "Auto-improvement loop (autoresearch-style)"),
    ("feat-dual-license", "Dual License (MIT + Mesh License)"),
    ("feat-context-regeneration", "Context regeneration & perfect recall loop"),
]
for fid, title in new_features:
    found = any(title in t for t in existing_titles)
    print(f'  {"[EXISTS]" if found else "[NEW]":8s} {fid:40s} | {title}')
