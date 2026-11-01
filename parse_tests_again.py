import json

with open(".gitcore/features.json", "r") as f:
    data = json.load(f)

features = [
    "feat-codegraph-walk-hardening",
    "feat-generative-ui",
    "feat-headless-server-packaging",
    "fix-daemon-serve-while-starting"
]

for feature in features:
    print(f"\n--- {feature} ---")
    feat = data["features"][feature]
    tests = feat.get('tests', [])
    for test in tests:
        print(f"{test}")
