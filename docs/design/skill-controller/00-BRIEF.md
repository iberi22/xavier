# Skill Controller — shared brief for the research & design phase

Status: design phase (no production code yet). Branch `design/skill-controller`.
Audience: every agent contributing a design document. Read fully before writing.

## Goal

Evolve Xavier from a **skill reader** into a **skill controller** for all agentic CLIs on the
machine (Claude Code, OpenCode, Codex, agy/Gemini CLI, Hermes, OpenClaw): one canonical store,
declarative placement, task-scoped ephemeral skills, drift detection, and usage telemetry.
Xavier is the controller, never the only copy: if Xavier is down, skills keep working as files.

## Verified facts (2026-09-27)

### What Xavier already has (reuse, do not rebuild)
- `src/context/skill_registry.rs` (1313 lines): scans `<workspace>/.agents/skills` and
  `$HOME/.hermes/skills` (hardcoded canonical store), symlink-cycle-safe walk, respects the
  Hermes disabled list (`$HOME/.hermes/config.yaml`), keyword + semantic search
  (EmbeddingPort + sqlite-vec), calibrated 0-1 confidence. Live: 659 skills indexed.
- `src/context/skill_dispatcher.rs` (343): task -> top skill + context pack (memories, prior
  decisions), MIN_DISPATCH_CONFIDENCE = 0.40, instructions wrapped in an UNTRUSTED boundary.
- `src/api/skills.rs`: REST `POST /api/skill/dispatch`, `GET /api/skill/list`, `GET /skills`.
- MCP tools `xavier_dispatch_skill`, `xavier_skill_list` (`src/server/mcp/tools_context.rs`).
- Session context fusion: injects top-1 skill on compaction when confidence >= 0.5.
- Ledger features already `stable`: feat-skill-scan-paths, feat-skill-semantic-rank,
  feat-skill-context-fusion, feat-skill-mcp-tool, feat-skill-loader-fix; `beta`:
  feat-skill-dispatch-confidence.
- Missing entirely: any write path (placement, symlinks), ephemeral skills, drift audit,
  usage telemetry, secret scanning of skills.

### How the tools discover skills
| Tool | Global | Project | Nested dirs |
|---|---|---|---|
| Claude Code | `~/.claude/skills` | `.claude/skills` | yes: `<subdir>/.claude/skills` when working there. Does NOT read `.agents/skills` |
| OpenCode | `~/.config/opencode/skills`, `~/.claude/skills`, `~/.agents/skills` | `.opencode/`, `.claude/`, `.agents/` skills | yes, walks up to the git root |
| Codex CLI | `~/.agents/skills` | `.agents/skills` (cwd, parents, repo root) | yes |
| Gemini CLI / agy | `~/.gemini/skills` / `~/.gemini/config/skills` (agy) | `.agents/skills` at workspace root | unverified; assume root only |
- Agent Skills spec (agentskills.io): `name` <= 64 chars matching folder, `description` <= 1024
  chars (what + when), optional `license`, `compatibility`, `metadata`, `allowed-tools`;
  body < 500 lines / < 5000 tokens; overflow to `references/`, `scripts/`, `assets/`.
- Skill listings are read at session start; a file created mid-session is generally not seen.
  Descriptions of all installed skills stay in context (~9k tokens today for ~200 skills);
  Claude Code docs warn that with many skills some lose their descriptions.

### Current machine state
- 7 skill trees, almost all symlink farms: `~/.claude/skills`, `~/.config/opencode/skills`,
  `~/.codex/skills`, `~/.openclaw/skills`, `~/.hermes/skills`, `~/.gemini/config/skills`,
  `~/.gemini/skills`. Real files live in 3 sources: `~/.hermes/skills` (119),
  `~/clawd/skills` (91), `~/.agents/skills`, plus repo-local `skills/` dirs.
- 214 distinct skill names; 1 divergent real copy (gemini). Project skills in `<repo>/skills/`
  are discovered by NO tool natively; they only work via global symlinks (so they load everywhere).
- A 2026-09-27 audit found: plaintext credentials in 6 skills, ~20 skills pointing at a retired
  service, Windows paths, tool names from other harnesses, ~12 skills contradicting each other
  on routing, ~16 without `description`. Skills rot silently; nothing re-checks them.

## Target design (to be validated/refined by this phase)

1. **Projector** (fully autonomous, deterministic, no LLM): canonical store (a git repo, likely
   `~/.agents/skills`) + declarative manifest (which skill goes to which tool/project) ->
   Xavier reconciles symlinks, idempotent, dry-run, backup before change, never deletes a
   regular file it did not create.
2. **Ephemeral skills** (autonomous inside worktrees): before an orchestrator spawns a CLI agent
   for a task, Xavier composes `<worktree>/.agents/skills/eph-<task>/SKILL.md` (+ `.claude/skills`
   link) from registry skills + memories + Code-Graph; removed when the task closes. For live
   sessions, the same composition is served over MCP (`xavier_dispatch_skill`), no files.
3. **Drift audit** (autonomous detection, human-approved changes): deterministic checks (dead
   paths/ports/endpoints, versions contradicting repos, missing frontmatter, contradictions,
   secrets) -> report + proposed diff; never auto-rewrites canonical skills; never touches
   secrets/safety rules automatically.
4. **Usage telemetry** (autonomous): record each dispatch/invocation to drive pruning.

| Task | Autonomy |
|---|---|
| Inventory, placement, symlinks | Full |
| Ephemeral skills per task | Full, in worktrees |
| MCP dispatch | Full (exists) |
| Drift detection, secret scan | Full, report only |
| Rewriting canonical skills | Proposal (diff) + approval |
| Secrets, safety, approval rules | Never automatic |

## Hard constraints for every document and future task
- Follow `AGENTS.md` of this repo (waves, ledger promoted only by green runs, ADRs, 12-factor
  config, `cargo fmt` + clippy `-D warnings`, thiserror/anyhow, Tokio+Rayon rule).
- The repo is PUBLIC: no secrets, no personal absolute paths (write `$HOME/...`), no user names.
- Prefer reusing existing Xavier modules and small well-maintained crates over new frameworks.
- No over-engineering: smallest design that meets the goal; no new service/daemon if an existing
  one (xavier.service, atlas-watch) can host it; no FUSE/VFS, no custom DSLs, no plugin systems.
- Filesystem safety: never follow symlinks out of allowed roots, never `rm -rf`, atomic writes,
  backups, dry-run default, explicit allowlist of target dirs.
- Write in English; concise; cite `file:line` for claims about the code.
