## 2026-06-12 - ParticleBackground re-render optimization
**Learning:** `ParticleBackground` is a heavy canvas component that doesn't accept props but is heavily re-rendered when parent `App.tsx` state changes (like during chat typing/streaming), leading to unnecessary CPU load and animation restarts.
**Action:** Use `React.memo` to wrap pure visual components like `ParticleBackground` that don't depend on parent props to ensure they skip rendering during complex parent state mutations.
## 2026-06-12 - ChatHistory JSON.parse re-render bottleneck
**Learning:** In list rendering components like `ChatHistory`, inline dynamic object parsing like `JSON.parse(msg.metadata)` within the array `.map()` causes an O(N) performance bottleneck. Whenever a new element is appended, the parent component re-renders, causing every old string to be parsed from scratch.
**Action:** Always extract complex list items that do heavy parsing or computations into a standalone component wrapped in `React.memo()`. This ensures that when new items are appended, the old items are skipped by reconciliation, making list appends O(1) instead of O(N).
## 2026-06-12 - BookmarksView list rendering optimization
**Learning:** In `BookmarksView`, mapping over `filteredBookmarks` and rendering complex DOM for each artifact caused the entire grid to re-render whenever state like `editingId` changed.
**Action:** Extract list items into their own components (e.g. `BookmarkCardItem`) and wrap them in `React.memo()` to prevent O(N) re-renders when parent state changes but individual item props remain identical. Also, ensure local item state (like editing state) is pushed down into the item component itself so it doesn't pollute the parent's props, effectively breaking React.memo.
