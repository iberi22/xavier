## 2026-07-20 - Accessible Form Controls with Neon Focus
**Learning:** Icon-only controls and form inputs require explicit aria-labels and htmlFor bindings, paired with focus-visible styles that match the app's neon accent (`focus-visible:ring-[#39ff14]/50`) for keyboard accessibility.
**Action:** Ensure all future custom form controls map standard ARIA attributes while incorporating the design system's specific focus rings.
## 2026-07-28 - Memory Browser Accessibility Labels
**Learning:** React fragments and generic HTML interactive elements (like icon-only `<button>`, `<input>`, and `<select>`) in Xavier's UI components often lack screen-reader accessible names by default.
**Action:** Consistently enforce the addition of explicit `aria-label` attributes to interactive elements and `aria-hidden="true"` to purely decorative icons (like Lucide React icons) to ensure comprehensive assistive technology support.
## 2026-12-06 - Accessible Hover-Reveal Buttons
**Learning:** Visually hidden interactive elements (like icon buttons that appear on group-hover) must remain keyboard accessible. Using `opacity-0` hides them from sight but not from focus. However, when a keyboard user tabs to them, they remain invisible unless a focus state makes them visible again.
**Action:** Always add `focus-within:opacity-100` to the container of hover-reveal buttons, and ensure each button has `focus-visible:outline-none focus-visible:ring-2` to provide clear, visible feedback when navigating via keyboard. Also ensure they have explicit `aria-label`s and `aria-hidden="true"` on their inner SVG icons.
