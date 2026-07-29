## 2026-07-20 - Accessible Form Controls with Neon Focus
**Learning:** Icon-only controls and form inputs require explicit aria-labels and htmlFor bindings, paired with focus-visible styles that match the app's neon accent (`focus-visible:ring-[#39ff14]/50`) for keyboard accessibility.
**Action:** Ensure all future custom form controls map standard ARIA attributes while incorporating the design system's specific focus rings.
## 2026-07-28 - Memory Browser Accessibility Labels
**Learning:** React fragments and generic HTML interactive elements (like icon-only `<button>`, `<input>`, and `<select>`) in Xavier's UI components often lack screen-reader accessible names by default.
**Action:** Consistently enforce the addition of explicit `aria-label` attributes to interactive elements and `aria-hidden="true"` to purely decorative icons (like Lucide React icons) to ensure comprehensive assistive technology support.
## 2026-07-29 - Hover-revealed Buttons Keyboard Accessibility
**Learning:** Buttons that are visually hidden until hovered (using `opacity-0` and `group-hover:opacity-100`) are completely invisible to keyboard-only users who navigate via Tab.
**Action:** When using hover-revealed patterns, always pair `opacity-0` with `focus-visible:opacity-100` so the button becomes visible when it receives keyboard focus, ensuring feature parity for keyboard users.
