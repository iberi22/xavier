use std::path::{Path, PathBuf};
use axum::{
    extract::Path as AxumPath,
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
};

const PANEL_BUILD_DIR: &str = "panel-ui/build";

pub async fn panel_index() -> impl IntoResponse {
    match tokio::fs::read_to_string(panel_build_path("index.html")).await {
        Ok(contents) => Html(contents).into_response(),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "Panel frontend assets are missing. Build them first: cd panel-ui && npm install && npm run build",
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
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(PANEL_BUILD_DIR)
        .join(relative)
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
