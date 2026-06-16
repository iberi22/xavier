import subprocess, json

issues = [
    {
        "title": "feat-governance-dao: Bicameral DAO on-chain integration",
        "labels": ["enhancement","mesh-network","design"],
        "body": """## Goal
Migrate the existing bicameral governance mock (src/data_commons/governance.rs, 854 lines) to real on-chain integration.

## Current State
- src/data_commons/governance.rs - Bicameral DAO design (50% users + 50% council) with XIP lifecycle, quorum, veto
- src/mesh/governance.rs - Mock DAO with proposals, votes, trust scores (147 lines)
- Both are in-memory/JSON only - NO on-chain integration

## Implementation Phases
- Phase A: Replace mock with SQLite-backed DAO (persist state)
- Phase B: Add REST endpoints for proposal CRUD + voting
- Phase C: Smart contract + web3 integration
- Phase D: UI panel for proposal browsing

## Acceptance Criteria
- Proposals can be created, viewed, voted on via REST API
- Voting weight = XP * 0.4 + reputation * 0.3 + activity_7d * 0.3
- Quorum verification (20% minimum, 33% for critical)
- Council veto (66%) + community override (75%)
- 7-day activity requirement for voting
- All DAO state persisted in SQLite
"""
    },
    {
        "title": "feat-runtime-health: Native runtime health and self-monitoring loop",
        "labels": ["enhancement","jules"],
        "body": """## Goal
Build a native runtime health loop INSIDE the xavier binary (not an external PS1 script) that continuously monitors system health, database integrity, embedding providers, and mesh peers.

## Current State
Dogfood cycle is an external PS1 script (xavier-dogfood-cycle.ps1) with no integration into Xavier itself.

## What Needs Building
- System health checks (disk usage, CPU, RAM, uptime)
- Database integrity (SQLite VACUUM, page count, WAL size, corruption check)
- Embedding health (provider ping, latency, error rate)
- Mesh peer health (connectivity, sync lag, trust scores)
- Health event bus that feeds into notification system
- REST endpoint: GET /health with structured JSON response
- Auto-repair: VACUUM if fragmentation > 30%, reconnect to mesh peers if lag > 60s

## Implementation Complexity: MEDIUM (many pieces but all self-contained)
"""
    },
    {
        "title": "feat-auto-improvement: Auto-improvement loop (autoresearch-style)",
        "labels": ["enhancement","jules","design"],
        "body": """## Goal
Closed-loop auto-improvement inside Xavier: benchmark -> gap analysis -> generate experiment -> validate -> merge -> re-measure.

Inspired by E:\\scripts-python\\autoresearch program.

## What Needs Building
- Run benchmarks against real usage data (recall@k, precision, latency)
- Identify gaps (recall < 100%, latency > threshold)
- Generate experiment configs (chunk overlap, RRF weights, policy params)
- Apply experiment and re-benchmark
- Accept/reject based on improvement
- Generate PR if improvement confirmed
- Track improvement history over time

## Dependencies
- Depends on feat-runtime-health for the health loop infrastructure
- Depends on feat-context-regeneration for benchmark data

## Implementation Complexity: HIGH (systemic, touches many modules)
"""
    },
    {
        "title": "feat-dual-license: Dual License (MIT + Mesh License)",
        "labels": ["enhancement","design"],
        "body": """## Goal
Implement dual license system: MIT for standalone use, Mesh License for network participation.

## What Needs Building
- LICENSE-MIT and LICENSE-MESH files
- License detection at startup (requires user acceptance for mesh)
- Feature gating: mesh & governance require Mesh License
- XP rewards and voting rights only under Mesh License
- Data Commons opt-in consent only under Mesh License
- Cargo.toml: package.license = "MIT OR Xavier-Mesh-1.0"

## Acceptance Criteria
- Running xavier without args shows license info on first run
- mesh commands rejected with clear message if Mesh License not accepted
- standalone features (memory, search, encryption) work under MIT
- License acceptance persisted in config

## Implementation Complexity: LOW (config checks + feature gates)
"""
    },
    {
        "title": "feat-context-regeneration: Context regeneration and perfect recall loop",
        "labels": ["enhancement","jules","design"],
        "body": """## Goal
Continuous context regeneration using real usage data to drive recall@k toward 100% on production benchmarks.

## What Needs Building
- Build benchmark suite from real query logs
- Measure recall@k, MRR, precision over benchmark set
- Auto-tune RRF weights per query category
- Auto-tune embedding chunk size and overlap
- Auto-tune HORMER navigation policy
- Report benchmark drift over time
- Alert when recall drops below threshold

## Dependencies
- Depends on feat-runtime-health for the runtime loop
- Depends on feat-auto-improvement for the auto-optimization
- Uses existing BM25 + Vector search infrastructure

## Implementation Complexity: VERY HIGH (full system integration)
"""
    }
]

for issue in issues:
    cmd = ['gh', 'issue', 'create',
           '--title', issue['title'],
           '--label', ','.join(issue['labels']),
           '--body', issue['body']]
    result = subprocess.run(cmd, capture_output=True, text=True, encoding='utf-8', errors='replace')
    if result.returncode == 0:
        print(f"CREATED: {result.stdout.strip()}")
    else:
        print(f"FAILED: {issue['title']}")
        print(f"  {result.stderr[:200]}")
