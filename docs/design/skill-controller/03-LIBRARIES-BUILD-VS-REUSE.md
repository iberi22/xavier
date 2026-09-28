# Libraries: Build vs. Reuse Decision Matrix

Status: Design Phase (Target: Xavier Skill Controller)  
Context: [docs/design/skill-controller/00-BRIEF.md](../../../docs/design/skill-controller/00-BRIEF.md)  
Repo Constraint: Public repo, zero secrets, zero personal absolute paths (strictly use `$HOME`).

---

## 1. Executive Summary & Inventory of Existing Dependencies

Before introducing new crates, existing dependencies in [Cargo.toml](../../../Cargo.toml) and `Cargo.lock` were inspected to maximize reuse and maintain a zero-bloat binary footprint:

| Crate | Location in Repo | Version in Lock/Toml | License | Skill Controller Applicability |
|---|---|---|---|---|
| `toml` | [Cargo.toml:262](../../../Cargo.toml) | `0.8.2` | MIT / Apache-2.0 | **Reuse directly**: Declarative placement manifest parsing (`skills.toml`). |
| `pulldown-cmark` | [Cargo.toml:295](../../../Cargo.toml) | `0.13.4` | MIT | **Reuse directly**: Markdown AST traversal for section & token budget audits. |
| `walkdir` | [Cargo.toml:321](../../../Cargo.toml) | `2.5.0` | Unlicense / MIT | **Reuse directly**: Recursive directory traversal for skill stores. |
| `notify` | [Cargo.toml:323](../../../Cargo.toml) | `8.2.0` | CC0-1.0 | **Do not expand**: Passive CLI/cron model is preferred; keep existing watcher. |
| `regex` | [Cargo.toml:338](../../../Cargo.toml) | `1.10.6` | MIT / Apache-2.0 | **Reuse directly**: Secret pattern matching & token heuristics. |
| `aho-corasick` | [Cargo.toml:346](../../../Cargo.toml) | `1.1.3` | MIT / Unlicense | **Reuse directly**: Multi-pattern high-throughput string matching for drift checks. |
| `serde_yaml` | [Cargo.toml:364](../../../Cargo.toml) | `0.9.34+deprecated` | MIT / Apache-2.0 | **Deprecate / Replace**: Officially archived upstream; evaluate modern pure-Rust replacement. |
| `tempfile` | [Cargo.toml:388](../../../Cargo.toml) | `3.27.0` | MIT / Apache-2.0 | **Promote to dependencies**: Atomic file writes via tempfile + rename. |
| `ignore` | [code-graph/Cargo.toml:31](../../../code-graph/Cargo.toml) | `0.4.33` | Dual Unlicense / MIT | **Promote as needed**: Workspace `.gitignore`-respecting skill discovery. |
| `globset` | [code-graph/Cargo.toml:32](../../../code-graph/Cargo.toml) | `0.4.20` | Dual Unlicense / MIT | **Promote to root**: Placement rules matching (`include = ["swal-*"]`). |
| `similar` | `Cargo.lock` (via `mockito`) | `2.7.0` | Apache-2.0 | **Promote to dependencies**: Unified diff generation for drift proposals. |
| `same-file` | `Cargo.lock` (via `walkdir`) | `1.0.6` | Unlicense / MIT | **Reuse via walkdir / promote**: Device/inode symlink identity verification. |

---

## 2. Build-vs-Reuse Decision Table

| Functional Domain | Evaluated Options | Recommendation | Target Version | Maintenance Status | License Compatibility | Rationale & Architectural Fit |
|---|---|---|---|---|---|---|
| **1. Frontmatter / YAML Parsing** | `serde_yaml`, `serde_yml`, `serde_norway`, `saphyr`, `gray_matter` | **Build small extractor + add `serde-saphyr` (or `serde_norway`)** | `saphyr 0.1.0` / `serde-saphyr 0.0.17` (alt: `serde_norway 0.9.47`) | `serde_yaml`: archived.<br>`serde_yml`: deprecated (shim).<br>`saphyr`: active 2026.<br>`serde_norway`: active fork. | MIT / Apache-2.0 (compatible with AGPL-3.0) | `serde_yaml` ([Cargo.toml:364](../../../Cargo.toml)) is archived; `serde_yml` is deprecated due to RUSTSEC-2025-0068. `gray_matter` (0.3.2) brings redundant `yaml-rust2` parsing. `src/context/skill_registry.rs:716-745` already uses a zero-copy line splitter for `name` and `description`. For full YAML validation (Hermes `config.yaml` at [src/context/skill_registry.rs:130](../../../src/context/skill_registry.rs) and rich frontmatter metadata), adopt pure-Rust, panic-free `serde-saphyr` (or drop-in `serde_norway`) to eliminate unmaintained C-FFI bindings. |
| **2. Directory Walking** | `walkdir`, `ignore` | **Reuse existing `walkdir`** (promote `ignore` if gitignore support required) | `walkdir 2.5.0`<br>`ignore 0.4.33` | Actively maintained by BurntSushi / rust-lang | Unlicense / MIT | `walkdir = "2"` is already in [Cargo.toml:321](../../../Cargo.toml). Xavier's canonical registry crawler in [src/context/skill_registry.rs:157-235](../../../src/context/skill_registry.rs) uses `walkdir` with custom inode/dev symlink cycle protection. `ignore` is already compiled in [code-graph/Cargo.toml:31](../../../code-graph/Cargo.toml); only promote if project-level `.gitignore` filtering is strictly needed. |
| **3. Glob Matching** | `globset` | **Promote existing `globset` to root** | `globset 0.4.20` | Actively maintained (BurntSushi) | Dual Unlicense / MIT | Already locked in `Cargo.lock` and [code-graph/Cargo.toml:32](../../../code-graph/Cargo.toml). Single DFA compilation for fast batch evaluation of manifest placement rules (e.g. `include = ["rust-*"]`, `exclude = ["legacy-*"]`). Zero new external crates. |
| **4. Atomic File Writes** | `tempfile`, `atomicwrites` | **Reuse existing `tempfile`** (promote from dev-deps) | `tempfile 3.27.0` | Actively maintained | MIT / Apache-2.0 | `tempfile` is already at [Cargo.toml:388](../../../Cargo.toml). Promoting to runtime dependencies enables `NamedTempFile::new_in(parent_dir)` + `.persist(target_path)`, guaranteeing same-filesystem POSIX `rename(2)` atomicity. `atomicwrites` (0.4.4) is abandoned. |
| **5. Symlink Creation & Inspection** | `std::os::unix::fs`, `same-file` | **Build small with `std::os::unix::fs` + reuse `same-file`** | `std` (Rust 1.94+)<br>`same-file 1.0.6` | `std`: tier 1.<br>`same-file`: actively maintained. | Unlicense / MIT | Symlink creation and atomic swap (create temporary symlink then `rename` over old destination) require only `std::os::unix::fs::symlink` and `std::fs::rename` (already demonstrated in [src/context/skill_registry.rs:863-864](../../../src/context/skill_registry.rs)). `same-file` is already locked via `walkdir` to verify link destinations without dereferencing loops. |
| **6. File Watching** | `notify`, `notify-debouncer-full` | **Do NOT adopt debouncer / keep on-demand execution** | `notify 8.2.0` (existing) | Actively maintained | CC0-1.0 | Continuous filesystem watching across 7 home symlink trees risks inotify exhaustion and race conditions with active agent sessions. Per [00-BRIEF.md:41,86-87](../../../docs/design/skill-controller/00-BRIEF.md), agents load skills on session start; Xavier's controller performs reconciliation on CLI invocation (`xavier skill sync`), daemon startup, or timer. Keep existing `notify` in [Cargo.toml:323](../../../Cargo.toml) for existing memory features; do not introduce `notify-debouncer-full`. |
| **7. Secret Detection** | `scripts/check-secrets.sh` (gitleaks) vs. regex set vs. external crate | **Build small native regex scanner in Rust** | N/A (uses `regex 1.10` & `aho-corasick 1.1`) | Core crates actively maintained | MIT / Apache-2.0 | `scripts/check-secrets.sh:16-26` relies on `gitleaks` or bash grep targeting git-tracked files. Skill controller drift audit inspects non-git directories (`$HOME/.hermes/skills`, `$HOME/.agents/skills`, `<workspace>/.agents/skills`). Spawning external CLI processes is slow and fragile. Reusing `regex` ([Cargo.toml:338](../../../Cargo.toml)) and `aho-corasick` ([Cargo.toml:346](../../../Cargo.toml)) with patterns from [scripts/check-secrets.sh:23](../../../scripts/check-secrets.sh) yields zero external dependencies and sub-millisecond in-memory audits. |
| **8. TOML Manifest** | `toml` | **Reuse existing `toml`** | `toml 0.8.2` | Actively maintained | MIT / Apache-2.0 | [Cargo.toml:262](../../../Cargo.toml) already includes `toml = "0.8"`. Skill controller configuration manifests (`skills.toml`) deserialize cleanly into strongly typed Serde structures. Zero new crates. |
| **9. Diff / Patch Generation** | `similar`, `diffy` | **Promote existing `similar` to root** | `similar 2.7.0` | Actively maintained (mitsuhiko) | Apache-2.0 | `similar` 2.7.0 is already present in `Cargo.lock` (transitive via `mockito` in [Cargo.toml:386](../../../Cargo.toml)). Adding `similar = { version = "2.7", features = ["unified-diff"] }` adds zero new compile nodes. Outputs standard unified diffs for human review per [00-BRIEF.md:78](../../../docs/design/skill-controller/00-BRIEF.md) ("Proposal (diff) + approval"). |
| **10. Markdown Parsing & Section Audit** | `pulldown-cmark` | **Reuse existing `pulldown-cmark`** | `pulldown-cmark 0.13.4` | Actively maintained (rust-lang) | MIT | Already present in [Cargo.toml:295](../../../Cargo.toml). Pull-event streaming parser extracts headings (`# Title`, `## Description`, `## Usage`), counts code blocks, and validates structural compliance without heap-heavy full-document AST generation. |

---

## 3. Deep-Dive Evaluations

### 3.1 Frontmatter & YAML Parsing
* **Current State in Xavier**: [src/context/skill_registry.rs:716-745](../../../src/context/skill_registry.rs) implements `parse_frontmatter` using basic string slicing (`strip_prefix("name:")` and `strip_prefix("description:")`). For Hermes config, [src/context/skill_registry.rs:130](../../../src/context/skill_registry.rs) invokes `serde_yaml::from_str`.
* **Crate Research & Landscape**:
  * `serde_yaml`: Deprecated and archived since March 2024.
  * `serde_yml` (v0.0.13, [crates.io/crates/serde_yml](https://crates.io/crates/serde_yml)): Deprecated. Version 0.0.13 is a temporary compatibility shim rerouting to `noyalib` following RUSTSEC-2025-0068. **Do not use.**
  * `serde_norway` (v0.9.47, [crates.io/crates/serde_norway](https://crates.io/crates/serde_norway)): Community-maintained hard fork of `serde_yaml`. Drop-in replacement for Serde, but still retains legacy C-FFI / `unsafe-libyaml` heritage.
  * `saphyr` & `serde-saphyr` (v0.1.0 / v0.0.17, [crates.io/crates/serde-saphyr](https://crates.io/crates/serde-saphyr)): Modern pure-Rust YAML 1.2 parser and deserializer with zero `unsafe`, panic-free parsing, and execution budget limits to prevent denial-of-service on malicious input.
  * `gray_matter` (v0.3.2, [crates.io/crates/gray_matter](https://crates.io/crates/gray_matter)): Frontmatter extraction library, but introduces `yaml-rust2` ^0.10, adding an unnecessary parser engine.
* **Decision**: **Build small frontmatter splitter + adopt `serde-saphyr` (or `serde_norway`)**. Frontmatter boundaries are strictly isolated between initial `---` lines (< 40 lines of Rust). The extracted slice is deserialized with a maintained Serde engine, replacing `serde_yaml = "0.9.34"` across Xavier.

### 3.2 Symlink Placement & Atomic Writes
* **Directory Trees**: 7 tool targets (`$HOME/.claude/skills`, `$HOME/.config/opencode/skills`, `$HOME/.codex/skills`, `$HOME/.openclaw/skills`, `$HOME/.hermes/skills`, `$HOME/.gemini/config/skills`, `$HOME/.gemini/skills`) per [00-BRIEF.md:46-49](../../../docs/design/skill-controller/00-BRIEF.md).
* **Mechanism**:
  1. `tempfile::NamedTempFile::new_in(target_dir)` creates a temporary link placeholder.
  2. `std::os::unix::fs::symlink(canonical_path, temp_path)` creates the new pointer.
  3. `std::fs::rename(temp_path, final_dest)` atomically swaps the link into place.
* **Safety**:
  * Never `rm -rf` target directories.
  * Use `same-file = "1.0.6"` to check if `final_dest` already resolves to `canonical_path` before any modification.
  * If `final_dest` exists and is a regular non-symlink file, refuse to overwrite without an explicit user backup flag per [00-BRIEF.md:60-61](../../../docs/design/skill-controller/00-BRIEF.md).

### 3.3 Secret Detection in Skills
* **Audited Need**: 6 existing skills were discovered with hardcoded credentials ([00-BRIEF.md:52](../../../docs/design/skill-controller/00-BRIEF.md)).
* **Evaluation**:
  * Shelling out to `gitleaks` via `scripts/check-secrets.sh` fails in containerized/offline environments without `gitleaks` installed, and only operates on git repos.
  * Heavy crates like `detect-secrets` add large external rule files and C dependencies.
* **Decision**: **Build small native scanner in Xavier**.
  * Use `regex::RegexSet` ([Cargo.toml:338](../../../Cargo.toml)) and `aho-corasick` ([Cargo.toml:346](../../../Cargo.toml)).
  * Compile the exact high-signal patterns defined in [scripts/check-secrets.sh:23](../../../scripts/check-secrets.sh):
    * `sk-or-v1-[A-Za-z0-9]{20,}` (OpenRouter / OpenAI keys)
    * `ghp_[A-Za-z0-9]{20,}` (GitHub Personal Access Tokens)
    * `AKIA[0-9A-Z]{16}` (AWS Access Keys)
    * `xox[baprs]-[A-Za-z0-9-]{10,}` (Slack Tokens)
    * `AIza[0-9A-Za-z_-]{30,}` (Google Cloud / Gemini API Keys)
    * `BEGIN (RSA|OPENSSH|EC|DSA) PRIVATE KEY` (Private Keys)
    * `XAVIER_TOKEN=[a-zA-Z0-9]{8,}` (Xavier Auth Tokens)
  * Runs entirely in-memory during drift audits; flags exact line and byte offset without altering target skills.

### 3.4 Diff & Patch Generation for Drift Proposals
* **Requirement**: Drift proposals must generate reviewable unified diffs without auto-rewriting canonical skills ([00-BRIEF.md:68,78](../../../docs/design/skill-controller/00-BRIEF.md)).
* **Candidate Comparison**:
  * `diffy` (v0.5.2, [crates.io/crates/diffy](https://crates.io/crates/diffy)): Good patch generator, but adds a new crate to the workspace.
  * `similar` (v2.7.0, [crates.io/crates/similar](https://crates.io/crates/similar)): Standard diff engine in the Rust ecosystem (by Armin Ronacher). Already present in `Cargo.lock` via `mockito`.
* **Decision**: **Promote `similar` to `[dependencies]`**. Generating a standard unified diff:
  ```rust
  let diff = similar::TextDiff::from_lines(&original_skill, &repaired_skill);
  let patch = diff.unified_diff().header("a/SKILL.md", "b/SKILL.md").to_string();
  ```
  Provides human-readable proposals directly via CLI or REST/MCP response with zero new downloads.

---

## 4. Alignment with External Specs, Standards & Registries

| Specification / Tool | Standard Authority | Alignment Status | Actionable Policy in Xavier Controller |
|---|---|---|---|
| **Agent Skills Standard** | [agentskills.io](https://agentskills.io) / Anthropic open standard | **ADOPT (Mandatory Baseline)** | Validate `SKILL.md` structure strictly: YAML frontmatter with `name` (1–64 characters, `^[a-z0-9-]+$`, matches directory name), `description` (1–1024 characters), optional `compatibility`, `license`, `metadata`, `allowed-tools`. Validate body limits (< 500 lines, < 5000 tokens) and standard subdirectories (`scripts/`, `references/`, `assets/`). |
| **Skills CLI (`npx skills`)** | [skills.sh](https://skills.sh) / GitHub `agent-skills` community | **ALIGN (Interoperable File System Convention)** | Do NOT embed Node.js or run `npx`. Align with the `.agents/skills/` directory convention used by Skills CLI. Ensure skills placed or updated by `npx skills add` are indexed, audited, and projected by Xavier without conflicting formats or metadata corruption. |
| **Hermes Skill Registry** | SWAL / Hermes Ecosystem (`$HOME/.hermes/skills`) | **ADOPT (Native Ingestion)** | Maintain native support for Hermes canonical store and its `$HOME/.hermes/config.yaml` (`skills.disabled` list) as codified in [src/context/skill_registry.rs:100-140](../../../src/context/skill_registry.rs). |
| **Tool-Specific Directories** | Claude (`.claude/skills`), OpenCode (`.opencode/skills`), Codex (`.agents/skills`), Gemini (`.gemini/skills`) | **ADOPT (Projection Target Table)** | Declarative projection engine maps canonical skills to symlinks in these tool-specific paths per [00-BRIEF.md:31-37](../../../docs/design/skill-controller/00-BRIEF.md). |

---

## 5. Dependency Delta for `Cargo.toml`

When transitioning from the design phase to implementation, the changes to [Cargo.toml](../../../Cargo.toml) are minimal:

```toml
# 1. Promote from [dev-dependencies] (line 388) to [dependencies]
tempfile = "3.27.0"

# 2. Promote from Cargo.lock / code-graph (zero net download bloat)
globset = "0.4"
similar = { version = "2.7", features = ["unified-diff"] }

# 3. Replace deprecated serde_yaml (line 364)
# Preferred pure-Rust alternative:
serde-saphyr = "0.0.17"
# (or drop-in fork: serde_norway = "0.9")
```

All other requirements (TOML parsing, Markdown auditing, directory traversal, secret scanning, symlink manipulation) are satisfied by existing crates already active in Xavier's root `Cargo.toml`.
