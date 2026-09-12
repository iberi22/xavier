## 2026-09-01 - [Missing type attributes on buttons]
**Learning:** Several buttons throughout the application lack the explicit `type="button"` attribute. In React/HTML, a button without a type defaults to `type="submit"`, which can lead to unintended form submissions if these components are ever used within a form.
**Action:** Always explicitly specify `type="button"` on any interactive button component unless it is specifically intended to submit a form.

## 2024-05-18 - [LeaseHistoryPage Refresh Button UX/a11y Fix]
**Learning:** Adding accessible elements to icon-only buttons (`aria-label`, `type="button"`, `aria-hidden="true"` on the icon itself, and using `focus-visible` ring styles) is required to comply with Biome linters and general a11y practices in this app. Also, one must be very careful not to let automatic linters arbitrarily modify hooks like `useCallback` (e.g. changing the dependency array to include `apiClient.getLeaseHistory`), as this causes infinite loop bugs when the instance changes every render.
**Action:** Only make precise minimal changes for UX/a11y and strictly verify that no other logic or hooks have been tampered with or automatically reformatted.

## 2024-05-18 - [Fix a11y htmlFor and id on form elements]
**Learning:** When making accessibility improvements to form inputs (like connecting `label`s to `input`s via `htmlFor` and `id`), be cautious when running formatters/linters like Biome. Biome's unsafe fixes may alter unused variables in `catch` blocks or change button `type`s automatically, which might introduce regressions or conflict with existing logic. Auto-formatting can also touch completely unrelated files, so it's critical to scope fixes and revert unintended changes.
**Action:** Use targeted linting or skip unsafe fixes when verifying code. Explicitly review `git diff` to ensure auto-formatters did not modify unintended files before committing.
## 2024-05-17 - Added aria-pressed to ThemeToggle
**Learning:** React component toggle buttons representing state (like a theme toggle) often miss `aria-pressed`, which is important to announce to screen readers whether the toggle is currently active or not. The `ThemeToggle` component in this codebase lacked this attribute.
**Action:** Always check toggle buttons for an `aria-pressed` attribute when auditing for accessibility.
## 2026-05-18 - Studio Modal Borderless Elevation & Pill Navigation
**Learning:** Studio theme modal containers benefit from borderless backdrop-blur card surfaces (`bg-[#141518]/95 border border-white/[0.06] rounded-[24px]`) and pill-shaped active tab buttons (`bg-[#26272b] text-white/90`) to maintain neutral contrast and visual clarity across complex multi-tab dialogs.
**Action:** Use pill-styled active states and borderless subtle card borders when designing studio configuration modals.
