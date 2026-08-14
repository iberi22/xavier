## 2026-06-12 - ParticleBackground re-render optimization
**Learning:** `ParticleBackground` is a heavy canvas component that doesn't accept props but is heavily re-rendered when parent `App.tsx` state changes (like during chat typing/streaming), leading to unnecessary CPU load and animation restarts.
**Action:** Use `React.memo` to wrap pure visual components like `ParticleBackground` that don't depend on parent props to ensure they skip rendering during complex parent state mutations.

## 2026-06-12 - ChatHistory JSON.parse re-render bottleneck
**Learning:** In list rendering components like `ChatHistory`, inline dynamic object parsing like `JSON.parse(msg.metadata)` within the array `.map()` causes an O(N) performance bottleneck. Whenever a new element is appended, the parent component re-renders, causing every old string to be parsed from scratch.
**Action:** Always extract complex list items that do heavy parsing or computations into a standalone component wrapped in `React.memo()`. This ensures that when new items are appended, the old items are skipped by reconciliation, making list appends O(1) instead of O(N).

## 2026-07-30 - BookmarksView list item re-render optimization
**Learning:** Similar to the ChatHistory issue, `BookmarksView` keeps inline editing state for each bookmark at the parent level (`editingId`, `editTitle`, etc.). This means typing *one letter* in an edit field re-renders the *entire* list of bookmarks, causing massive lag when there are many artifacts.
**Action:** Always extract complex interactive list items into a standalone component (`BookmarkItem`) wrapped in `React.memo()`. Localize the interactive state (like form fields) inside the child item so that updates only trigger a re-render for that specific item, not the entire parent list.

## 2026-08-01 - MalocaView list rendering bottlenecks
**Learning:** `MalocaView` was doing inline `.map()` for lists of `proposals` and `nodes` directly inside parent components like `CouncilPanel` and `NodesPanel`. Updating local states like text input for a single item would cause a full re-render of large lists, leading to UI lag.
**Action:** As learned from `BookmarksView`, always extract large lists containing complex interactive items into separate `React.memo()` wrapped components (e.g., `ProposalItem`, `MeshNodeItem`). Pass minimal callbacks down and keep item-specific state local to the item component.

## 2026-08-02 - MemoryCard re-render optimization
**Learning:** `MemoryBrowser` maps over a list of `MemoryEntry` elements, rendering `MemoryCard` instances. Given the presence of complex inner states in child components (`expanded` state toggles) and dynamic inputs driving search functionality and new additions on the parent, leaving `MemoryCard` un-memoized results in unnecessary rendering logic running per character typed, scaling with O(N) where N is the current page size of the rendered list.
**Action:** Used `React.memo` around `MemoryCard` inside `MemoryBrowser`. This reduces string DOM reconciliations inside the mapping iteration making appending/filtering behave close to O(1) in the visual layer.
## 2026-08-03 - Memoization of App, GraphCanvas, and InputArea components
**Learning:** In the `panel-ui` frontend, components rendering heavy nested structures (e.g., `ForceGraph2D` in `GraphCanvas`) or iterative child elements (e.g., `DraggableWidget` lists in `App`) must have their callback props wrapped in `useCallback` (e.g., `handleNodeClick`, `handleRemoveWidget`), and the child components themselves wrapped in `React.memo()`. This establishes rendering optimization by preventing expensive sub-tree reconciliations during higher-level parent state changes.
**Action:** Use `React.memo()` to wrap iterative components like `DraggableWidget` and static layout elements like `InputArea` to ensure they avoid re-rendering. Additionally, use `useCallback` to memoize inline functions passed down as props to heavily nested components like `ForceGraph2D` to avoid unneeded reconciliations.
## 2024-05-18 - [React.memo on GraphCanvas]
**Learning:** `ForceGraph2D` is computationally expensive to render. When wrapped components like `GraphCanvas` are mounted in interactive parents (like `ConfigModal` with tab switches), they will trigger N+1 canvas recalculations on unrelated state updates.
**Action:** Always wrap computationally heavy visualizer components (like canvas/force-graphs) in `React.memo()` if their props don't need to change frequently.

## 2026-08-06 - Proper Memoization of Callback Props
**Learning:** Wrapping a component in `React.memo()` (like `InputArea`) is useless if the parent component (`App.tsx`) passes inline arrow functions or unmemoized functions as props. This causes the `React.memo()` component to re-render on every parent render, completely negating the performance optimization and potentially causing layout thrashing during fast state updates (like streaming chat tokens).
**Action:** Always verify that all callback props passed to a `React.memo()` component are wrapped in `useCallback()` in the parent component to maintain referential stability.

## 2026-08-07 - [TopStatusBar Memoization]
**Learning:** In a highly animated React app (like `panel-ui` using Framer Motion), statically placed components that consume global state or contain local polling intervals (`TopStatusBar`) must be wrapped in `React.memo` to prevent profound layout recalculations across the app whenever a parent context (e.g. typing in an input field) updates.
**Action:** Wrap complex, static UI layer components in `React.memo()` to decouple their render lifecycle from fast-updating siblings or parents.
## 2026-08-08 - [React.memo boundary for ChatHistory container]
 **Learning:** Large components rendering iterative child lists (like `ChatHistory` rendering `ChatMessageItem`) will still re-render when a high-frequency parent state changes (like typing inputs in `App.tsx`), even if the individual list items are memoized.
 **Action:** Always wrap top-level dynamic list containers in `React.memo()` to prevent O(1) container re-renders and expensive DOM tree traversal when the container props haven't changed.
## 2024-08-14 - Fix unnecessary child component re-renders
**Learning:** Wrapping callback props passed to child components in `useCallback` is important to prevent unnecessary re-renders in React when parent state updates.
**Action:** Always wrap `onClose`, `onComplete`, and other callbacks in `useCallback` when passing them to memoized or expensive child components.
