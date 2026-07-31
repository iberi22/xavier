#!/usr/bin/env python3
"""Fail if any owned path appears in more than one Ola11 issue island."""
from collections import defaultdict
OWNED = {
  "01": ["src/server/headless/routes.rs"],
  "02": ["src/cli/handlers/offline_models.rs"],
  "03": ["src/health/mod.rs"],
  "04": ["src/memory/sqlite_vec_store/schema_impl.rs"],
  "05": ["src/memory/sqlite_vec_store/store_impl.rs", "src/memory/tests.rs"],
  "06": ["config/xavier.config.json", "src/settings/defaults.rs"],
  "07": ["src/server/panel/assets.rs"],
  "08": ["panel-ui/src/components/Onboarding/OnboardingFlow.tsx", "panel-ui/src/components/Onboarding/AuthStep.tsx"],
  "09": ["docs/ops/nixos-docker.md"],
  "10": ["installer/xavier.wxs", "docs/FEATURE_STATUS.md"],
  "11": ["docs/ops/local-ci-with-agent-priv.md"],
  "12": [".gitcore/features.json", ".gitcore/features-detailed.json", "docs/devlog/2026-07-31-ola11-harden-close.md"],
}
inv = defaultdict(list)
for issue, files in OWNED.items():
    for f in files:
        inv[f].append(issue)
bad = {f: iss for f, iss in inv.items() if len(iss) > 1}
if bad:
    raise SystemExit(f"OVERLAP: {bad}")
print("OK: disjoint islands", len(OWNED), "issues", sum(len(v) for v in OWNED.values()), "files")
