use axum::{
    extract::Path as AxumPath,
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const PANEL_BUILD_DIR: &str = "panel-ui/build";

/// Resolve the directory that contains the built Panel UI (`index.html` + `assets/`).
///
/// Priority (first match that exists wins):
/// 1. `XAVIER_PANEL_UI_DIR` env var
/// 2. `<exe_dir>/panel-ui/build` (portable installer layout)
/// 3. `<exe_dir>/panel-ui` (if index.html is at that root)
/// 4. `<cwd>/panel-ui/build`
/// 5. Compile-time `CARGO_MANIFEST_DIR/panel-ui/build` (dev checkout)
pub fn panel_ui_root() -> PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let mut candidates: Vec<PathBuf> = Vec::new();

        if let Ok(env_path) = std::env::var("XAVIER_PANEL_UI_DIR") {
            candidates.push(PathBuf::from(env_path));
        }

        if let Ok(exe) = std::env::current_exe() {
            if let Some(exe_dir) = exe.parent() {
                candidates.push(exe_dir.join("panel-ui").join("build"));
                candidates.push(exe_dir.join("panel-ui"));
                candidates.push(exe_dir.join("build"));
            }
        }

        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(cwd.join(PANEL_BUILD_DIR));
            candidates.push(cwd.join("panel-ui"));
        }

        candidates.push(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(PANEL_BUILD_DIR),
        );

        for candidate in candidates {
            if candidate.join("index.html").is_file() {
                return candidate;
            }
        }

        // Last resort: compile-time path (error message still points at build step).
        Path::new(env!("CARGO_MANIFEST_DIR")).join(PANEL_BUILD_DIR)
    })
    .clone()
}

pub async fn panel_index() -> impl IntoResponse {
    match tokio::fs::read_to_string(panel_build_path("index.html")).await {
        Ok(contents) => Html(contents).into_response(),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            format!(
                "Panel frontend assets are missing at {}. Build them first: cd panel-ui && pnpm install && pnpm run build \
(or set XAVIER_PANEL_UI_DIR / ship panel-ui/build next to xavier.exe)",
                panel_ui_root().display()
            ),
        )
            .into_response(),
    }
}

pub async fn panel_asset(AxumPath(path): AxumPath<String>) -> impl IntoResponse {
    let asset_path = panel_build_path(&format!("assets/{path}"));
    match tokio::fs::read(&asset_path).await {
        Ok(bytes) => asset_response(bytes, asset_content_type(&asset_path)),
        Err(_) => (StatusCode::NOT_FOUND, "Asset not found").into_response(),
    }
}

pub fn panel_build_path(relative: &str) -> PathBuf {
    panel_ui_root().join(relative)
}

pub fn asset_content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|value| value.to_str()) {
        Some("js") => "application/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("json") => "application/json; charset=utf-8",
        _ => "application/octet-stream",
    }
}

pub fn asset_response(bytes: Vec<u8>, content_type: &'static str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .body(axum::body::Body::from(bytes))
        .expect("test assertion")
}
