//! Onboarding handlers for context-aware suggestions.

use crate::cli::onboarding::generate_suggestions;
use crate::cli::state::CliState;
use axum::{extract::State, Json};

/// Onboarding suggestions handler.
pub async fn onboarding_suggestions_handler(
    State(state): State<CliState>,
) -> impl axum::response::IntoResponse {
    let suggestions = generate_suggestions(&state.workspace_dir);
    Json(suggestions)
}
