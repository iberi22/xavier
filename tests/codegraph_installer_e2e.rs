//! Comprehensive E2E tests for CodeGraph CLI install, binary resolution, and HTTP sidecar status.

use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::TempDir;
use tower::ServiceExt;

use xavier::codebase::codegraph_sidecar::{
    ensure_codegraph_sidecar, ensure_codegraph_sidecar_soft, EnsureOptions, InstallMode,
};

/// Helper to resolve binary location in system PATH or custom search paths.
fn resolve_sidecar_binary(binary_name: &str, custom_path: Option<&Path>) -> Option<PathBuf> {
    if let Some(path_dir) = custom_path {
        let bin = path_dir.join(binary_name);
        if bin.exists() {
            return Some(bin);
        }
    }

    if let Ok(path_env) = env::var("PATH") {
        for dir in env::split_paths(&path_env) {
            let candidate = dir.join(binary_name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    None
}

/// Helper simulating binary installation logic to a destination folder with symlinks.
fn simulate_sidecar_installation(
    source_dir: &Path,
    bin_dir: &Path,
    binary_name: &str,
) -> std::io::Result<(PathBuf, PathBuf)> {
    fs::create_dir_all(source_dir)?;
    fs::create_dir_all(bin_dir)?;

    let source_bin_path = source_dir.join(binary_name);
    let mut file = File::create(&source_bin_path)?;
    writeln!(file, "#!/bin/sh\necho colby-sidecar v1.0.0")?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = file.metadata()?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&source_bin_path, perms)?;
    }

    let install_bin_path = bin_dir.join(binary_name);
    let symlink_path = bin_dir.join("colby-codegraph");

    fs::copy(&source_bin_path, &install_bin_path)?;

    #[cfg(unix)]
    {
        if symlink_path.exists() {
            fs::remove_file(&symlink_path)?;
        }
        std::os::unix::fs::symlink(&install_bin_path, &symlink_path)?;
    }

    Ok((install_bin_path, symlink_path))
}

/// Handler for `GET /code/sidecar/status`.
async fn code_sidecar_status_handler() -> Json<Value> {
    let workspace = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let outcome = ensure_codegraph_sidecar_soft(&workspace);

    Json(json!({
        "status": "ok",
        "sidecar": {
            "available": outcome.available,
            "message": outcome.message,
            "bin_path": outcome.bin_path.as_ref().map(|p| p.to_string_lossy()),
            "resolved_via": if outcome.available { "path" } else { "none" }
        }
    }))
}

// ─────────────────────────────────────────────────────────────────────────────
// Section A: Resolution and Health Verification
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_codegraph_sidecar_resolution_and_health() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let workspace = temp_dir.path();

    // 1. Verify default soft ensure outcome when mock-disabled
    let soft_outcome = ensure_codegraph_sidecar_soft(workspace);
    assert!(!soft_outcome.available);
    assert!(soft_outcome.message.contains("mock-disabled"));
    assert!(soft_outcome.bin_path.is_none());

    // 2. Verify ensure with custom options
    let opts = EnsureOptions {
        reprompt: true,
        install_mode: InstallMode::Ask,
    };
    let outcome = ensure_codegraph_sidecar(workspace, opts);
    assert!(!outcome.available);
    assert_eq!(outcome.bin_path, None);

    // 3. Test helper resolution logic for custom binary path
    let bin_name = "colby-sidecar-dummy";
    let mock_bin_dir = temp_dir.path().join("bin");
    fs::create_dir_all(&mock_bin_dir).expect("Failed to create mock bin dir");

    let fake_bin = mock_bin_dir.join(bin_name);
    File::create(&fake_bin).expect("Failed to create dummy binary");

    let resolved = resolve_sidecar_binary(bin_name, Some(&mock_bin_dir));
    assert!(resolved.is_some());
    assert_eq!(resolved.unwrap(), fake_bin);

    let unresolved = resolve_sidecar_binary("nonexistent-sidecar-binary-xyz", Some(&mock_bin_dir));
    assert!(unresolved.is_none());
}

// ─────────────────────────────────────────────────────────────────────────────
// Section B: Install from Source & Symlink Simulation
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_codegraph_install_from_source_simulation() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let source_dir = temp_dir.path().join("source");
    let install_bin_dir = temp_dir.path().join("installed_bin");
    let binary_name = "colby-sidecar";

    let (installed_bin, symlink) =
        simulate_sidecar_installation(&source_dir, &install_bin_dir, binary_name)
            .expect("Failed sidecar installation simulation");

    assert!(installed_bin.exists());

    #[cfg(unix)]
    {
        assert!(symlink.exists());
        let meta = fs::symlink_metadata(&symlink).expect("symlink metadata");
        assert!(meta.file_type().is_symlink());

        let target = fs::read_link(&symlink).expect("read symlink target");
        assert_eq!(target, installed_bin);
    }

    // Verify binary discovery from the installed directory
    let resolved = resolve_sidecar_binary(binary_name, Some(&install_bin_dir));
    assert!(resolved.is_some());
    assert_eq!(resolved.unwrap(), installed_bin);
}

// ─────────────────────────────────────────────────────────────────────────────
// Section C: HTTP Sidecar Status Endpoint Verification
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_codegraph_http_status_endpoint() {
    let app = Router::new().route("/code/sidecar/status", get(code_sidecar_status_handler));

    let request = Request::builder()
        .uri("/code/sidecar/status")
        .method("GET")
        .body(Body::empty())
        .expect("Failed to build request");

    let response = app.oneshot(request).await.expect("Failed to send request");

    assert_eq!(response.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("Failed to read body bytes");

    let json_val: Value = serde_json::from_slice(&body_bytes).expect("Failed to parse JSON");

    assert_eq!(json_val["status"], "ok");
    assert!(json_val["sidecar"].is_object());
    assert_eq!(json_val["sidecar"]["available"], false);
    assert!(json_val["sidecar"]["message"]
        .as_str()
        .unwrap()
        .contains("mock-disabled"));
}
