# CI Execution & Test Isolation Guidelines

## Test Isolation Guard (`--test-threads=1`)

To prevent environment variable leaks and flaky test behavior across unit and integration test suites, tests that modify global environment variables (`XAVIER_*`, `OPENAI_API_KEY`, etc.) must be executed in sequence and protected with environment guards.

### Requirements

1. **Single-threaded Execution Guard:**
   CI runners and local verification commands must execute lib tests with single-thread constraints:
   ```bash
   cargo test --lib -- --test-threads=1
   ```
   This is enforced by default in `.cargo/config.toml`:
   ```toml
   [env]
   RUST_TEST_THREADS = "1"
   ```

2. **Serial Execution Annotation (`#[serial]`):**
   Any test function modifying environment variables (`std::env::set_var` / `std::env::remove_var`) must be annotated with `#[serial]` from `serial_test`:
   ```rust
   use serial_test::serial;

   #[tokio::test]
   #[serial]
   async fn test_env_mutating_behavior() {
       // ...
   }
   ```

3. **`EnvGuard` Restoration Pattern:**
   Use `EnvGuard` or explicit lock guards to capture baseline environment variable values upon entry and automatically restore them upon `Drop`.

## Integration job: Observability Contract

`.github/workflows/ci.yml` runs a dedicated job, **Rust Integration (Observability Contract)**, scoped to one target (not every integration binary):

```bash
cargo test --package xavier --features ci-safe --test observability_contract -- --test-threads=1
```

`tests/observability_contract.rs` boots a real `xavier http` and asserts that `GET /health`, MCP `health_check` and the stats surface agree (WAVE-29.10, #2563). The spawned server must stay hermetic: `XAVIER_HOME`, `XAVIER_STATE_DIR`, `XAVIER_DATA_DIR` and the working directory all point inside a tempdir, so the test never reads or writes the developer's `~/.xavier` (including the encrypted `auth.db`). Every field read asserts presence and type, so a missing field fails loudly.

## Verifying across parallel worktrees

Worktrees that share one `CARGO_TARGET_DIR` all produce the same `target/debug/deps/xavier-<hash>` test binary. If cargo decides nothing changed, the binary you run may come from another worktree. Before a verification run, `touch src/lib.rs`, check that the build log says `Compiling xavier ... (<this worktree>)`, and confirm the new test names appear in the test output. The pre-commit hook lints with **default features** (no `ci-safe`), so run clippy both ways.
