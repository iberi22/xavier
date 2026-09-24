## 2024-09-24 - Accessibility fixes in MessagingConfigModal
**Learning:** In React form inputs, associating labels (`htmlFor`) with input `id`s and explicitly specifying `type="button"` on non-submit buttons prevents a11y violations (e.g. `lint/a11y/noLabelWithoutControl`, `lint/a11y/useButtonType`) and stops unexpected form submissions.
**Action:** Always link form controls with labels properly, and never leave `<button>` without an explicit type.
