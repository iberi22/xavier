import json, sys
data = json.load(sys.stdin)
print('ULTIMOS PRs MERGEADOS:')
for pr in data:
    print(f'  #{pr["number"]} - {pr["title"][:60]} | {pr["mergedAt"][:10]}')
