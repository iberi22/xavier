# Contributing to Xavier

Thank you for your interest in contributing to Xavier! This document provides guidelines and instructions for contributing to the cognitive memory runtime for AI agents.

## Development Setup

### Prerequisites

- **Rust 1.75+** (with `cargo` and `rustup`)
- **SQLite** (bundled via `rusqlite`, no system install needed)
- **OpenSSL** development headers (for `git2` vendored-openssl)

### Clone and Build

```bash
git clone https://github.com/iberi22/xavier.git
cd xavier
cargo build
```

### Building with Enterprise Features

```bash
cargo build --features enterprise
```

### Running Tests

```bash
# All unit tests
cargo test --lib

# All tests (including integration tests)
cargo test

# Specific test
cargo test test_name

# With enterprise features
cargo test --features enterprise
```

### Environment Variables

Copy `.env.example` to `.env` and configure:

```bash
cp .env.example .env
# Edit .env with your configuration
```

Key environment variables:

| Variable           | Description                        | Default            |
| ------------------ | ---------------------------------- | ------------------ |
| `XAVIER_HOST`      | HTTP server host                   | `127.0.0.1`       |
| `XAVIER_PORT`      | HTTP server port                   | `8003`             |
| `XAVIER_DATA_DIR`  | Data directory for memory storage  | `~/.xavier`        |
| `RUST_LOG`         | Log level (tracing)                | `info`             |

## Code Style

### Formatting

We use `cargo fmt`. Run before committing:

```bash
cargo fmt --all
```

### Linting

We use `cargo clippy` with deny warnings. CI will fail on warnings:

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

Or for a focused check:

```bash
cargo clippy --all-targets --features enterprise -- -D warnings
```

### Pre-commit Checks

Run these before every commit to ensure CI passes:

```bash
cargo fmt --all && cargo clippy --all-targets --features enterprise -- -D warnings && cargo test --lib
```

## Commit Message Convention

We follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>

[optional body]

[optional footer]
```

**Types:**

| Type       | Description                         |
| ---------- | ----------------------------------- |
| `feat`     | New feature                         |
| `fix`      | Bug fix                             |
| `docs`     | Documentation only                  |
| `chore`    | Maintenance, deps, build changes    |
| `refactor` | Code refactoring (no behavior change) |
| `perf`     | Performance improvements            |
| `test`     | Adding or updating tests            |
| `security` | Security fixes or hardening         |

**Examples:**

```
feat(mcp): add MemoryFragment tool for Gestalt integration
fix(api): correct list_memories timeout handling
docs(adr): document multi-crate workspace evolution strategy
security(http): fix SSRF risk in verify_save_handler
chore(deps): update reqwest to 0.13
```

## Branch Naming

```
<type>/<short-description>
```

Examples: `feat/gestalt-mcp`, `fix/list-memories-timeout`, `docs/readme-update`, `security/ssrf-hardening`

## Pull Request Process

1. **Fork** the repository and create a branch from `main`
2. **Develop** your feature or fix (follow the style guide above)
3. **Test** — ensure all tests pass (`cargo test --lib`)
4. **Lint** — ensure `cargo fmt` and `cargo clippy` are clean
5. **Commit** — use conventional commit messages
6. **Push** — push your branch to origin
7. **Open PR** — fill out the PR template completely
8. **Review** — await code review; address feedback promptly

### PR Requirements

- [ ] `cargo fmt` passes
- [ ] `cargo clippy` passes with no warnings
- [ ] All tests pass (`cargo test --lib`)
- [ ] New features include tests
- [ ] Documentation updated if needed
- [ ] CHANGELOG.md entry added (if applicable)
- [ ] No secrets or credentials committed

## Project Architecture

Xavier uses a **hexagonal architecture** (ports & adapters):

- **`src/lib.rs`** — Library root, core types, and trait definitions
- **`src/main.rs`** — CLI binary entrypoint
- **`src/main_tui.rs`** — TUI dashboard (optional, requires `cli-interactive`)
- **`src/main_egui.rs`** — Native GUI (optional, requires `egui-standalone`)
- **`code-graph/`** — Static analysis crate for dependency graph extraction
- **Ports** — Traits defining boundary interfaces (e.g., `MemoryQueryPort`, `AgentLifecyclePort`)
- **Adapters** — Implementations of ports (e.g., SQLite-Vec backend, MCP protocol)

### Core Stack

- **Storage**: SQLite via `rusqlite` with `sqlite-vec` for vector embeddings
- **WebSocket**: `axum` for real-time event streaming
- **CLI**: `clap` with `pico-args` for argument parsing
- **Embeddings**: Optional `gllm` + `candle-core` for local LLM inference
- **Security**: `aes-gcm` for E2E encryption, `argon2` for key derivation, `hmac` for webhook verification

## Getting Help

- **Issues** — open at https://github.com/iberi22/xavier/issues
- **Discussions** — use GitHub Discussions
- **Enterprise support** — contact enterprise@southwest-ai-labs.com
- **Sponsors** — for dedicated support, reach out via GitHub Sponsors

## License

By contributing to Xavier, you agree that your contributions will be licensed under the MIT License. Enterprise features are subject to a separate Enterprise License.
