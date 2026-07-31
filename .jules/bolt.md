## 2026-06-12 - ParticleBackground re-render optimization
**Learning:** `ParticleBackground` is a heavy canvas component that doesn't accept props but is heavily re-rendered when parent `App.tsx` state changes (like during chat typing/streaming), leading to unnecessary CPU load and animation restarts.
**Action:** Use `React.memo` to wrap pure visual components like `ParticleBackground` that don't depend on parent props to ensure they skip rendering during complex parent state mutations.
## 2026-06-12 - ChatHistory JSON.parse re-render bottleneck
**Learning:** In list rendering components like `ChatHistory`, inline dynamic object parsing like `JSON.parse(msg.metadata)` within the array `.map()` causes an O(N) performance bottleneck. Whenever a new element is appended, the parent component re-renders, causing every old string to be parsed from scratch.
**Action:** Always extract complex list items that do heavy parsing or computations into a standalone component wrapped in `React.memo()`. This ensures that when new items are appended, the old items are skipped by reconciliation, making list appends O(1) instead of O(N).
## 2026-07-30 - BookmarksView list item re-render optimization
**Learning:** Similar to the ChatHistory issue, `BookmarksView` keeps inline editing state for each bookmark at the parent level (`editingId`, `editTitle`, etc.). This means typing *one letter* in an edit field re-renders the *entire* list of bookmarks, causing massive lag when there are many artifacts.
**Action:** Always extract complex interactive list items into a standalone component (`BookmarkItem`) wrapped in `React.memo()`. Localize the interactive state (like form fields) inside the child item so that updates only trigger a re-render for that specific item, not the entire parent list.
