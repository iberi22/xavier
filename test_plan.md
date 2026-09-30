1. **Identify the opportunity:**
   - In `panel-ui/src/components/QuotaTable.tsx`, the `QuotaTable` component renders a list of `Quota` items using `.map()`.
   - The list items `<tr>` and its contents are re-created on every render of `QuotaTable`, even if the items haven't changed.
   - If the parent component (`ProvidersPage`) re-renders, it recreates `mappedQuotas` (via `useMemo` when `quotas` changes), and passes it to `QuotaTable`. `QuotaTable` maps over the whole list again, creating new DOM elements.
   - By extracting the row rendering into a memoized component `QuotaTableRow`, we can prevent unnecessary reconciliation and re-rendering of individual row items if their data hasn't changed.

2. **Refactor `QuotaTable.tsx`:**
   - Extract the `<tr>` block inside the `quotas.map(...)` into a new component `QuotaTableRow`.
   - Wrap `QuotaTableRow` with `React.memo()`.
   - Add the Bolt Performance Optimization comment block above the new component to explain the optimization.
   - Update `QuotaTable` to map over `quotas` and render `QuotaTableRow` for each item.

3. **Verify:**
   - Run `cd panel-ui && pnpm run biome:check`
   - Run `pnpm test` in the `panel-ui` directory.
   - Ensure the application still works correctly.

4. **Complete pre-commit steps:**
   - Call `pre_commit_instructions` and follow testing and verification steps.

5. **Submit:**
   - Commit and submit the code with an appropriate message and description.
