## 2026-06-12 - ParticleBackground re-render optimization
**Learning:** `ParticleBackground` is a heavy canvas component that doesn't accept props but is heavily re-rendered when parent `App.tsx` state changes (like during chat typing/streaming), leading to unnecessary CPU load and animation restarts.
**Action:** Use `React.memo` to wrap pure visual components like `ParticleBackground` that don't depend on parent props to ensure they skip rendering during complex parent state mutations.
## 2026-06-12 - ChatHistory JSON.parse re-render bottleneck
**Learning:** In list rendering components like `ChatHistory`, inline dynamic object parsing like `JSON.parse(msg.metadata)` within the array `.map()` causes an O(N) performance bottleneck. Whenever a new element is appended, the parent component re-renders, causing every old string to be parsed from scratch.
**Action:** Always extract complex list items that do heavy parsing or computations into a standalone component wrapped in `React.memo()`. This ensures that when new items are appended, the old items are skipped by reconciliation, making list appends O(1) instead of O(N).
## 2026-06-12 - BookmarksView O(N) re-render optimization
**Learning:** In list rendering components like `BookmarksView`, managing inline edit state (like `editTitle`, `editType`) at the parent level causes an O(N) performance bottleneck. Whenever a user types in an edit input, the parent component re-renders, causing every bookmark in the list to re-render.
**Action:** Extract list items that manage local user interaction state into a standalone component wrapped in `React.memo()`. This ensures that when a user types in one item's input, only that specific item re-renders, resolving the O(N) bottleneck.
