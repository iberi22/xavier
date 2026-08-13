## 2026-07-20 - Accessible Form Controls with Neon Focus
**Learning:** Icon-only controls and form inputs require explicit aria-labels and htmlFor bindings, paired with focus-visible styles that match the app's neon accent (`focus-visible:ring-[#39ff14]/50`) for keyboard accessibility.
**Action:** Ensure all future custom form controls map standard ARIA attributes while incorporating the design system's specific focus rings.
## 2026-07-28 - Memory Browser Accessibility Labels
**Learning:** React fragments and generic HTML interactive elements (like icon-only `<button>`, `<input>`, and `<select>`) in Xavier's UI components often lack screen-reader accessible names by default.
**Action:** Consistently enforce the addition of explicit `aria-label` attributes to interactive elements and `aria-hidden="true"` to purely decorative icons (like Lucide React icons) to ensure comprehensive assistive technology support.

## 2026-07-31 - Bookmarks a11y reapplied on BookmarkItem
**Learning:** After Bolt extracted `BookmarkItem`, Palette PRs #1093/#1097 conflicted on `BookmarksView.tsx`. Re-apply aria-labels/focus rings on category chips (`BookmarksView`) and item controls (`BookmarkItem`).
## 2024-03-24 - Missing ARIA Labels on Icon-Only UI Elements
**Learning:** In custom toolbars and dropdowns (like `TopStatusBar.tsx` and `NotificationsDropdown.tsx`), icon-only buttons often rely on `title` attributes for tooltips but lack explicit `aria-label`s. Screen readers may not consistently read `title` attributes on interactive elements, making them functionally invisible or confusing to visually impaired users.
**Action:** Always provide an explicit `aria-label` for icon-only buttons, even if a `title` attribute is present, and ensure the child icon components (like Lucide React icons) have `aria-hidden="true"` so they aren't redundantly announced.

## 2026-08-02 - [WAI-ARIA Switch Roles for Custom Toggles]
**Learning:** When building custom toggle switch components (e.g. using a styled button with a sliding indicator instead of a native checkbox), they require `role="switch"` and `aria-checked` attributes to be properly announced by screen readers as toggleable switches rather than generic buttons.
**Action:** Always verify that any custom toggle UI implements the WAI-ARIA switch pattern and includes appropriate focus-visible styles (e.g. `focus-visible:outline-none focus-visible:ring-2`) so keyboard users can navigate to and operate them clearly.

## 2024-05-18 - Form controls lacking IDs in reusable component wrappers
**Learning:** Reusable form components (like Input, Select, Slider wrappers) frequently lack proper `<label htmlFor="...">` and `<input id="...">` bindings because they are instantiated multiple times. This breaks screen reader associations and clickable label hit areas.
**Action:** Always verify reusable UI components generate and apply unique IDs (e.g. using React's `useId()` hook) to maintain accessible label bindings across multiple instantiations.
## 2026-08-04 - [Added ARIA switch attributes to custom toggle components]
**Learning:** When using custom `button` elements to represent on/off toggles (instead of native checkboxes), screen readers might announce them simply as 'button' without indicating state. Adding `role="switch"` and `aria-checked={boolean}` properly communicates their intended behavior and current state.
**Action:** Always include `role="switch"` and `aria-checked={boolean}` when creating custom toggle switches using buttons to ensure they are accessible to assistive technologies.
## 2026-08-07 - Add accessibility and focus states to SystemAlertBanner
**Learning:** Found an accessibility issue pattern in the app's components where custom interactive elements like alert dismiss buttons lacked `aria-label`, `type="button"`, and `focus-visible` styles, rendering them completely inaccessible to keyboard and screen reader users. Furthermore, wrapper containers acting as alerts lacked `role="alert"` and `aria-live="assertive"`.
**Action:** Always verify that dynamically rendered alert containers have `role="alert"` and `aria-live="assertive"`, and ensure all icon-only action buttons have explicit `aria-label`s, `type="button"`, and standard focus styles (`focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[color]/50`). Decorative icons inside interactive elements must be explicitly hidden from screen readers using `aria-hidden="true"`.
## 2026-08-08 - Button Accessibility: Explicit type attributes
**Learning:** Found a recurring pattern where `<button>` elements in interactive components (like `TopStatusBar.tsx`) lack the explicit `type="button"` attribute. Without this, buttons inside or near forms might default to `type="submit"`, causing unintended form submissions or page reloads when interacted with via keyboard or assistive technologies.
**Action:** Always ensure that `<button>` elements used strictly for UI actions or toggling state explicitly declare `type="button"` to guarantee predictable behavior across all contexts.
