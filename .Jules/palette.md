## 2026-09-01 - [Missing type attributes on buttons]
**Learning:** Several buttons throughout the application lack the explicit `type="button"` attribute. In React/HTML, a button without a type defaults to `type="submit"`, which can lead to unintended form submissions if these components are ever used within a form.
**Action:** Always explicitly specify `type="button"` on any interactive button component unless it is specifically intended to submit a form.
