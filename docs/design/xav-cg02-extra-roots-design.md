# xav-cg.02 — `XAVIER_CODE_EXTRA_ROOTS`: design

Status: design only, no code yet. Recovered from the plan that was lost outside the repo
(`proyectosSWAL/docs/private/xavier-plan-jules-2026-10.md`, section F2).

## 1. The problem, in terms of the real code

Today `xavier code scan <path>` resolves exactly one repository and anchors its graph at
`<root>/.xavier/code_graph.db`:

- `code_scan_handler` (`src/cli/handlers/code.rs:669`) calls `resolve_target_path(payload.path)`.
- `resolve_requested_repo(project_id, root)` (`src/cli/handlers/code.rs:108`) canonicalises the
  path through `find_repo_root` and stores `code_graph_db_path_for_root(&canonical)`.
- `code_graph_db_path_for_root` (`src/codebase/repo_identity.rs:215`) returns
  `<root>/.xavier/code_graph.db`.

So an operator with worktrees outside the workspace — the normal case in this ecosystem, where
every agent lives in `~/wt/<something>` — has no way to put them in the graph. They must run
`code scan` once per worktree and `find` can only ever answer for whichever root it is given.
An agent asked "who calls `require_permission`?" across the whole estate gets an answer scoped
to one worktree, or an empty result, and cannot tell those apart from "not indexed".

## 2. Design

### The variable

`XAVIER_CODE_EXTRA_ROOTS` — an OS path-list, same convention as `PATH`, because the values are
filesystem roots and every operator already has `PATH`-muscle memory. Separator: `:` on Unix,
`;` on Windows (`std::env::split_paths` handles both, which is the whole reason to use it).

```rust
/// Extra repository roots to keep in the code graph alongside the cwd.
///
/// OS path-list (`:` / `;`), same convention as `PATH`; `std::env::split_paths`
/// parses it. Every entry is resolved through [`find_repo_root`] and
/// [`code_graph_db_path_for_root`], so an extra root obeys exactly the same
/// per-repo anchoring and isolation as the workspace root: it can never answer
/// with another repo's graph.
///
/// Duplicate roots (including one equal to the cwd) collapse to a single entry.
/// Entries that do not exist are dropped, not invented.
pub fn extra_code_roots() -> Vec<PathBuf>;
```

### Where it is read, and why there

In `repo_identity.rs`, next to `code_graph_db_path_for_root`, NOT in `codegraph_paths.rs` and
NOT in the HTTP handler.

- `codegraph_paths.rs` resolves a *given* workspace to a path. It has no notion of a set.
- The handler must stay a thin adapter: `resolve_requested_repo` already owns "which repo is
  this request about". Adding env-driven root discovery there would let a request silently
  answer from a different repo — exactly the XAV-01 cross-repo leak that `find_repo_root` was
  hardened against (see its doc comment: a missing path once inherited `~/.hermes`' identity).
- `repo_identity.rs` is where root→identity already lives, so the new roots get `project_id`,
  sanitisation and anchoring for free, from one place.

### Rules, each justified by existing code

| Case | Behaviour | Why |
|---|---|---|
| Relative entry | resolved against cwd, then `find_repo_root` | a path is meaningless without an anchor; `find_repo_root` already absolutises (`std::path::absolute`, line 97) |
| Non-existent entry | dropped with a `warn!` | `find_repo_root` anchors a missing path at itself to avoid borrowing an unrelated identity. For an *extra* root that means a graph that can never be filled, so dropping it is cleaner than indexing a phantom |
| Entry equal to the cwd root | collapsed to one entry | the cwd root is already anchored by `resolve_requested_repo`; two entries would mean two pools and two `project_id`s for one repo |
| Two entries resolving to the same root | collapsed | same reason, after `canonicalize` |
| Empty entry (`"a::b"`) | ignored | `split_paths` yields empty components; there is no root there |
| Entry outside any git repo | kept, anchored at itself | same rule as the workspace: it degrades for itself instead of leaking another repo's graph |
| Env var unset/blank | empty vec | the current behaviour, unchanged |

### Identity and cap

Each extra root is a normal repo identity: `derive_project_id(&canonical)` and
`code_graph_db_path_for_root(&canonical)`. Two extra roots that are worktrees of the SAME repo
share a `project_id` by construction — that is existing behaviour, not something this change
introduces.

**Cap: 8 extra roots.** `ConnectionManager::MAX_POOLS` is 16 with LRU eviction, and the
workspace root already holds one. Sixteen live pools is the hard ceiling; letting extras
evict the workspace's own pool would make `find` intermittently empty for the repo the operator
is standing in, which is the bug class this whole plan exists to kill. 8 leaves headroom and is
configurable downward by not listing more.

## 3. Files to change

| File | Change |
|---|---|
| `src/codebase/repo_identity.rs` | add `extra_code_roots()`; add `pub fn all_code_roots(workspace: &Path) -> Vec<PathBuf>` that returns the workspace root first, then the extras, deduped and capped |
| `src/codebase/mod.rs` | re-export if the crate re-exports its fns by name (check the existing pattern) |
| `src/cli/handlers/code.rs` | `resolve_requested_repo` stays per-repo. A `code/stats` or new `code/roots` surface may call `all_code_roots`. Do NOT change `resolve_requested_repo`'s contract |

Untouched on purpose: `find_repo_root`, `code_graph_db_path_for_root`, `derive_project_id`, and
everything PR #2824 established about per-repo anchoring. If this feature needs to change any of
those, the design is wrong.

## 4. Tests

In `tests/codegraph_scan_then_query.rs` (the file PR #2824 added), which already carries
`xavier::isolate_test_process!()`:

1. `extra_roots_default_to_empty_when_unset` — no env var ⇒ empty vec. Proves no behaviour
   change for everyone who does not set it.
2. `extra_roots_are_split_and_resolved` — `"/a:/b"` ⇒ two absolute paths.
3. `extra_root_equal_to_cwd_is_deduplicated` — listing the cwd root yields one entry, not two.
4. `extra_roots_collapse_duplicate_real_paths` — `/a` and `/a/.` produce one entry after
   canonicalisation.
5. `nonexistent_extra_root_is_dropped` — the entry does not appear.
6. `empty_components_are_ignored` — `"/a::/b"` yields two roots, not three.
7. `all_code_roots_respects_the_cap` — with 20 entries set, the vec is the workspace root plus
   at most 8.
8. `extra_root_gets_its_own_graph_db` — two temp git repos listed as extras produce two
   distinct `code_graph.db` paths under their own roots. This is the one that would catch a
   regression of the XAV-01 isolation.

Each must fail without the fix, which is mechanical: the fn does not exist, so they cannot
compile; for the behaviour rules, break one rule at a time and confirm the specific test fails.

## 5. Risks

| Risk | Severity | Mitigation |
|---|---|---|
| Extra roots evict the workspace pool (`MAX_POOLS = 16`, LRU) and make `find` intermittently empty for the current repo | **HIGH** | the cap of 8; the test that asserts it |
| An extra root outside any git repo borrows a real repo's identity — the XAV-01 cross-repo leak | **HIGH** | route every entry through the hardened `find_repo_root`, which anchors missing paths at themselves; `extra_root_gets_its_own_graph_db` |
| Env var set globally by accident in a shell profile, so every `find` widens scope silently | MEDIUM | `all_code_roots` is opt-in per process; `code/stats` should report how many roots are in play, so a surprise is visible rather than silent |
| Windows `;` separator vs Unix `:` | LOW | `std::env::split_paths` handles both; no hand-rolled parsing |
| Reading the env var on every request | LOW | cheap; but resolve once per command, not per symbol lookup, to avoid re-parsing inside a query loop |