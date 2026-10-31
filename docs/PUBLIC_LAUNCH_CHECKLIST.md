# Xavier Public Launch Checklist & Measurable Quality Gates

This checklist defines mandatory, measurable gates required for public release of Xavier v1.0.

---

## Pillar 1: Claims & Documentation Accuracy Gate

- [ ] **Zero Unsubstantiated Claims**: Public claims match technical implementation exactly:
  - Codebase Indexing is documented as **AST Symbol Reference Indexing** (Tree-sitter + SQLite FTS5 + Edge relationships), NOT vector-based GraphRAG.
  - Knowledge / Memory Graph is documented as **Belief Graph (`EntityGraph`)** for conceptual relationship mapping.
- [ ] **MCP Documentation**: Migration guide and tool catalog (`mem_search`, `mem_save`, `code_find`, `sys_health`) documented across `docs/site/src/content/docs/guides/mcp.md` and `docs/manual/MCP_INTEGRATION.md`.

---

## Pillar 2: Security & Authentication Gate

- [ ] **No Default Token Fallback**: Running `xavier http` without `XAVIER_TOKEN` fails fast unless explicitly enabled via `dev-mode`.
- [ ] **Prompt Guard Sanitization**: `SecurityService` passes 100% of prompt injection detection and sanitization unit tests.
- [ ] **Secret Redaction**: All sensitive structs (`Claims`, `LoginRequest`, tokens) implement custom `Debug` redaction to prevent secret leakage in logs.

---

## Pillar 3: Code Graph & Query Gate (`/code/find`)

- [ ] **Symbol Kind Filtering**: `/code/find` accurately filters by all `SymbolKind` variants (`Function`, `Struct`, `Enum`, `Class`, `Method`, `Variable`, `Constant`, `Route`, `Trait`, `Impl`, `Module`, `File`, etc.).
- [ ] **Multi-Parameter Coherence**: Queries combining `query`, `name`, `kind`, and `pattern` filter results deterministically without bypassing rules.
- [ ] **Response SLA**: Symbol query execution latency < 50ms for indexed repositories.

---

## Pillar 4: MCP Protocol & Stdio/SSE Gate

- [ ] **Stdio Interface**: `xavier mcp` handles standard JSON-RPC 2.0 requests over stdio cleanly.
- [ ] **SSE Interface**: Native MCP SSE endpoints (`/mcp/sse`) stream tools catalog and tool calls under authentication.
- [ ] **Schema Compliance**: Tool input schemas match specification for `mem_search`, `mem_save`, `code_find`, and `sys_health`.

---

## Pillar 5: Test Suite & CI/CD Gate

- [ ] **Rust Core Tests**: `cargo test --lib` passes with zero failures.
- [ ] **Clippy & Code Hygiene**: `cargo clippy --workspace --all-targets` reports 0 errors.
- [ ] **Frontend Panel UI**: `pnpm run test` in `panel-ui/` completes cleanly.
