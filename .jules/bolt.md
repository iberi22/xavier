## 2026-06-12 - ParticleBackground re-render optimization
**Learning:** `ParticleBackground` is a heavy canvas component that doesn't accept props but is heavily re-rendered when parent `App.tsx` state changes (like during chat typing/streaming), leading to unnecessary CPU load and animation restarts.
**Action:** Use `React.memo` to wrap pure visual components like `ParticleBackground` that don't depend on parent props to ensure they skip rendering during complex parent state mutations.
## 2026-06-12 - ChatHistory JSON.parse re-render bottleneck
**Learning:** In list rendering components like `ChatHistory`, inline dynamic object parsing like `JSON.parse(msg.metadata)` within the array `.map()` causes an O(N) performance bottleneck. Whenever a new element is appended, the parent component re-renders, causing every old string to be parsed from scratch.
**Action:** Always extract complex list items that do heavy parsing or computations into a standalone component wrapped in `React.memo()`. This ensures that when new items are appended, the old items are skipped by reconciliation, making list appends O(1) instead of O(N).

## 2026-07-23 - MemoryBrowser Search Input Debouncing
**Learning:** In the React frontend, binding a search input directly to an API call inside a  can cause severe API spam (making a request on every single keystroke) if not debounced.
**Action:** Use a debounced state (e.g.  updated via  in a  with a cleanup) to delay network calls until the user has stopped typing.
## 2026-06-12 - MemoryBrowser Search Input Debouncing
**Learning:** In the React frontend, binding a search input directly to an API call inside a `useEffect` can cause severe API spam (making a request on every single keystroke) if not debounced.
**Action:** Use a debounced state (e.g. `debouncedQuery` updated via `setTimeout` in a `useEffect` with a cleanup) to delay network calls until the user has stopped typing.
