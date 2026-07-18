# [Ola 5 · 06] code-graph: symbols FTS5 replace LIKE scan

> Implements #445

## Exact Technical Context
- `code-graph/src/db/mod.rs` `find_symbols` ~412-425 uses `WHERE name LIKE ?1`
- Add FTS5 virtual table symbols_fts; sync on insert; MATCH query with BM25 rank
- Fallback LIKE if query empty

## Acceptance Criteria
- [ ] FTS5 table created in init_schema
- [ ] insert path updates FTS5
- [ ] find_symbols uses MATCH when query non-empty
- [ ] Unit tests search by name
- [ ] cargo test -p code-graph (or workspace)
- [ ] ONLY code-graph/ crate
- [ ] NEVER xavier-core/

## Merge order
Independent — parallel with cost issues.
