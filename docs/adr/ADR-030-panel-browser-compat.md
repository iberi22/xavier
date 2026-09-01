# ADR-030: Panel-UI Dual-Mode Browser Compatibility Pattern

*Status: ACCEPTED | Date: 2026-09-01 | Deciders: Xavier Core & Panel Teams*

---

## Context

Xavier's management panel (`panel-ui`) was originally designed to run inside a Tauri desktop container. In this environment, frontend components used static imports from `@tauri-apps/api/core` and invoked native IPC functions such as `invoke("get_realtime_metrics")`, `invoke("get_xavier_token")`, `invoke("get_current_config_state")`, and event listeners like `listen("new-notification")`.

When `panel-ui` is served directly in standard web browsers (e.g., Chrome, Firefox, Safari) via HTTP server (`:8006`), native Tauri IPC bindings are absent. Static imports or un-guarded calls to Tauri APIs cause catastrophic runtime failures:
1. `TypeError: transformCallback is not a function` or undefined IPC bridge rejections.
2. Unhandled promise rejections on app initialization, breaking core UI rendering.
3. Infinite 401 unauthorized loops due to missing token resolution in browser sessions.
4. Total component crashes in header, notification center, and folder selection modules.

To support seamless dual-mode execution (Tauri desktop app AND standalone web browser application), `panel-ui` requires a robust, non-breaking architecture pattern for browser compatibility.

---

## Decision

We adopt a universal **Dual-Mode Browser-Safe Execution Pattern** across all `panel-ui` components.

### 1. Environment Detection via `__TAURI_INTERNALS__` Guard
We establish `__TAURI_INTERNALS__` in `window` as the canonical runtime discriminator:
```typescript
export const isTauriEnvironment = (): boolean => {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
};
```
Static top-level imports of `@tauri-apps/api` are strictly forbidden in shared components.

### 2. Dynamic Import of Tauri IPC Bindings
Tauri IPC modules are loaded dynamically only when executing inside Tauri:
```typescript
if (isTauriEnvironment()) {
  const { invoke } = await import("@tauri-apps/api/core");
  const result = await invoke("native_command");
} else {
  // Execute HTTP fallback
}
```

### 3. Graceful HTTP API Fallbacks
When running in browser mode, components fall back to standard HTTP endpoints exposed by Xavier runtime (`:8006`):
- **Realtime Metrics**: `TopStatusBar` fetches `/health` (unauthenticated) to extract `system.cpu_usage` and `ram_usage_percent`, replacing `get_realtime_metrics`.
- **Notifications**: `NotificationCenter` and `NotificationsDropdown` poll `GET /notifications` (with `X-Xavier-Token`) every 30 seconds, replacing Tauri's `listen("new-notification")` event subscription.
- **Authentication**: `useApiToken` hook resolves local storage overrides first, falling back to build-time `VITE_XAVIER_API_TOKEN` environment variable when `invoke("get_xavier_token")` is unavailable.
- **File & Directory Selection**: `InputArea` falls back to browser File API (`<input type="file" webkitdirectory />`) to extract relative root directory paths when native `open({directory: true})` is unavailable.

### 4. Resilient UX State Management
- Components display `LoadingSpinner` or skeleton pulse loaders while asynchronous HTTP fallback requests resolve.
- Global `ErrorToast` components catch and display network or server errors without crashing the main application view.

---

## Consequences

### Positive
- **Zero Browser Crashes**: Eliminates all `TypeError` and IPC rejection crashes when opening `panel-ui` in standard browsers.
- **Dual-Mode Parity**: Identical user interface and core functionality whether accessed via Tauri desktop or Web browser.
- **Production Readiness**: Enables deployment of `panel-ui` as a hosted web application or Docker container service behind reverse proxies.
- **Backwards Compatibility**: Tauri desktop builds remain 100% functional without degradation or performance penalty.

### Negative / Trade-offs
- **Polling Overhead**: 30-second notification polling in browser mode introduces minor periodic network requests compared to push-based Tauri events.
- **Vite Bundler Warnings**: Dynamic imports of Tauri modules trigger minor Vite code-splitting chunk warnings during build, which are explicitly acceptable.
- **Local CORS / Token Requirement**: Standalone browser mode requires `VITE_XAVIER_API_TOKEN` configuration for authenticated endpoints.

---

## Alternatives

1. **Separate Desktop and Web Codebases**:
   - *Rejected*: Maintaining two separate React applications would duplicate UI logic and drastically increase maintenance overhead.

2. **WebSockets for Browser Mode**:
   - *Rejected for Wave 5*: WebSockets add protocol complexity and state management overhead; HTTP polling at 30-second intervals fulfills all current notification and metric update requirements cleanly.

3. **Global Mock / Polyfill of Tauri IPC**:
   - *Rejected*: Polyfilling `window.__TAURI_INTERNALS__` in browser mode masks missing HTTP endpoints and risks silent failure modes during deployment. Dynamic imports with explicit fallbacks provide transparent, typed runtime behavior.
