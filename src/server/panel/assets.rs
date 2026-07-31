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

/// Panel index.
pub async fn panel_index() -> impl IntoResponse {
    match tokio::fs::read_to_string(panel_build_path("index.html")).await {
        Ok(contents) => Html(contents).into_response(),
        Err(_) => {
            let root = panel_ui_root();
            let html = format!(
                r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8"/>
  <meta name="viewport" content="width=device-width, initial-scale=1"/>
  <title>Xavier Panel — assets missing</title>
  <style>
    body {{ font-family: ui-sans-serif, system-ui, sans-serif; margin: 0; min-height: 100vh;
      background: #0b1220; color: #e6edf3; display: grid; place-items: center; }}
    main {{ max-width: 40rem; padding: 2rem; border: 1px solid #30363d; border-radius: 12px;
      background: #161b22; }}
    code {{ background: #21262d; padding: 0.15rem 0.4rem; border-radius: 6px; }}
    pre {{ background: #21262d; padding: 1rem; border-radius: 8px; overflow: auto; }}
    h1 {{ margin-top: 0; font-size: 1.35rem; }}
    p {{ line-height: 1.5; color: #c9d1d9; }}
  </style>
</head>
<body>
  <main>
    <h1>Panel frontend assets are missing</h1>
    <p>Expected <code>index.html</code> under:</p>
    <pre>{root}</pre>
    <p>Build the UI, then restart Xavier:</p>
    <pre>cd panel-ui &amp;&amp; pnpm install &amp;&amp; pnpm run build</pre>
    <p>Or point Xavier at an existing build with <code>XAVIER_PANEL_UI_DIR</code>
       / ship <code>panel-ui/build</code> next to the binary.</p>
  </main>
</body>
</html>"#,
                root = root.display()
            );
            (
                StatusCode::SERVICE_UNAVAILABLE,
                [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
                html,
            )
                .into_response()
        }
    }
}

/// Panel asset.
pub async fn panel_asset(AxumPath(path): AxumPath<String>) -> impl IntoResponse {
    let asset_path = panel_build_path(&format!("assets/{path}"));
    match tokio::fs::read(&asset_path).await {
        Ok(bytes) => asset_response(bytes, asset_content_type(&asset_path)),
        Err(_) => (StatusCode::NOT_FOUND, "Asset not found").into_response(),
    }
}

/// Panel build path.
pub fn panel_build_path(relative: &str) -> PathBuf {
    panel_ui_root().join(relative)
}

/// Asset content type.
pub fn asset_content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|value| value.to_str()) {
        Some("js") => "application/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("json") => "application/json; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Asset response.
pub fn asset_response(bytes: Vec<u8>, content_type: &'static str) -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .body(axum::body::Body::from(bytes))
        .expect("test assertion")
}
