use crate::cli::state::CliState;
use axum::{
    extract::{Json, Path, Query, State},
    response::IntoResponse,
};
use xavier_lib::server::headless;

pub async fn health() -> impl IntoResponse {
    headless::routes::health().await
}

pub async fn context(
    State(state): State<CliState>,
    Query(params): Query<headless::routes::ContextParams>,
) -> impl IntoResponse {
    headless::routes::context(state.memory.as_ref(), params).await
}

pub async fn memory_search(
    State(state): State<CliState>,
    Json(req): Json<headless::routes::SearchRequest>,
) -> impl IntoResponse {
    headless::routes::memory_search(state.memory.as_ref(), req).await
}

pub async fn tools() -> impl IntoResponse {
    headless::routes::tools().await
}

pub async fn execute_tool(
    Path(name): Path<String>,
    Json(req): Json<headless::routes::ToolExecuteRequest>,
) -> impl IntoResponse {
    headless::routes::execute_tool(name, req).await
}

pub async fn provider_status(State(state): State<CliState>) -> impl IntoResponse {
    let router = state.provider_router.read().await;
    let active = router.current_provider().as_str().to_string();
    headless::routes::provider_status(active).await
}
