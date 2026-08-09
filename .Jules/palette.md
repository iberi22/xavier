# Palette Journal

Critical UX/accessibility learnings only.

## 2026-07-17 - Icon-only controls rely on `title` alone
**Learning:** Primary panel-ui chrome (InputArea, TopStatusBar, NotificationsDropdown) uses lucide icon-only buttons with `title` tooltips but almost no `aria-label` / `aria-pressed` / focus-visible rings. Screen readers get empty buttons; keyboard focus is invisible against the dark glass UI.
**Action:** When touching any icon-only control, pair `title` with `aria-label`, set `aria-hidden` on decorative icons, and add `focus-visible:ring-2 focus-visible:ring-[#39ff14]/50` so keyboard focus matches the neon accent.
## 2026-08-06 - Added accessible form bindings
**Learning:** Reusable form UI components must utilize React's `useId()` hook to generate unique identifiers, ensuring accessible `<label htmlFor="...">` to `<input id="...">` bindings without collisions.
**Action:** Added `useId()` to `CloudRelayConfig.tsx` to bind inputs properly.
## 2026-08-09 - Add ARIA label to GraphView close button
**Learning:** Found that the slide-over details panel in `GraphView.tsx` had an icon-only close button (`<X />`) missing an `aria-label`.
**Action:** Added `aria-label="Close details"` to ensure screen reader users can understand the button's purpose when focused.
