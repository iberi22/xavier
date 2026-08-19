//! Tauri build script.
//!
//! `tauri_build` validates that every `bundle.externalBin` exists for the
//! current target triple even on `cargo check` (not only on bundle). The
//! Xavier sidecar is produced by release scripts and is often absent on a
//! fresh Linux checkout (only Windows sidecars are committed). When missing,
//! we create a compile-time placeholder and warn — real packaging still
//! requires copying the built `xavier` binary (see docs/ops/release-packaging.md).

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    ensure_sidecar_placeholder();
    tauri_build::build()
}

fn ensure_sidecar_placeholder() {
    println!("cargo:rerun-if-changed=binaries");
    println!("cargo:rerun-if-env-changed=TARGET");

    let Ok(target) = env::var("TARGET") else {
        return;
    };

    let exe = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let path = manifest_dir
        .join("binaries")
        .join(format!("xavier-{target}{exe}"));

    if path.exists() {
        return;
    }

    println!(
        "cargo:warning=Tauri sidecar `{}` missing; creating compile-time stub. \
         For a real desktop bundle, copy the built xavier binary \
         (see docs/ops/release-packaging.md or scripts/release-build.sh).",
        path.display()
    );

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    // Minimal placeholder so tauri-build existence checks pass on cargo check/clippy.
    // Release packaging must overwrite this with a real `xavier` binary.
    let _ = fs::write(
        &path,
        b"#!/bin/sh\necho 'xavier sidecar stub - replace with real binary' >&2\nexit 1\n",
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(&path) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = fs::set_permissions(&path, perms);
        }
    }
}
