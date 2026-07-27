## 2026-06-12 - ParticleBackground re-render optimization
**Learning:** `ParticleBackground` is a heavy canvas component that doesn't accept props but is heavily re-rendered when parent `App.tsx` state changes (like during chat typing/streaming), leading to unnecessary CPU load and animation restarts.
**Action:** Use `React.memo` to wrap pure visual components like `ParticleBackground` that don't depend on parent props to ensure they skip rendering during complex parent state mutations.
## 2026-06-12 - ChatHistory JSON.parse re-render bottleneck
**Learning:** In list rendering components like `ChatHistory`, inline dynamic object parsing like `JSON.parse(msg.metadata)` within the array `.map()` causes an O(N) performance bottleneck. Whenever a new element is appended, the parent component re-renders, causing every old string to be parsed from scratch.
**Action:** Always extract complex list items that do heavy parsing or computations into a standalone component wrapped in `React.memo()`. This ensures that when new items are appended, the old items are skipped by reconciliation, making list appends O(1) instead of O(N).
## 2026-06-12 - BookmarksView list item editing re-render bottleneck
**Learning:** In list rendering components like `BookmarksView`, having local item state (like editing properties) managed in the parent component means every keystroke triggers a re-render of the entire list (an O(N) operation).
**Action:** Always extract interactive list items into their own components wrapped in `React.memo()` and push their local ephemeral state inside them. This limits state changes (like typing in an edit input) to only re-rendering that specific item, achieving O(1) rendering time instead of O(N) when updating single item states in a list.
