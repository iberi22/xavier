# Mobile Roadmap - Xavier & SWAL Projects

This document outlines the approach for bringing Xavier and other SWAL (Selective Workspace for Agentic Learning) projects to mobile platforms using Tauri Mobile.

## Approach: Tauri Mobile Monorepo

We leverage Tauri v2's native support for Android and iOS. The monorepo structure allows us to share the core Rust logic and the React-based Panel UI across desktop and mobile.

### Monorepo Structure

- `xavier-core/`: Pure Rust logic (the 62 modules).
- `panel-ui/`: React + Tailwind v4 + Vite frontend.
- `panel-ui/src-tauri/`: Tauri configuration and native Rust bindings.
- `mobile/`: Symlinked entry point for mobile-specific builds and configuration.
  - `mobile/src/` -> `../panel-ui/src`
  - `mobile/src-tauri/` -> `../panel-ui/src-tauri`
  - `mobile/android/` -> Generated Android project.

### Mobile-Responsive UI Strategy

- **Viewport Units**: Use `dvh` (Dynamic Viewport Height) instead of `vh` to account for mobile browser toolbars and notches.
- **Tailwind v4**: Utilize responsive modifiers (`sm:`, `max-md:`) for adaptive layouts.
- **Touch Optimization**: Ensure clickable elements are at least 44x44px.
- **Native Bridges**: Use `@tauri-apps/api` for native features (notifications, filesystem) with graceful fallbacks.

## Project Projections

### GARA-G (Flutter -> Tauri Mobile)

Currently, GARA-G uses Flutter for mobile. To unify the ecosystem:
1. **Migration Path**: Incrementally replace Flutter screens with React components from the unified `panel-ui`.
2. **Logic Reuse**: Bind the GARA-G Rust core directly to Tauri commands.
3. **Benefit**: Single codebase for UI and logic across Linux, Windows, macOS, and Android.

### Cortex (Python -> Rust Plugin)

Cortex relies on Python for certain agentic workflows.
1. **Embedding Strategy**: Embody Python as a Rust plugin or sidecar.
2. **Mobile Constraints**: On Android, use `pyo3` with a cross-compiled Python interpreter or move heavy Python logic to a remote Xavier node, using the mobile app as a thin client.
3. **UI**: Adapt the Cortex management dashboard to the `mobile-responsive` patterns established in Xavier.

## Future Phases

1. **Phase 1 (Complete)**: Basic Android build and responsive UI shell for Xavier.
2. **Phase 2**: Full iOS target support (requires macOS runner).
3. **Phase 3**: Biometric authentication integration (Fingerprint/FaceID) via Tauri plugins.
4. **Phase 4**: Offline-first vector search on-device using `sqlite-vec`.
