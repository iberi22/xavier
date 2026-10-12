# Skill Controller Security Review Checklist (O04)

This checklist tracks the implementation of security rules and guardrails for the skill controller (D4, D10–D14).

## Threat List
*   **Prompt injection** via ingested skills/evidence
*   **Path escape**
*   **Symlink swap (TOCTOU)**
*   **Secret leakage** in reports
*   **Privilege escalation** via MCP

## Review Checklist

| Decision | Invariant | Guard ID | Verdict |
|----------|-----------|----------|---------|
| **D4** | No MCP tool mutates skills (only list, dispatch, usage allowed in tools_context). | `guard_01` | |
| **D10** | Handlers like `/api/skill/audit` and `/api/skill/usage` take no request extractors. | `guard_02` | |
| **D11** | Skill-controller modules contain no `.unwrap()`/`.expect()` outside tests. | `guard_03` | |
| **D12** | No `std::process::Command` in `src/context/skill_controller/`. | `guard_04` | |
| **D13** | Blocking controller calls from async code go through `spawn_blocking` (enforced by no `async fn` in controller). | `guard_05` | |
| **D14** | Fencing and sanitization manual checks. | `manual` | |
