1. **Phase 1: Types & Contracts (`src/domain/cycle_breaks/w30_11.rs`)**
   - Move the following from `src/memory/schema.rs` to `w30_11.rs`:
     - `MemoryKind` (and its `impl`)
     - `EvidenceKind` (and its `impl`)
     - `RetrievalScope` (and its `impl`)
     - `MemoryQueryFilters`
   - Add necessary imports in `w30_11.rs` (e.g., `serde`, `anyhow`, `xavier_core_logic::MemoryLevel`).
   - Move the following from `src/retrieval/config.rs` to `w30_11.rs`:
     - `DEFAULT_RRF_K`
     - `DEFAULT_KEYWORD_WEIGHT`
     - `DEFAULT_VECTOR_WEIGHT`
     - `configured_rrf_k`
     - `configured_keyword_weight`
     - `configured_vector_weight`
   - Add necessary imports for config (e.g., `crate::settings::XavierSettings`).

2. **Phase 2: Core Logic (Rewire imports)**
   - **`src/memory/schema.rs`**: Replace the moved types with a `pub use` statement pointing to `w30_11.rs`.
   - **`src/retrieval/config.rs`**: Replace the moved constants/functions with a `pub use` statement pointing to `w30_11.rs`.
   - **`src/search/hooks.rs`**: Change `use crate::memory::schema::MemoryQueryFilters;` to `use crate::domain::cycle_breaks::w30_11::MemoryQueryFilters;`.
   - **`src/search/rerank.rs`**: Change `use crate::memory::schema::MemoryQueryFilters;` to `use crate::domain::cycle_breaks::w30_11::MemoryQueryFilters;`.
   - **`src/search/hybrid.rs`**:
     - Change usage of `crate::retrieval::config::...` to `crate::domain::cycle_breaks::w30_11::...`.
     - Also ensure `MemoryQueryFilters` is imported from `w30_11.rs` (or just leave it if it's not being checked, but it's cleaner to change it).
   - **`src/context/hybrid.rs`**: Change `use crate::retrieval::config;` to import `DEFAULT_RRF_K` from `crate::domain::cycle_breaks::w30_11::DEFAULT_RRF_K` (or just `use crate::domain::cycle_breaks::w30_11 as config;`).

3. **Phase 3: Pre-commit & Verification**
   - Use `pre_commit_instructions` tool to run the required pre-commit steps.
   - Run acceptance criteria grep commands manually to ensure cycle dependencies are cut.

4. **Phase 4: Submit**
   - Submit the PR with the issue's instructions (`[WAVE-30.11] feat-search-retrieval-decoupling`).
