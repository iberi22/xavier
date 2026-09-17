## 2024-05-20 - [Accessible Custom Toggle Buttons]
**Learning:** Custom UI toggle buttons built with `div` or `span` mimicking a native toggle require explicit ARIA attributes like `role="switch"` and `aria-checked` so screen readers understand their state, along with `aria-label` for an accessible name.
**Action:** Always add `role="switch"`, `aria-checked`, `aria-label`, and `focus-visible` styles when implementing custom toggle buttons to ensure keyboard and screen reader accessibility.
