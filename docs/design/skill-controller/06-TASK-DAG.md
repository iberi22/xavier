# Skill Controller — task DAG and traceability

Generated 2026-09-27 from `06-TASK-DAG.json` (Codex gpt-6-sol high, reviewed by the orchestrator). Atlas session `s_25e5b6ab946470`; GitHub label `skill-controller` (epics carry native sub-issues and blocked-by links). Every task issue embeds the rules block and Definition of Done of `04-RULES-AND-GUARDRAILS.md`.

| Task | Epic | Agent | Size | Depends on | Issue | Atlas |
|---|---|---|---|---|---|---|
| **EP-00** | — | — | — | — | #2584 | `t_25f9f6ab9464e0` |
| **EP-10** | — | — | — | EP-00 | #2585 | `t_25fa36ab9464e0` |
| **EP-20** | — | — | — | EP-10 | #2586 | `t_25fa46ab9464e0` |
| **EP-30** | — | — | — | EP-20 | #2587 | `t_25fa56ab9464f0` |
| **EP-40** | — | — | — | EP-30 | #2588 | `t_25fba6ab9464f0` |
| **EP-50** | — | — | — | EP-30 | #2589 | `t_25fbb6ab9464f0` |
| **EP-60** | — | — | — | EP-30 | #2590 | `t_25fbc6ab9464f0` |
| **EP-99** | — | — | — | EP-40, EP-50, EP-60 | #2591 | `t_25fbd6ab9464f0` |
| P01 Implement and run the ADR-034 store simulation | EP-00 | agy-medium | M | — | #2592 | `t_25e5c6ab946470` |
| P02 Reconcile ADR-034 with simulation and ADR-019 | EP-00 | codex | S | P01 | #2593 | `t_25e616ab946470` |
| P03 Accept or reject ADR-034 | EP-00 | human | S | P02 | #2594 | `t_25e686ab946470` |
| P04 Correct project skill source documentation and REQ-060 | EP-00 | codex | S | — | #2595 | `t_25e7c6ab946470` |
| P05 Reconcile store, registry, and projector feature specs | EP-00 | codex | S | — | #2596 | `t_25ed76ab946470` |
| P06 Reconcile ephemeral, drift, and telemetry feature specs | EP-00 | codex | S | — | #2597 | `t_25edc6ab946470` |
| P07 Verify six planned ledger entries and their tests | EP-00 | codex | S | P05, P06 | #2598 | `t_25ee16ab946480` |
| S01 Make skill store paths configurable with identical defaults | EP-10 | jules | M | P03, P04, P07 | #2599 | `t_25ee66ab946480` |
| R01 Extract the shared registry constructor | EP-20 | jules | S | S01 | #2600 | `t_25ef56ab946480` |
| R02 Use shared registry construction in REST | EP-20 | jules | S | R01 | #2601 | `t_25efd6ab946480` |
| R03 Use shared registry construction in MCP | EP-20 | jules | S | R01 | #2602 | `t_25f026ab946490` |
| R04 Unify fusion construction and document confidence constants | EP-20 | jules | S | R01 | #2603 | `t_25f076ab946490` |
| J01 Parse and validate TOML manifest v1 | EP-30 | jules | M | R02, R03, R04 | #2604 | `t_25f0d6ab946490` |
| J02 Implement host policy and publication secret gate | EP-30 | agy-medium | M | J01 | #2605 | `t_25f126ab946490` |
| J03 Build immutable dry-run plans | EP-30 | jules | M | J02 | #2606 | `t_25f1a6ab946490` |
| J04 Implement ownership journal and backup state | EP-30 | agy-medium | M | J02 | #2607 | `t_25f1f6ab9464a0` |
| J05 Apply plans with atomic per-skill publication | EP-30 | agy-medium | M | J03, J04 | #2608 | `t_25f246ab9464a0` |
| J06 Conditionally roll back owned projections | EP-30 | jules | S | J05 | #2609 | `t_25f2a6ab9464a0` |
| J07 Implement the CLI skills handler | EP-30 | jules | M | J06 | #2610 | `t_25f336ab9464a0` |
| J08 Wire controller modules and CLI subcommands | EP-30 | agy-medium | M | J07 | #2611 | `t_25f386ab9464b0` |
| E01 Compose bounded, scoped task context | EP-40 | agy-medium | M | J08 | #2612 | `t_25f3d6ab9464b0` |
| E02 Publish and release task-scoped skills before agent spawn | EP-40 | agy-medium | M | E01 | #2613 | `t_25f526ab9464b0` |
| E03 Deliver the same composition to live sessions | EP-40 | agy-medium | M | E02, R03, R04 | #2614 | `t_25f5d6ab9464b0` |
| D01 Audit drift and generate redacted proposals | EP-50 | agy-medium | M | J08 | #2615 | `t_25f626ab9464c0` |
| D02 Expose report-only drift views | EP-50 | jules | M | D01, J07, R02 | #2616 | `t_25f676ab9464c0` |
| T01 Persist privacy-preserving lifecycle events | EP-60 | agy-medium | M | J08 | #2617 | `t_25f6c6ab9464c0` |
| T02 Record selection, delivery, and acknowledged use | EP-60 | jules | M | T01, E01 | #2618 | `t_25f746ab9464c0` |
| T03 Expose usage summaries on CLI, REST, and MCP | EP-60 | agy-medium | M | T02, D02, E03 | #2619 | `t_25f796ab9464d0` |
| O01 Test Codex, Claude Code, and OpenCode adapters with Xavier stopped | EP-99 | opencode-free | M | T03 | #2620 | `t_25f7e6ab9464d0` |
| O02 Test Gemini, agy, Hermes, and OpenClaw adapters with Xavier stopped | EP-99 | opencode-free | M | T03 | #2621 | `t_25f836ab9464d0` |
| O03 Write the measured migration and rollback runbook | EP-99 | codex | S | O01, O02 | #2622 | `t_25f886ab9464d0` |
| O04 Review security of the complete initiative | EP-99 | claude-cloud | M | O03, E03, D02, T03 | #2623 | `t_25f906ab9464e0` |
| O05 Approve migration go/no-go | EP-99 | human | S | O04 | #2624 | `t_25f956ab9464e0` |
| O06 Migrate approved real skill sources in bounded batches | EP-99 | human | L | O05 | #2625 | `t_25f9a6ab9464e0` |
