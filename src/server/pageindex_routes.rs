//! PageIndex REST routes, nested under `/v1/pageindex` behind the same auth
//! middleware as the rest of the protected API.
//!
//! Thin wrappers over `pageindex_glue::tools`: bodies are the tool envelopes,
//! only the HTTP status is derived from them.

use std::sync::Arc;

use axum::{
    extract::{Path, Query},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
    Extension, Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::pageindex_glue::tools::{
    TOOL_BROWSE, TOOL_GET_DOCUMENT, TOOL_GET_PAGES, TOOL_GET_STRUCTURE, TOOL_INDEX,
};
use crate::pageindex_glue::{call, PageIndexState, ToolContext};
use crate::security::auth::{Claims, Permission};
use crate::workspace::WorkspaceContext;

const FALLBACK_WORKSPACE: &str = "default";

/// Build the PageIndex router; state is injected as an `Extension`.
pub fn router(state: Arc<PageIndexState>) -> Router {
    Router::new()
        .route("/documents", get(browse).post(ingest))
        .route(
            "/documents/{name}",
            get(get_document).delete(delete_document),
        )
        .route("/documents/{name}/structure", get(structure))
        .route("/documents/{name}/pages", get(pages))
        .layer(Extension(state))
}

#[derive(Debug, Deserialize)]
struct BrowseParams {
    query: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct StructureParams {
    max_depth: Option<usize>,
    node_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PagesParams {
    pages: Option<String>,
}

/// Caller identity: authenticated claims are mandatory, even for reads.
fn context(
    claims: &Option<Extension<Claims>>,
    workspace: &Option<Extension<WorkspaceContext>>,
) -> Result<(ToolContext, bool), Box<Response>> {
    let Some(Extension(claims)) = claims else {
        return Err(Box::new(
            (
                StatusCode::UNAUTHORIZED,
                Json(json!({"ok": false, "error": "authentication required",
                    "hint": "Send X-Xavier-Token or a Bearer token"})),
            )
                .into_response(),
        ));
    };
    let workspace = workspace
        .as_ref()
        .map(|Extension(w)| w.workspace_id.clone())
        .unwrap_or_else(|| FALLBACK_WORKSPACE.to_string());
    let ctx = ToolContext {
        workspace,
        can_write: claims.role.can_add_memory(),
    };
    Ok((ctx, claims.role.can_delete_memory()))
}

fn status_for(envelope: &Value) -> StatusCode {
    if envelope.get("ok").and_then(Value::as_bool) == Some(true) {
        return StatusCode::OK;
    }
    let err = envelope
        .get("error")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if err.starts_with("permission denied") {
        StatusCode::FORBIDDEN
    } else if err.starts_with("not found") {
        StatusCode::NOT_FOUND
    } else if err.starts_with("storage")
        || err.starts_with("internal")
        || err.contains("task failed")
    {
        StatusCode::INTERNAL_SERVER_ERROR
    } else {
        StatusCode::BAD_REQUEST
    }
}

fn respond(envelope: Value) -> Response {
    (status_for(&envelope), Json(envelope)).into_response()
}

async fn run(
    state: &Arc<PageIndexState>,
    tool: &str,
    args: Value,
    claims: &Option<Extension<Claims>>,
    workspace: &Option<Extension<WorkspaceContext>>,
) -> Response {
    match context(claims, workspace) {
        Ok((ctx, _)) => respond(call(state, tool, &args, &ctx).await),
        Err(resp) => *resp,
    }
}

async fn browse(
    Extension(state): Extension<Arc<PageIndexState>>,
    claims: Option<Extension<Claims>>,
    workspace: Option<Extension<WorkspaceContext>>,
    Query(p): Query<BrowseParams>,
) -> Response {
    let mut args = Map::new();
    if let Some(q) = p.query {
        args.insert("query".into(), json!(q));
    }
    if let Some(l) = p.limit {
        args.insert("limit".into(), json!(l));
    }
    if let Some(o) = p.offset {
        args.insert("offset".into(), json!(o));
    }
    run(
        &state,
        TOOL_BROWSE,
        Value::Object(args),
        &claims,
        &workspace,
    )
    .await
}

async fn get_document(
    Extension(state): Extension<Arc<PageIndexState>>,
    claims: Option<Extension<Claims>>,
    workspace: Option<Extension<WorkspaceContext>>,
    Path(name): Path<String>,
) -> Response {
    let args = json!({"doc_name": name});
    run(&state, TOOL_GET_DOCUMENT, args, &claims, &workspace).await
}

async fn structure(
    Extension(state): Extension<Arc<PageIndexState>>,
    claims: Option<Extension<Claims>>,
    workspace: Option<Extension<WorkspaceContext>>,
    Path(name): Path<String>,
    Query(p): Query<StructureParams>,
) -> Response {
    let mut args = Map::new();
    args.insert("doc_name".into(), json!(name));
    if let Some(d) = p.max_depth {
        args.insert("max_depth".into(), json!(d));
    }
    if let Some(n) = p.node_id {
        args.insert("node_id".into(), json!(n));
    }
    run(
        &state,
        TOOL_GET_STRUCTURE,
        Value::Object(args),
        &claims,
        &workspace,
    )
    .await
}

async fn pages(
    Extension(state): Extension<Arc<PageIndexState>>,
    claims: Option<Extension<Claims>>,
    workspace: Option<Extension<WorkspaceContext>>,
    Path(name): Path<String>,
    Query(p): Query<PagesParams>,
) -> Response {
    let mut args = Map::new();
    args.insert("doc_name".into(), json!(name));
    if let Some(pages) = p.pages {
        args.insert("pages".into(), json!(pages));
    }
    run(
        &state,
        TOOL_GET_PAGES,
        Value::Object(args),
        &claims,
        &workspace,
    )
    .await
}

/// Body: `{name, path|content, format?, summarize?, optimize?}`.
async fn ingest(
    Extension(state): Extension<Arc<PageIndexState>>,
    claims: Option<Extension<Claims>>,
    workspace: Option<Extension<WorkspaceContext>>,
    Json(body): Json<Value>,
) -> Response {
    let mut args = match body {
        Value::Object(m) => m,
        _ => {
            return respond(json!({"ok": false, "error": "body must be a JSON object",
                "hint": "Send {name, content|path, format?, summarize?, optimize?}"}))
        }
    };
    if let Some(name) = args.remove("name") {
        args.insert("doc_name".into(), name);
    }
    run(&state, TOOL_INDEX, Value::Object(args), &claims, &workspace).await
}

async fn delete_document(
    Extension(state): Extension<Arc<PageIndexState>>,
    claims: Option<Extension<Claims>>,
    workspace: Option<Extension<WorkspaceContext>>,
    Path(name): Path<String>,
) -> Response {
    let (ctx, can_delete) = match context(&claims, &workspace) {
        Ok(v) => v,
        Err(resp) => return *resp,
    };
    if !can_delete {
        return respond(json!({"ok": false,
            "error": "permission denied: deleting documents requires write access",
            "hint": "Use a role that may delete memory"}));
    }
    let doc = name.clone();
    let joined = tokio::task::spawn_blocking(move || {
        let idx = state.index()?;
        idx.delete(&ctx.workspace, &doc)
    })
    .await;
    match joined {
        Ok(Ok(())) => respond(json!({"ok": true, "deleted": name})),
        Ok(Err(e)) => {
            let msg = e.to_string();
            let hint = "Check the name with GET /v1/pageindex/documents";
            respond(json!({"ok": false, "error": msg, "hint": hint}))
        }
        Err(e) => respond(json!({"ok": false, "error": format!("task failed: {e}"),
            "hint": "retry; report if it persists"})),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pageindex_glue::PageIndexSettings;
    use crate::security::auth::UserRole;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    use xavier_pageindex::store::SqliteStore;

    fn app(role: Option<UserRole>) -> Router {
        let state = Arc::new(PageIndexState::with_store(
            PageIndexSettings::default(),
            SqliteStore::open_in_memory().unwrap(),
        ));
        let r = router(state);
        match role {
            Some(role) => r.layer(Extension(Claims::new(
                "u".into(),
                "u@x.dev".into(),
                role,
                chrono::Duration::hours(1),
            ))),
            None => r,
        }
    }

    async fn send(
        app: Router,
        method: &str,
        uri: &str,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(uri);
        let body = match body {
            Some(b) => {
                req = req.header("content-type", "application/json");
                Body::from(b.to_string())
            }
            None => Body::empty(),
        };
        let resp = app.oneshot(req.body(body).unwrap()).await.unwrap();
        let status = resp.status();
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    #[tokio::test]
    async fn test_pageindex_routes_browse_ok() {
        let a = app(Some(UserRole::User));
        let body = json!({"name": "guide", "content": "# One\nalpha\n## Two\nbeta\n", "format": "markdown"});
        let (st, v) = send(a.clone(), "POST", "/documents", Some(body)).await;
        assert_eq!(st, StatusCode::OK, "{v}");
        assert_eq!(v["ok"], true);

        let (st, v) = send(a.clone(), "GET", "/documents?limit=5", None).await;
        assert_eq!(st, StatusCode::OK, "{v}");
        assert_eq!(v["total"], 1, "{v}");

        let (st, v) = send(
            a.clone(),
            "GET",
            "/documents/guide/structure?max_depth=1",
            None,
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{v}");
        let (st, v) = send(a.clone(), "GET", "/documents/guide/pages?pages=1", None).await;
        assert_eq!(st, StatusCode::OK, "{v}");
        let (st, v) = send(a, "DELETE", "/documents/guide", None).await;
        assert_eq!(st, StatusCode::OK, "{v}");
    }

    #[tokio::test]
    async fn test_pageindex_routes_structure_404_unknown_doc() {
        let a = app(Some(UserRole::User));
        let (st, v) = send(a, "GET", "/documents/missing/structure", None).await;
        assert_eq!(st, StatusCode::NOT_FOUND, "{v}");
        assert_eq!(v["ok"], false);
    }

    #[tokio::test]
    async fn test_pageindex_routes_require_auth() {
        let (st, _) = send(app(None), "GET", "/documents", None).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED);
        let body = json!({"name": "a", "content": "x"});
        let (st, _) = send(app(None), "POST", "/documents", Some(body.clone())).await;
        assert_eq!(st, StatusCode::UNAUTHORIZED);

        let (st, v) = send(
            app(Some(UserRole::Readonly)),
            "POST",
            "/documents",
            Some(body),
        )
        .await;
        assert_eq!(st, StatusCode::FORBIDDEN, "{v}");
        let (st, _) = send(
            app(Some(UserRole::Readonly)),
            "DELETE",
            "/documents/a",
            None,
        )
        .await;
        assert_eq!(st, StatusCode::FORBIDDEN);
    }
}
