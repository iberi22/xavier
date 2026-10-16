#!/usr/bin/env bash
# Sourced by every segmented E2E wave; callers aggregate failures for the gate.

# Explicit zero-test allowlist. Empty by default: add an exact target name only
# with a comment explaining why executing no tests is intentional.
E2E_ZERO_TEST_ALLOWLIST=(
  # gated on the `local-gllm` feature, which the E2E waves do not build (needs a local model).
  quick_embed_test
  # whole file is behind cfg(feature = "ratatui") (cli-interactive), not part of ci-safe.
  tui_screenshot_e2e
  # its only test is #[ignore] (needs a local chat model); reports 0 passed, 1 ignored.
  e2e_chat_local
)

run_wave_target() {
  local target="$1" target_timeout="$2" features="$3"
  local log="${target}.log" allowed status

  # Keep the complete output: a trailing diagnostic must not hide a zero count.
  timeout -k 15 "$target_timeout" cargo test --package xavier --features "$features" \
    --test "$target" -- --test-threads=1 > "$log" 2>&1
  status=$?
  tail -20 "$log"
  if [ "$status" -ne 0 ]; then
    return "$status"
  fi

  if grep -Eq 'running 0 tests|test result:.* 0 passed;' "$log"; then
    for allowed in "${E2E_ZERO_TEST_ALLOWLIST[@]}"; do
      if [ "$target" = "$allowed" ]; then
        echo "Zero tests explicitly allowed: $target"
        return 0
      fi
    done
    echo "::error::$target executed zero passing tests (see $log)"
    return 1
  fi
  if ! grep -Eq 'test result:.* [1-9][0-9]* passed;' "$log"; then
    echo "::error::$target did not report a passing test count (see $log)"
    return 1
  fi
}
