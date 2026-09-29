# Xavier desktop app (Linux)

A double-click desktop app: the Tauri shell (`panel-ui/src-tauri`) plus the panel UI and a bundled `xavier` binary.

## Install

1. Download `Xavier_<version>_amd64.AppImage` (or the `.deb`) from the [latest release](https://github.com/iberi22/xavier/releases/latest).
2. `chmod +x Xavier_*.AppImage`, then double-click it (or run it from a terminal).
3. `.deb`: `sudo apt install ./Xavier_*_amd64.deb`.

Requires a glibc 2.35+ distro (built on Ubuntu 22.04) with GTK3 and WebKitGTK 4.1 available; the AppImage bundles what it needs.

### NixOS

AppImages are not FHS binaries. Either run them with `appimage-run Xavier_*.AppImage`, or enable `programs.appimage = { enable = true; binfmt = true; };` so double-click and `./Xavier_*.AppImage` work directly. If the window is blank, try `WEBKIT_DISABLE_DMABUF_RENDERER=1`.

## How it finds a server (safety contract)

On launch the app probes `http://127.0.0.1:${XAVIER_PORT:-8006}/health`:

- **A Xavier answers** (for example your `xavier.service` systemd unit): the UI attaches to it. Nothing is spawned, and the app never stops, restarts or kills it - not on start, not on exit.
- **Nothing answers**: the app starts its bundled sidecar (`xavier http <port> --mcp-port 0`) with its own data directory and stops only that child when you quit. If the port is held by something that is not Xavier, the app reports an error instead of touching it.
- A sidecar left behind by a crash is just "a Xavier that answers" on the next launch: it is re-attached, not killed.

Closing the window quits the app. A second launch focuses the running window (single instance).

## Data and configuration

| What | Where |
|---|---|
| Sidecar data (SQLite, vec store) | `$XDG_DATA_HOME/xavier-desktop` or `~/.local/share/xavier-desktop` (override: `XAVIER_DESKTOP_DATA_DIR`) |
| Sidecar root token | `<data dir>/sidecar-token` (mode 0600, generated on first run) |
| Port | `XAVIER_PORT` (default 8006) |

The sidecar never uses `~/.xavier`, so it cannot collide with a production install.

## Authentication

The panel authenticates with its own `/auth/*` session (register / login, optional 2FA), the same as the browser panel at `http://127.0.0.1:8006/`. The app does not read or inject `XAVIER_TOKEN` into the UI, so no raw API token is ever placed in the webview. Attaching to an existing server therefore shows the panel's login screen; use an account on that server. A fresh sidecar starts empty: register an account on first run.

The shell points the panel at the resolved server by setting `localStorage.xavier_remote_url` to `http://127.0.0.1:<port>`; a non-loopback URL you chose in Settings is left alone.

## PDF layout (optional)

The sidecar is built with `pageindex-pdfium`. PDF bookmarks work without extra files. For font-based heading detection the release build bundles `libpdfium.so` (bblanchon/pdfium-binaries) under the app resources and passes it via `XAVIER_PAGEINDEX_PDFIUM_LIB`. If that download fails in CI the app ships without it and PDF layout falls back to bookmarks.

## Building

CI: `.github/workflows/desktop.yml` (Actions > Desktop > Run workflow, any branch) uploads the `desktop-linux` artifact; `release.yml` calls it on `v*` tags and attaches the AppImage, `.deb` and `SHA256SUMS-desktop.txt` to the release.

Locally (non-NixOS):

```bash
cargo build --release --bin xavier --features "ci-safe,pageindex-pdfium"
TRIPLE=$(rustc -Vv | sed -n 's/^host: //p')
mkdir -p panel-ui/src-tauri/binaries && cp target/release/xavier "panel-ui/src-tauri/binaries/xavier-$TRIPLE"
pnpm install && cd panel-ui && pnpm exec tauri build --bundles appimage,deb
```

On NixOS, `nix-shell shell.nix` supports `cargo check`/`clippy` of the shell, but AppImage bundling downloads FHS tooling, so use the CI artifact.
