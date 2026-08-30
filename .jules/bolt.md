## 2024-05-24 - React component performance with useMemo
**Learning:** In `MeshTopologyGraph.tsx`, node array filtering and statuses processing logic were running on every re-render, leading to O(N) operations.
**Action:** Wrapped the nodes generation filtering in `useMemo` to prevent recalculation when parent states like text inputs in tabs changed, and wrapped the export component inside `React.memo()`
