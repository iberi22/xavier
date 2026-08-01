# Panel UI Frontend Build and Verification Runbook

This document describes the operations procedures for building, serving, and verifying the Xavier Panel UI frontend application.

## Overview

The Panel UI is a React/Vite-based single-page application located under `panel-ui/`.

- **Vite Build Directory**: By default, Vite is configured to compile assets into `panel-ui/build/` to align with Xavier's Rust backend configuration (`PANEL_BUILD_DIR = "panel-ui/build"`).
- **Mirror Build Directory**: For standard deployment pipelines and alternative verification tooling, a mirror directory is maintained under `panel-ui/dist/` via a `postbuild` hook.
- **Portability**: In portable layouts (desktop or standalone installer), the built `build` directory is shipped directly adjacent to the `xavier` executable.

---

## Build Procedure

To compile the production assets, follow these steps:

### Prerequisite Check
Ensure Node.js (>=22.12.0) and `pnpm` are installed.
```bash
node --version
pnpm --version
```

### Run Build Command
From the repository root or the `panel-ui` directory, run:
```bash
# Option A: From the repository root using workspace filters
pnpm --filter xavier-panel-ui build

# Option B: From the panel-ui directory
cd panel-ui
pnpm install
pnpm build
```

This will run:
1. `vite build` (generates compiled bundle into `panel-ui/build`)
2. `postbuild` hook (mirrors production assets into `panel-ui/dist`)

---

## Output Structure

Upon successful build completion, the following layout is generated:

```text
panel-ui/
├── build/ (used by Rust backend)
│   ├── assets/
│   │   ├── index.css
│   │   └── index.js
│   └── index.html
└── dist/ (used by standard static verification tools)
    ├── assets/
    │   ├── index.css
    │   └── index.js
    └── index.html
```

---

## Configuration and Overrides

Xavier's backend uses a structured fallback chain to locate the Panel UI frontend. If you need to point Xavier to an existing or custom build directory without relocating files, use the **`XAVIER_PANEL_UI_DIR`** environment variable.

```bash
# Example override
export XAVIER_PANEL_UI_DIR="/absolute/path/to/my/panel-ui/dist"
./xavier http
```

### Fallback Priority (within Rust backend):
1. `XAVIER_PANEL_UI_DIR` environment variable (if set and exists)
2. `<exe_dir>/panel-ui/build` (portable layout)
3. `<exe_dir>/panel-ui`
4. `<cwd>/panel-ui/build`
5. Compile-time `CARGO_MANIFEST_DIR/panel-ui/build` (development layout)

---

## Smoke Testing & Verification

To verify that the built frontend can be served and retrieved successfully, perform a curl-based smoke test.

### Step 1: Start Xavier in HTTP Mode
Run the server on a custom port in the background. Note that `XAVIER_TOKEN` must be configured for the startup to succeed:

```bash
XAVIER_TOKEN=dummy_verification_token ./target/debug/xavier http 8123 --mcp-port 0 > xavier_server.log 2>&1 &
```

### Step 2: Query the Frontend Root
Perform a request to verify that the server returns the generated `index.html`:

```bash
curl -s http://localhost:8123/ | grep -q "Xavier Panel" && echo "✅ Smoke test passed!" || echo "❌ Smoke test failed!"
```

### Step 3: Shutdown the Server
Clean up the background process:
```bash
kill $(lsof -t -i :8123) 2>/dev/null || true
```

---

## Troubleshooting

### Issue: "Panel frontend assets are missing"
If you request `http://localhost:8006/` and see a 503 SERVICE UNAVAILABLE page explaining that assets are missing:
1. Ensure `pnpm build` was run successfully inside `panel-ui/`.
2. Check that `panel-ui/build/index.html` exists.
3. If running the server outside the repository root directory, set `XAVIER_PANEL_UI_DIR` to the absolute path of the built frontend directory.
