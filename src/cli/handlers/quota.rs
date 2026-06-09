use crate::cli::handlers::json_response;
use crate::cli::state::CliState;
use anyhow::Result;
use axum::{extract::State, http::StatusCode, response::Response};
use chrono::Utc;
use xavier::domain::proxy::types::ProviderQuota;

pub async fn handle_quota_command() -> Result<()> {
    let base_url = crate::cli::config::resolve_base_url();
    let token = crate::cli::config::require_xavier_token()?;
    let client = crate::cli::commands::enums::CLI_HTTP_CLIENT.clone();

    let resp = client
        .get(format!("{}/v1/providers/quota", base_url))
        .header("X-Xavier-Token", &token)
        .send()
        .await?;

    if resp.status().is_success() {
        let quotas: Vec<ProviderQuota> = resp.json().await?;

        println!(
            "{:<15} {:<12} {:<18} {:<18} {:<20}",
            "Provider", "Tier", "Req Remaining", "Tok Remaining", "Resets At"
        );
        println!("{}", "-".repeat(85));

        for q in quotas {
            let provider = q.provider.as_str();
            let tier = format!("{:?}", q.api_tier);
            let req_rem = q
                .requests_remaining
                .map(|r: u64| r.to_string())
                .unwrap_or_else(|| "-".to_string());
            let tok_rem = q
                .tokens_remaining
                .map(|t: u64| t.to_string())
                .unwrap_or_else(|| "-".to_string());
            let resets = q
                .resets_at
                .map(|dt: chrono::DateTime<chrono::Utc>| {
                    let duration = dt.signed_duration_since(Utc::now());
                    if duration.num_seconds() > 0 {
                        format!("in {}s", duration.num_seconds())
                    } else {
                        "Now".to_string()
                    }
                })
                .unwrap_or_else(|| "-".to_string());

            println!(
                "{:<15} {:<12} {:<18} {:<18} {:<20}",
                provider, tier, req_rem, tok_rem, resets
            );
        }
    } else {
        println!("❌ Failed to fetch quotas: {}", resp.text().await?);
    }

    Ok(())
}

pub async fn v1_providers_quota(State(state): State<CliState>) -> Response {
    match state.rate_manager.get_all_quotas().await {
        Ok(quotas) => json_response(
            StatusCode::OK,
            serde_json::to_value(quotas).unwrap_or_default(),
        ),
        Err(e) => json_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            serde_json::json!({ "error": e.to_string() }),
        ),
    }
}
