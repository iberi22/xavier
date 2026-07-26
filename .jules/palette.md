## 2026-07-20 - Accessible Form Controls with Neon Focus
**Learning:** Icon-only controls and form inputs require explicit aria-labels and htmlFor bindings, paired with focus-visible styles that match the app's neon accent (`focus-visible:ring-[#39ff14]/50`) for keyboard accessibility.
**Action:** Ensure all future custom form controls map standard ARIA attributes while incorporating the design system's specific focus rings.
## 2026-07-28 - Memory Browser Accessibility Labels
**Learning:** React fragments and generic HTML interactive elements (like icon-only `<button>`, `<input>`, and `<select>`) in Xavier's UI components often lack screen-reader accessible names by default.
**Action:** Consistently enforce the addition of explicit `aria-label` attributes to interactive elements and `aria-hidden="true"` to purely decorative icons (like Lucide React icons) to ensure comprehensive assistive technology support.

## 2024-05-18 - Missing ARIA labels and focus styles on icon-only buttons
**Learning:** Found an accessibility issue pattern where icon-only buttons lacked ARIA labels and keyboard focus states. The lucide-react icons used inside them also missed the `aria-hidden="true"` attribute, which could lead to screen readers misinterpreting them.
**Action:** When adding or updating icon-only buttons, ensure to include `aria-label` attribute, `aria-hidden="true"` on the enclosed icon, and standard neon focus styles (`focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[#39ff14]/50`) for proper accessibility.
