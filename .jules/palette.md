## 2023-10-24 - Status Bar Configuration Button Hidden from Keyboard Users
**Learning:** Utility classes like `opacity-0` with `group-hover:opacity-100` hide interactive elements from keyboard users completely, making them impossible to discover via tab navigation, even if they are focusable.
**Action:** Always pair `opacity-0` hover-revealed buttons with `focus-visible:opacity-100` and focus rings (e.g. `focus-visible:ring-2`) to ensure keyboard users can discover and interact with them.
