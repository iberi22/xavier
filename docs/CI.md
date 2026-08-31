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
