# [feat] Panel UI E2E Testing and Live Backend Integration

## Context
The Panel UI interface redesign (React + Vite) has been successfully merged and parity with the Xavier Rust backend is complete. The backend now exposes `/panel/api/bookmarks`, `/panel/api/widgets`, and `/panel/api/graph` via a local SQLite memory store.

## Problem
Currently, the E2E tests in `panel-ui/tests/app.spec.ts` are using mocked API responses (e.g., `page.route('**/panel/api/threads', ...)`). Additionally, Playwright is not properly configured within the `pnpm` monorepo workspace for `panel-ui`, causing `pnpm test` or `npx playwright test` to fail.

## Goal
Fully integrate Playwright into the `panel-ui` directory and rewrite the E2E tests to execute against the locally compiled Xavier Rust backend, ensuring 100% test pass rate for all core UI flows.

## Required Steps
1. **Fix `pnpm` Setup**:
   - Ensure `@playwright/test` is correctly installed in `panel-ui/package.json`.
   - Ensure you can run Playwright commands via `pnpm` inside the `panel-ui` directory.

2. **Remove API Mocks**:
   - Edit `panel-ui/tests/app.spec.ts`.
   - Remove all mock routing (like `page.route('**/health', ...)`).

3. **Live Backend Integration**:
   - Modify the Playwright config (`playwright.config.ts`) to use a `webServer` configuration that automatically compiles and spawns the Xavier backend before tests run. 
   - **Command to spawn backend**: `cargo run --bin xavier-tui --features ui` (or equivalent, ensuring it starts on the correct test port).
   - Ensure tests hit `http://127.0.0.1:8006` or wherever the backend binds.

4. **Verify UI Flows**:
   - Authentication (Token validation)
   - Bookmarks creation/pinning
   - Graph visualization rendering
   - Draggable Widgets

## Acceptance Criteria
- `pnpm exec playwright test` inside `panel-ui` passes 100%.
- No mocked `page.route` intercepts for Xavier API endpoints exist in the test suite.
- The tests run against a real, ephemeral SQLite instance managed by the backend during the test run.
