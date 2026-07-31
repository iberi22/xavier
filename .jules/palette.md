## 2026-07-20 - Accessible Form Controls with Neon Focus
**Learning:** Icon-only controls and form inputs require explicit aria-labels and htmlFor bindings, paired with focus-visible styles that match the app's neon accent (`focus-visible:ring-[#39ff14]/50`) for keyboard accessibility.
**Action:** Ensure all future custom form controls map standard ARIA attributes while incorporating the design system's specific focus rings.
## 2026-07-28 - Memory Browser Accessibility Labels
**Learning:** React fragments and generic HTML interactive elements (like icon-only `<button>`, `<input>`, and `<select>`) in Xavier's UI components often lack screen-reader accessible names by default.
**Action:** Consistently enforce the addition of explicit `aria-label` attributes to interactive elements and `aria-hidden="true"` to purely decorative icons (like Lucide React icons) to ensure comprehensive assistive technology support.
## 2025-02-12 - [Improved Keyboard Accessibility for Action Buttons in BookmarksView]
**Learning:** Found an accessibility issue pattern where hover-revealed action buttons (`opacity-0 group-hover:opacity-100`) inside list items (like `BookmarksView` cards) are invisible to keyboard-only users who navigate via `Tab`.
**Action:** Always add `focus-within:opacity-100` alongside hover classes to containers holding hidden action elements. Additionally, ensure all inputs and icon-only buttons receive `aria-label`, `aria-hidden="true"` (for SVG icons), and consistent `focus-visible:ring` styles.
