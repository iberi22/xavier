# Dogfood O1-O7 Memory Fallback and Limitations Log

This document serves as the durable engineering follow-up record for the O1-O7 dogfood review conducted on 2026-09-18 against the real `code-graph/src` codebase (34 files, symbols 1917 → 2293 in a temporal index). It captures the offline memory replay payload, details the TDD fixes committed in baseline `a5fd53ec`, maps open limitations to Wave 24 issues, and documents test suite count corrections.

## Memory payload (Xavier POST pending)

Due to endpoint unavailability on port `:8006` (`:8006` down on 2026-09-18/19), the dogfood memory payload was unable to land directly in Xavier's vector store. The full structured payload below is stored here for replay by the orchestrator via `POST /memory/add` under namespace `project: xavier`.

```json
{
  "namespace": "project: xavier",
  "content": "Dogfood review 2026-09-18 for code-graph O1-O7 against real code-graph/src (34 files, 1917 -> 2293 symbols temporal index). Key facts: Tamil-scale Rust parser fix extracted 373 live imports into ImportMap (previously 0 due to unhandled use_declaration/scoped_use_list AST nodes). Query cycle detection capped at MAX_CYCLES = 512 shortest-first (live graph reduced from 219,201 cycles / 11s wall-clock time to 512 cycles / 4s wall-clock time). Test gate obligation returned false on O symbols. Route queries confirmed confidence margins and honest refusal for unknown symbols. ResponseMeta declared est_tokens across query responses. Code-graph crate unit suite: 135 passed (lib target) / pre-wave baseline 132 passed, 3 failed plugin integration tests. Feature ledger updated 67 items across O2-O7. Working tree clean with Cargo.lock reverted and test examples deleted.",
  "metadata": {
    "source": "dogfood-o1-o7-review",
    "timestamp": "2026-09-18T00:00:00Z",
    "symbols_count": 2293,
    "imports_extracted": 373,
    "max_cycles_cap": 512,
    "wall_clock_seconds": 4,
    "features_updated": "O1-O7"
  }
}
```

To replay this memory once the service on `:8006` is operational:

```bash
curl -X POST http://localhost:8006/memory/add \
  -H "Content-Type: application/json" \
  -d '{
    "namespace": "project: xavier",
    "content": "Dogfood review 2026-09-18 for code-graph O1-O7 against real code-graph/src: 373 imports extracted into ImportMap, MAX_CYCLES=512 shortest-first reduced cycle runtime from 11s (219,201 cycles) to 4s (512 cycles), ResponseMeta declares est_tokens, test-gate obligation=false on O symbols, 135 passed in lib suite, ledger 67 O2/O7 updated."
  }'
```

## Fixes landed in a5fd53ec

The dogfooding session uncovered three critical runtime bugs in `code-graph`, which were immediately resolved via TDD fixes in baseline commit `a5fd53ec`:

1. **Rust AST Import Extraction (`use_declaration` and `scoped_use_list`)**
   - **File & Line Pointers:** `code-graph/src/parser/rust.rs` lines 192, 250, 262, and test at line 562 (`extracts_use_declarations_as_imports`).
   - **Finding:** The Rust parser was ignoring `use_declaration` and nested `scoped_use_list` tree-sitter nodes, resulting in 0 extracted imports across the entire codebase.
   - **Fix:** Implemented AST traversal for `use_declaration` and `scoped_use_list` to populate `ImportMap`.
   - **Verification:** Live import count jumped from `0` to `373` extracted imports.

2. **Call Cycle Explosion Capping (`MAX_CYCLES`)**
   - **File & Line Pointers:** `code-graph/src/query/mod.rs` lines 268 (`pub const MAX_CYCLES: usize = 512`), 923, 932, and 941.
   - **Finding:** Cycle detection on dense call graphs traversed all permutations without bounds, generating 219,201 cycles and taking 11 seconds wall-clock time.
   - **Fix:** Implemented shortest-first cycle collection bounded strictly by `MAX_CYCLES = 512`.
   - **Verification:** Reduced cycle output from `219,201` to `512` cycles and wall-clock execution time from `11s` to `4s`.

3. **Per-Language Method-Noise Filtering**
   - **File & Line Pointers:** `code-graph/src/language/mod.rs` (and `code-graph/src/parser/rust.rs`).
   - **Finding:** High-frequency method names (e.g. `clone`, `fmt`, `default`, `new`) created noise in central symbol hubs.
   - **Fix:** Introduced language-specific method-noise exclusion lists in global hub calculations.
   - **Verification:** Hub rankings now reflect core domain types and functions rather than utility trait implementations.

Verification summary from repository checks:
- `git show a5fd53ec --stat`: 2,331 files changed, confirming baseline integration.
- `grep -n "MAX_CYCLES" code-graph/src/query/mod.rs`: `pub const MAX_CYCLES: usize = 512;`
- `grep -n "use_declaration\|scoped_use_list" code-graph/src/parser/rust.rs`: Matches at lines 192, 250, 262, and 562.

## Honest limitations (still open → wave mapping)

The dogfooding review identified several technical limitations in the O1-O7 implementation. These open items have been systematically mapped to dedicated Wave 24 issues for resolution:

1. **Symbol Extraction Collisions (`extract×5`) → `WAVE-24.07`**
   - *Limitation:* Overlapping symbol extraction rules cause repeated symbol definitions when parsing composite types and macros.
   - *Target:* Wave 24 issue `WAVE-24.07`.

2. **File-Level Cycle Resolution → `WAVE-24.08`**
   - *Limitation:* Cycle detection currently operates at symbol call granularity; file-level module dependency cycles are not resolved.
   - *Target:* Wave 24 issue `WAVE-24.08`.

3. **Edit-Check Contract Verification CLI Only → `WAVE-24.09`**
   - *Limitation:* The `edit-check` contract breaking arity detector is currently accessible only via CLI tools, lacking IDE/LSP hooks.
   - *Target:* Wave 24 issue `WAVE-24.09`.

4. **Test-Gate `sh/` Blindness and Execution Overhead → `WAVE-24.06`**
   - *Limitation:* Test-gate analysis cannot map shell script execution (`.sh`) to tested Rust components, and incurs performance overhead on large test passes.
   - *Target:* Wave 24 issue `WAVE-24.06`.

5. **Plugin Harness Failures → `WAVE-24.01`**
   - *Limitation:* Three external plugin integration tests fail when external binaries or mocks are unconfigured.
   - *Target:* Wave 24 issue `WAVE-24.01`.

6. **Gating, Belief, MCP, and CLI Integration Gaps → `WAVE-24.02`, `WAVE-24.03`, `WAVE-24.04`, `WAVE-24.05`**
   - *Limitation:* RRF gating weights (`WAVE-24.02`), belief propagation (`WAVE-24.03`), MCP tool core integration (`WAVE-24.04`), and CLI subcommand ergonomics (`WAVE-24.05`) require dedicated refinement.
   - *Targets:* `WAVE-24.02`, `WAVE-24.03`, `WAVE-24.04`, `WAVE-24.05`.

7. **Dogfood Follow-up & Memory Replay Documentation → `WAVE-24.10`**
   - *Limitation:* Lack of durable record for dogfood findings and fallback memory payload.
   - *Target:* Wave 24 issue `WAVE-24.10` (this document).

## Count correction

The session summary initially reported "5 preexisting plugin failures". Measured verification against the codebase clarifies and supersedes this count:

1. **Library Target Unit Suite (`cargo test -p code-graph --lib`):**
   - Output: `test result: ok. 135 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 6.99s`
   - All 135 unit tests in the library crate pass cleanly with zero failures.

2. **Integration Test Suite (`cargo test -p code-graph`):**
   - When running the full suite without external plugin environment variables, exactly 3 tests fail in external plugin harness fixtures (addressed in `WAVE-24.01`):
     - `test_plugin_parser_mock_process_protocol`
     - `test_plugin_parser_error_handling`
     - `test_codegraph_plugin_auto_detection_and_execution`
   - Pre-wave baseline count: 132 passed / 3 failed (total 135 tests).

Therefore, the measured count of **135 passed** (in `--lib`) or **132 passed / 3 failed** (in full integration runs) supersedes the session summary error citing 5 failures.
