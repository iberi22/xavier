# B2: Fix snippet prefix assertion in v1_api tests

## Problem

`src/server/v1_api.rs:1732` asserts `long_content.starts_with(snippet)` — this
encodes the assumption that snippets are always prefixes of content. When the
snippet logic was changed to center on query terms (Ola9), this test became
a blocker for further snippet improvements.

## Solution

Rewrite the test to verify that the snippet **contains query terms**, not that
it's a prefix of the content.

### Steps

1. Read the test at `v1_api.rs:1732` to understand the assertion
2. Replace `starts_with` with a check that snippet contains relevant keywords
3. Add a second assertion: snippet length ≤ 200 chars (hard cap from Ola9)
4. Verify: `cargo test -p xavier --lib test_v1_memories_search` passes

## Acceptance

- [ ] Test no longer asserts prefix semantics
- [ ] Test verifies snippet contains query-relevant terms
- [ ] Test verifies snippet respects 200-char hard cap
- [ ] All v1_api tests pass

## Files

- `src/server/v1_api.rs` (test only)
