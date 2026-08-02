# Release Packaging Runbook

This document outlines the complete, production-ready release packaging pathway for Xavier. It covers building native installers on Windows (WiX Toolset and Inno Setup), desktop packaging for Linux/macOS (Tauri), and cross-compilation configurations (from NixOS to Windows).

---

## 1. Windows Installer Build (WiX & Inno Setup)

Windows packaging was simplified in Ola 11 (#1138). The legacy, unused `xavier-gui.exe` standalone binary was completely removed. All installer shortcuts now point directly to **`xavier.exe`** (the canonical CLI and HTTP-based server binary) or **`xavier-tui.exe`** (the TUI backend).

### Prerequisites
- **Windows OS** (for running native packaging tools).
- **WiX Toolset v3.11+**: [Download WiX](https://wixtoolset.org/releases/). Ensure `candle.exe`, `light.exe`, and `heat.exe` are added to your system `PATH`.
- **Inno Setup v6+ (Optional but Preferred)**: [Download Inno Setup](https://jrsoftware.org/isdl.php). Ensure `iscc.exe` is in your `PATH`.
- **Rust Toolchain**: `rustup target add x86_64-pc-windows-msvc`.
- **Node.js & pnpm**: To compile the frontend assets.

---

### Step-by-Step Build Process

#### Step 1: Compile the Rust Binaries
Compile the release binaries in the repository root:
```powershell
cargo build --release --features "cli-interactive"
```
This produces `xavier.exe` and `xavier-tui.exe` in `target/release/`.

#### Step 2: Build the Panel UI Frontend
Compile the production-ready React/Vite assets:
```powershell
cd panel-ui
pnpm install
pnpm build
```
This generates the compiled assets inside `panel-ui/build/` and mirrors them to `panel-ui/dist/` (via the postbuild script). The Windows installer expects these assets to be present next to the executable at runtime under `panel-ui/build/`.

#### Step 3: Run the Installer Script
Navigate to the `installer` directory and run the automated PowerShell script:
```powershell
cd ../installer
.\build-installer.ps1
```

The script automatically detects whether WiX or Inno Setup is installed and triggers the compilation:

##### Pathway A: WiX Toolset (.msi)
If `candle.exe` is found, the build runs:
1. **Harvesting Frontend Assets**:
   ```powershell
   heat.exe dir "..\panel-ui\build" -dr PANELUIFOLDER -cg PanelUIComponents -gg -sreg -sfrag -srd -out panel-ui-files.wxs
   ```
2. **Compiling**:
   ```powershell
   candle.exe xavier.wxs panel-ui-files.wxs
   ```
3. **Linking**:
   ```powershell
   light.exe xavier.wixobj panel-ui-files.wixobj -o XavierInstaller.msi
   ```
This generates **`XavierInstaller.msi`** in the `installer/` directory.

##### Pathway B: Inno Setup (.exe)
If `iscc.exe` is found, it compiles the script `setup.iss` directly:
```powershell
iscc.exe setup.iss
```
This generates **`XavierSetup.exe`** in the `installer/Output/` directory. This is the preferred Windows installer option as it offers robust handling of nested directory structures.

---

## 2. Tauri App Desktop Packaging (Linux & macOS)

The React frontend under `panel-ui/` can be bundled as a native Tauri desktop app, packing the core Rust backend (`xavier`) as an embedded, self-contained sidecar binary.

### Prerequisites
- **macOS or Linux Host**
- **System Dependencies**:
  - **Ubuntu/Debian**: `libwebkit2gtk-4.1-dev`, `build-essential`, `curl`, `wget`, `file`, `libssl-dev`, `libgtk-3-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`.
  - **Fedora**: `webkit2gtk4.1-devel`, `openssl-devel`, `gtk3-devel`, `libappindicator-gtk3-devel`.
- **Node.js & pnpm**

### The Sidecar Binary Convention
Tauri requires embedded binaries to match the host platform's target triple (e.g., `xavier-x86_64-unknown-linux-gnu` or `xavier-x86_64-apple-darwin`). These sidecar binaries must be placed in `panel-ui/src-tauri/binaries/` prior to compiling the Tauri app.

`panel-ui/src-tauri/build.rs` creates a **compile-time stub** when the host-triple sidecar is missing so `cargo check -p app` does not fail on a fresh checkout. That stub is **not** a shippable binary — release packaging must overwrite it with a real `xavier` build (steps below). CI core checks already use `--exclude app` (GTK/WebKit deps); desktop packaging is a separate path.

### Packaging steps
You can run the automated script from the repository root:
```bash
chmod +x scripts/build-tauri.sh
./scripts/build-tauri.sh
```

Alternatively, run the manual commands:

1. **Build the Xavier Backend**:
   ```bash
   cargo build --release --bin xavier
   ```
2. **Copy and Target-Suffix the Sidecar**:
   Determine your host target triple:
   ```bash
   TARGET_TRIPLE=$(rustc -Vv | grep host: | cut -d ' ' -f 2)
   ```
   Create the target directory and copy the binary:
   ```bash
   mkdir -p panel-ui/src-tauri/binaries
   cp target/release/xavier panel-ui/src-tauri/binaries/xavier-$TARGET_TRIPLE
   ```
3. **Build the Tauri Application Bundle**:
   ```bash
   cd panel-ui
   pnpm install
   pnpm tauri build
   ```

### Output Artifacts
Tauri compiles and packages the app, placing the production bundles under `panel-ui/src-tauri/target/release/bundle/`:
- **Linux**: `.deb`, `.rpm`, `.AppImage`
- **macOS**: `.dmg`, `.app` (with support for Apple Silicon/Universal Binaries depending on the compiler target)

---

## 3. Cross-Compilation (NixOS → Windows)

Cross-compiling Rust binaries (like `xavier`) from a NixOS host to Windows (`x86_64-pc-windows-gnu`) ensures that developers on NixOS can produce standalone Windows executable artifacts.

### Pathway A: Containerized Cross-Compilation (Recommended)

Using **`cross` (`cross-rs`)** is the most reliable method as it abstracts target SDKs inside pre-built Docker containers.

#### Prerequisites on NixOS:
1. **Enable Docker**: Ensure Docker daemon is running. In NixOS, this must be declared in your configuration (see `docs/ops/nixos-docker.md` for privilege elevation notes if your agent runs inside an unprivileged sandbox):
   ```nix
   virtualisation.docker.enable = true;
   users.users.<username>.extraGroups = [ "docker" ];
   ```
2. **Install Cross**:
   ```bash
   cargo install cross --git https://github.com/cross-rs/cross
   ```

#### Compilation Command:
Run the compilation targeting Windows GNU:
```bash
cross build --target x86_64-pc-windows-gnu --release --bin xavier
```
The output executable will be placed in `target/x86_64-pc-windows-gnu/release/xavier.exe`.

*Note: Since the GNU ABI is used, the produced binary is dynamically linked against the MinGW runtime. For maximum portability, consider copying the required MinGW DLLs or compiling via target `x86_64-pc-windows-msvc` (which requires linking tools handled by `cross`).*

---

### Pathway B: Host-Native Cross-Compilation (Nix-Shell)

If you prefer to avoid Docker, you can configure a host-native cross-compiler shell using Nix packages.

#### 1. Nix Shell Configuration (`cross-shell.nix`)
Create or enter a nix shell containing the Windows MinGW cross-compiler toolchain:

```nix
{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  nativeBuildInputs = [
    pkgs.pkg-config
    pkgs.rustup
  ];

  buildInputs = [
    # MinGW 64-bit compiler and headers
    pkgs.pkgsCross.mingwW64.buildPackages.gcc
    pkgs.pkgsCross.mingwW64.windows.pthreads
  ];

  shellHook = ''
    export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER="x86_64-w64-mingw32-gcc"
    export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_AR="x86_64-w64-mingw32-ar"
    echo "✅ Native MinGW Cross-Compile Shell Active"
  '';
}
```

#### 2. Configure Cargo Target
Add the following target specifications in `.cargo/config.toml` (or pass them as environment variables):
```toml
[target.x86_64-pc-windows-gnu]
linker = "x86_64-w64-mingw32-gcc"
ar = "x86_64-w64-mingw32-ar"
```

#### 3. Run Build
Ensure the rustup target is installed and run cargo build:
```bash
rustup target add x86_64-pc-windows-gnu
cargo build --target x86_64-pc-windows-gnu --release --bin xavier
```

---

## 4. Release Integration Flow

When orchestrating a full multi-platform release:
1. **Source Version bump**: Update version numbers across `Cargo.toml`, `panel-ui/package.json`, and `panel-ui/src-tauri/tauri.conf.json`.
2. **Build Web Assets**: Run `pnpm build` under `panel-ui`.
3. **Build Target Binaries**: Compile binaries natively or via cross-compilation.
4. **Trigger Packagers**: Assemble `.msi`/`.exe` (Windows), `.deb`/`AppImage` (Linux), and `.dmg` (macOS).
5. **Sanity Verification**: Use `scripts/release-smoke.sh` to spin up the packaged executables and verify standard HTTP, RAG Memory, and thread endpoints work properly.
