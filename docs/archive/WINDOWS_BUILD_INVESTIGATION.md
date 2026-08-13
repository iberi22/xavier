# Xavier Windows Native Build Investigation

> **Date:** 2026-05-25
> **Investigator:** BELA (Claw)
> **Status:** BLOCKED — `libsql-ffi` incompatible with Windows native build

---

## Problem Summary

Xavier cannot be compiled as a native Windows EXE because its core dependency `libsql` → `libsql-ffi` uses Unix-only build tools (`cp`, `make`, `./configure`) in its `build.rs` that do not exist in Windows native environments.

---

## Root Cause Analysis

### The `libsql-ffi` Build Script

File: `~/.cargo/registry/src/.../libsql-ffi-0.5.0/build.rs`

The build script requires these Unix tools:

| Tool | Line | Purpose | Windows Equivalent |
|------|------|---------|-------------------|
| `cp` | 42 | Copy bindgen files | `copy` / `xcopy` |
| `make` | 67, 78 | Compile SQLite3 C code | `nmake` / MSBuild |
| `./configure` | 73 | Configure SQLite3 build | Manual config |
| `cmake` | 356 | Build SQLite3MultipleCiphers | ✅ Available |

The first failure point is line 42-48:
```rust
Command::new("cp")
    .arg("--no-preserve=mode,ownership")
    .arg("-R")
    .arg(format!("{dir}/{bindgen_rs_path}"))
    .arg(out_path)
    .unwrap();  // ← PANICS with "program not found"
```

### Error Message

```
thread 'main' panicked at .../libsql-ffi-0.5.0/build.rs:48:10:
called `Result::unwrap()` on an `Err` value: Error { kind: NotFound, message: "program not found" }
```

---

## What Was Tried

### Attempt 1: Direct `cargo build`
**Result:** ❌ FAIL — `libsql-ffi` NotFound error

### Attempt 2: Make `libsql` optional with feature flag
**Result:** ❌ ABANDONED — Would require massive refactoring of all memory backends

### Attempt 3: Install missing Unix tools from Git Bash + Strawberry Perl
**Tools found/provided:**
- ✅ `cp.exe` — Git Bash `C:\Program Files\Git\usr\bin\cp.exe`
- ✅ `gcc.exe` — Strawberry Perl MinGW `C:\Strawberry\c\bin\gcc.exe`
- ✅ `make.exe` — Created from `mingw32-make.exe` copy
- ✅ `bash.exe` — Git Bash / MSYS
- ⚠️ `cl.exe` — VS2022 BuildTools installed but NOT in PATH

**Result:** ⚠️ Build starts but hangs indefinitely (no output after 6+ minutes)

### Attempt 4: Build Docker image
**Result:** ✅ WORKS — Xavier runs in Docker on port 8006

---

## System Inventory

### Available Build Tools

| Tool | Path | Status |
|------|------|--------|
| Rust | `rustc 1.86.0` | ✅ Working |
| Cargo | `cargo 1.86.0` | ✅ Working |
| CMake | `cmake version 3.29.2` | ✅ Working |
| Ninja | `ninja version 1.12.0` | ✅ Working |
| Git Bash `cp` | `C:\Program Files\Git\usr\bin\cp.exe` | ✅ Available |
| Git Bash `bash` | `C:\Program Files\Git\usr\bin\bash.exe` | ✅ Available |
| GCC (MinGW) | `C:\Strawberry\c\bin\gcc.exe` | ✅ Available |
| `make` (copy) | `C:\Strawberry\c\bin\make.exe` | ✅ Created |
| MSVC `cl` | VS2022 BuildTools | ⚠️ Not in PATH |
| MSYS2 | Not installed | ❌ Missing |

---

## Why It Fails on Windows

The `libsql-ffi` crate is designed for Unix-like environments. It:

1. **Hardcodes Unix commands** (`cp`, `make`, `./configure`) without Windows fallbacks
2. **Compiles SQLite from C source** using Makefiles, not CMake
3. **Uses shell scripts** (`./configure`) that require `sh`/`bash`
4. **Does not use `cc` crate properly** for cross-platform C compilation

This is a known upstream issue. The `libsql` project is primarily developed for Linux/Docker environments.

---

## Workarounds & Solutions

### Option A: Install MSYS2 (Recommended for Native Build)

**Effort:** ~15 min install + retry build
**Result:** Native Windows EXE

MSYS2 provides a full Unix-like environment on Windows including:
- `pacman` package manager
- `make`, `cp`, `bash` natively
- MinGW-w64 GCC compiler
- All tools `libsql-ffi` expects

**Steps:**
1. Download and install MSYS2 from https://www.msys2.org/
2. Open MSYS2 terminal
3. `pacman -S make gcc`
4. Add MSYS2 to PATH
5. Retry `cargo build`

### Option B: Use WSL2 (Alternative)

**Effort:** ~30 min
**Result:** Linux binary that can be copied to Windows

1. Open WSL2 Ubuntu
2. `sudo apt update && sudo apt install build-essential cmake`
3. `cargo build --release --target x86_64-pc-windows-msvc`
4. Copy `target/release/xavier.exe` to Windows

### Option C: Continue Using Docker

**Effort:** 0 (already working)
**Result:** Xavier runs in container

Docker is the officially supported deployment method per README:
```powershell
docker run -p 8006:8006 -v xavier-data:/data ghcr.io/iberi22/xavier:latest
```

### Option D: Wait for Upstream Fix

**Effort:** 0
**Result:** Nothing changes

Request `libsql` team to fix Windows support in `libsql-ffi`.

---

## Recommendation

**For immediate use:** Continue with Docker ✅ (already working)

**For native EXE:** Install MSYS2 and retry build. This is the most viable path to a native Windows binary without code changes.

**For CI/CD:** Use WSL2 or cross-compile from Linux.

---

## Files Modified During Investigation

- `Cargo.toml` — Feature flag experiments (reverted)
- `src/utils/db_compat.rs` — Created then deleted
- `src/utils/mod.rs` — Reverted
- `C:\Strawberry\c\bin\make.exe` — Created from `mingw32-make.exe`
- `feedback/auto-save-feedback-2026-05-25.json` — Feedback saved

---

## Related Documentation

- `build-local.ps1` — Build script for local compilation (also targets Linux cross-compile)
- `docs/DOCKER_DEPLOY.md` — Docker deployment guide
- `README.md` — Primary install method is Docker or `cargo install` (which also fails on Windows for same reason)

---

*Investigation completed 2026-05-25. Next step: Install MSYS2 if native EXE is required.*
