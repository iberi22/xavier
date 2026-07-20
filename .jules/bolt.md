## 2026-06-12 - ParticleBackground re-render optimization
**Learning:** `ParticleBackground` is a heavy canvas component that doesn't accept props but is heavily re-rendered when parent `App.tsx` state changes (like during chat typing/streaming), leading to unnecessary CPU load and animation restarts.
**Action:** Use `React.memo` to wrap pure visual components like `ParticleBackground` that don't depend on parent props to ensure they skip rendering during complex parent state mutations.
