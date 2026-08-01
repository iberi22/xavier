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
