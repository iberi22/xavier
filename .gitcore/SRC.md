# SRC.md - Source Code Reference

**Project:** Xavier
**Generated:** 2026-06-12
**Repository:** https://github.com/iberi22/xavier
**Location:** `.gitcore/` (per GitCore Protocol)

## Directory Structure

```
Xavier/
├── .gitcore/        # Agent documentation (per GitCore Protocol)
├── benches          # Rust benchmarks (Criterion)
├── benchmark-results # Storage for benchmark execution outputs
├── benchmarks       # Benchmark scripts and configurations
├── benchmark_test_results # Results from automated benchmark suites
├── bin              # Auxiliary binaries and compiled tools
├── code-graph       # AST/symbol indexing sidecar (Workspace member)
├── data             # Local databases and persistent state (Runtime)
├── docker           # Dockerfiles and compose configurations
├── docs             # Technical documentation, ADRs, and guides
├── panel-ui         # Frontend dashboard (React/Vite)
├── scripts          # Maintenance and automation scripts
├── skills           # Agent capabilities and skill modules
├── src              # Core Rust source code
├── target           # Rust build artifacts
├── target_bench     # Build artifacts for benchmarks
├── target_bench_run1 # Specific benchmark run artifacts
├── target_verify    # Verification run artifacts
├── tests            # Integration and E2E tests
├── web              # Web dashboard components
```

## Root Modules

### benches
- Status: Complete
- Purpose: Contains performance benchmarks for core components like embeddings and search.

## Auto-Indexing con CodeGraph

- **Status**: Activo (pre-push hook)
- **Purpose**: Xavier se indexa a sí mismo usando tree-sitter AST parsing.
  Escanea `src/`, `scripts/`, `code-graph/src/`, `config/`, `tests/`, `benches/`.
  Resultados: `code_graph.db` (runtime) + `.xavier/codegraph.json` (versionado).
- **Binary**: `target/release/code-graph` (17MB)
- **Script**: `scripts/codegraph-self-scan.sh` — corre en pre-push hook.
- **Commit Snapshot**: `.xavier/codegraph-<short-commit>.json` por cada push.

### bench-results

| Directory | Purpose |
|-----------|---------|
| benches | Rust benchmarks (Criterion)
- Status: Complete
- Purpose: Stores the output of benchmark runs for historical comparison.

### benchmarks
- Status: Complete
- Purpose: Scripts and data used for running systematic performance tests.

### bin
- Status: Complete
- Purpose: Repository for platform-specific binary tools used in the workflow.

### code-graph
- Status: Complete
- Purpose: A separate Rust crate that handles code analysis and symbol search.

### data
- Status: Complete
- Purpose: Default directory for runtime data storage, including SQLite databases.

### docker
- Status: Complete
- Purpose: Containerization assets for deploying Xavier in isolated environments.

### docs
- Status: Complete
- Purpose: Centralized documentation repository including architecture specs and user guides.

### panel-ui
- Status: Complete
- Purpose: The main user interface for managing and visualizing Xavier's memory.

### scripts
- Status: Complete
- Purpose: Utility scripts for installation, workflow automation, and environment setup.

### skills
- Status: Complete
- Purpose: Definitions for agent-specific skills and tool configurations.

### src
- Status: Complete
- Purpose: The main Xavier engine, implementing the Cognitive Memory System.

### tests
- Status: Complete
- Purpose: Integration tests that verify the system as a whole.

### web
- Status: Complete
- Purpose: Web-based components and alternative frontend implementations.


## Core Submodules (src/)

### src/a2a
- Purpose: Agent-to-Agent communication protocols.

### src/adapters
- Purpose: Hexagonal architecture adapters (Inbound/Outbound).

### src/agents
- Purpose: Cognitive layers (System 1, 2, 3) and agent runtime.

### src/api
- Purpose: Internal API definitions for skills, search, and graph.

### src/app
- Purpose: Application-level use cases and services (Memory, Proxy, Security).

### src/billing
- Purpose: Usage tracking and billing integrations (Stripe).

### src/checkpoint
- Purpose: Session and state checkpointing mechanisms.

### src/chronicle
- Purpose: Automated documentation harvesting and generation.

### src/cli
- Purpose: Command-line interface implementation and handlers.

### src/codebase
- Purpose: Local codebase analysis and connection management.

### src/consistency
- Purpose: Memory regularization and consistency checks.

### src/consolidation
- Purpose: Memory merger and reflection logic.

### src/context
- Purpose: Context management, skill dispatching, and orchestrator.

### src/coordination
- Purpose: Event bus and agent registry coordination.

### src/crypto
- Purpose: Cryptographic primitives for keys and encryption.

### src/data_commons
- Purpose: Governance, reputation, and tokenization of shared data.

### src/domain
- Purpose: Domain models for agents, belief, memory, and security.

### src/embedding
- Purpose: Vector embedding providers (OpenAI, GLLM) and caching.

### src/enterprise
- Purpose: Enterprise-grade features (RBAC, Tenancy, Audit).

### src/memory
- Purpose: Hierarchical memory storage (QMD, Belief Graph, SQLite).

### src/mesh
- Purpose: P2P synchronization and mesh networking.

### src/observability
- Purpose: Monitoring, logging, and system health detection.

### src/retrieval
- Purpose: Advanced retrieval strategies (Gating, Policy, Scoring).

### src/scheduler
- Purpose: Background job scheduling and daemon management.

### src/search
- Purpose: Hybrid search (BM25 + Vector) and RRF merging.

### src/secrets
- Purpose: Secure secret storage (Vault, Local, Lending).

### src/security
- Purpose: Threat detection, prompt guards, and auth.

### src/server
- Purpose: HTTP, MCP, and Headless server implementations.

### src/session
- Purpose: User session management and persistence.

### src/settings
- Purpose: System configuration, defaults, and serialization.

### src/sync
- Purpose: Data synchronization protocol (Manifests, Chunks).

### src/tasks
- Purpose: Background tasks for sync and maintenance.

### src/telegram
- Purpose: Telegram bot integration for notifications and commands.

### src/tools
- Purpose: Internal tools (Kanban, GitCore, Validation).

### src/ui
- Purpose: CLI-based TUI dashboard components.

### src/utils
- Purpose: Common utility functions (HTTP, Crypto, Files).

### src/verification
- Purpose: Automated verification cycles for system integrity.

### src/workspace
- Purpose: Multi-tenant workspace registry and isolation.


## Build Commands

```bash
# Install all dependencies (Monorepo)
npm run install:all

# Build Panel UI
npm run build:panel

# Build Documentation Site
npm run build:docs

# Build Rust binary
cargo build --release

# Run all tests
cargo test
```

## Entry Points

- `src/main.rs`: Primary CLI and Server entry point.
- `src/main_tui.rs`: Interactive TUI dashboard entry point.
- `src/bin/cortex.rs`: Specialized cognitive entry point for advanced reasoning.

---

## Cross-References (GitCore Protocol)

| Referencia | Ruta | Propósito |
|---|---|---|
| Planning | `.gitcore/planning/PLANNING.md` | Visión, fases, prioridades Q3 2026 |
| Active Tasks | `.gitcore/planning/TASK.md` | Progreso por componente, tareas activas, deuda técnica |
| Feature Tracking | `.gitcore/features.json` | 20 features con estado, tests, issues vinculados |
| Feature Details | `.gitcore/features/` | Documentación detallada por feature |
| Architecture | `ARCHITECTURE.md` | Decisiones arquitectónicas no-negociables |
| Config Reference | `SRC_CONFIG.md` | Variables de entorno y configuración detallada |
| Agent Rules | `../../AGENTS.md` | Identidad Xavier, subagentes, protocolo MCP |
| Coding Rules | `../../RULES.md` | Reglas de codificación Rust, agentes, documentación |
| DevLog | `docs/devlog/` | Bitácora técnica semanal |
| API Docs | `docs/API.md` | Referencia de endpoints HTTP |
| MCP Contract | `docs/MCP_CONTRACT.md` | Contrato de 12 tools MCP |
| Deployment | `docs/DEPLOYMENT.md` | Guía de deploy Docker/ producción |
| State | `STATE.md` | Estado global del proyecto |

> **Convención:** Todos los documentos de planificación y features siguen el GitCore Protocol v3.
> Los agentes deben leer AGENTS.md, SOUL.md, USER.md al inicio de cada sesión.

*Auto-generated by GitCore Auto-Maintainer*
* All docs stored in .gitcore/ per GitCore Protocol v3*

---

## 🔗 Cross-references

| Documento | Ruta | Propósito |
|-----------|------|-----------|
| Planning | `.gitcore/planning/PLANNING.md` | Visión, fases, prioridades Q3 2026 |
| Tareas activas | `.gitcore/planning/TASK.md` | Progreso por componente, tareas activas |
| Feature registry | `.gitcore/features.json` | 20 features con estado y tests |
| Feature details | `.gitcore/features/` | Detalle por feature (FEATURE-*.md) |
| Arquitectura | `.gitcore/ARCHITECTURE.md` | Decisiones no-negociables |
| SRS (requisitos) | `docs/SRS/index.md` | Software Requirements Specification |
| Instrucciones agentes | `AGENTS.md` | ORDEN DE LECTURA, subagent protocol |
| Reglas de código | `RULES.md` | Convenciones Rust, R-DOC |
| DevLog | `docs/devlog/` | Bitácora técnica |
| Memoria persistente | Xavier REST API (http://localhost:8006) | Decisiones, sesiones, contexto |
