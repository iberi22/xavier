use axum::{
    extract::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::adapters::inbound::http::routes::{
    espacio_disabled, espacio_error_response, espacio_forbidden, espacio_join_error,
    get_space_manager, run_blocking, RootCredential,
};
use crate::espacio::invite::SpaceRole;
use crate::espacio::SpaceManager;

#[derive(Deserialize)]
pub struct CreateInviteRequest {
    pub target_node: String,
    pub role: String,
    #[serde(default = "default_ttl")]
    pub ttl_hours: i64,
}

fn default_ttl() -> i64 {
    24
}

#[derive(Deserialize)]
pub struct AcceptInviteRequest {
    pub member_display: String,
}

#[derive(Serialize)]
pub struct AcceptInviteResponse {
    pub member_id: String,
    pub role: SpaceRole,
}

pub fn espacio_invite_routes<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route(
            "/spaces/{id}/invites",
            post(espacio_invite_create_handler).get(espacio_invite_list_handler),
        )
        .route(
            "/invites/{invite_id}/accept",
            post(espacio_invite_accept_handler),
        )
}

pub async fn espacio_invite_create_handler(
    extension: Option<axum::extract::Extension<Arc<SpaceManager>>>,
    root: Option<axum::extract::Extension<RootCredential>>,
    auth: Option<axum::extract::Extension<crate::espacio::SpaceAuth>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(req): Json<CreateInviteRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    let has_access = match auth {
        Some(a) => {
            a.0.space_id == id
                && crate::espacio::permissions::can(
                    a.0.role,
                    crate::espacio::permissions::SpaceAction::ManageMembers,
                )
        }
        None => root.is_some(),
    };
    if !has_access || (auth.is_some() && root.is_none()) {
        // Refuse space tokens until body-e-inviteb
        return espacio_forbidden();
    }

    if req.ttl_hours < 1 || req.ttl_hours > 720 {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"status":"error","message":"ttl_hours must be 1..=720"})),
        )
            .into_response();
    }

    let role = match req.role.as_str() {
        "admin" => SpaceRole::Admin,
        "moderator" => SpaceRole::Moderator,
        "member" => SpaceRole::Member,
        "reader" => SpaceRole::Reader,
        _ => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"status":"error","message":"invalid role"})),
            )
                .into_response()
        }
    };

    let Some(manager) = extension.map(|e| e.0).or_else(get_space_manager) else {
        return espacio_disabled();
    };

    let inviter_node = if let Some(a) = auth {
        a.0.member_id.clone()
    } else {
        "root".to_string()
    };

    let expires_at = chrono::Utc::now() + chrono::Duration::hours(req.ttl_hours);

    match run_blocking(async move {
        manager.get(&id).await?;
        manager
            .invites()
            .create_with_expiry(id, inviter_node, req.target_node, role, expires_at)
            .await
    })
    .await
    {
        Some(Ok(invite)) => (
            axum::http::StatusCode::CREATED,
            Json(serde_json::to_value(invite).unwrap_or_default()),
        )
            .into_response(),
        Some(Err(err)) => espacio_error_response(err),
        None => espacio_join_error(),
    }
}

pub async fn espacio_invite_list_handler(
    extension: Option<axum::extract::Extension<Arc<SpaceManager>>>,
    root: Option<axum::extract::Extension<RootCredential>>,
    auth: Option<axum::extract::Extension<crate::espacio::SpaceAuth>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    let has_access = match auth {
        Some(a) => {
            a.0.space_id == id
                && crate::espacio::permissions::can(
                    a.0.role,
                    crate::espacio::permissions::SpaceAction::Read,
                )
        }
        None => root.is_some(),
    };
    if !has_access || (auth.is_some() && root.is_none()) {
        // Refuse space tokens until body-e-inviteb
        return espacio_forbidden();
    }

    let Some(manager) = extension.map(|e| e.0).or_else(get_space_manager) else {
        return espacio_disabled();
    };

    match run_blocking(async move {
        manager.get(&id).await?;
        Ok::<_, anyhow::Error>(manager.invites().list_for_space(&id).await)
    })
    .await
    {
        Some(Ok(invites)) => {
            Json(serde_json::to_value(invites).unwrap_or_default()).into_response()
        }
        Some(Err(err)) => espacio_error_response(err),
        None => espacio_join_error(),
    }
}

pub async fn espacio_invite_accept_handler(
    extension: Option<axum::extract::Extension<Arc<SpaceManager>>>,
    root: Option<axum::extract::Extension<RootCredential>>,
    auth: Option<axum::extract::Extension<crate::espacio::SpaceAuth>>,
    axum::extract::Path(invite_id): axum::extract::Path<String>,
    Json(req): Json<AcceptInviteRequest>,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    if root.is_none() {
        // Refuse space tokens until body-e-inviteb
        return espacio_forbidden();
    }

    let Some(manager) = extension.map(|e| e.0).or_else(get_space_manager) else {
        return espacio_disabled();
    };

    match run_blocking(async move {
        manager
            .invites()
            .accept_invite(&invite_id, &req.member_display, None, false)
            .await
    })
    .await
    {
        Some(Ok(accepted)) => (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({
                "member_id": accepted.member_id,
                "role": accepted.role.as_str(),
                "token_id": accepted.token_id,
                "token": accepted.token,
            })),
        )
            .into_response(),
        Some(Err(err)) => {
            if err.to_string().contains("already used") {
                return (
                    axum::http::StatusCode::CONFLICT,
                    Json(serde_json::json!({"status":"error","message":err.to_string()})),
                )
                    .into_response();
            } else if err.to_string().contains("expired") {
                return (
                    axum::http::StatusCode::BAD_REQUEST, // Or 410 Gone? Prompt says 4xx.
                    Json(serde_json::json!({"status":"error","message":err.to_string()})),
                )
                    .into_response();
            } else if err.to_string().contains("not found") {
                return (
                    axum::http::StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"status":"error","message":err.to_string()})),
                )
                    .into_response();
            }
            espacio_error_response(err)
        }
        None => espacio_join_error(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::espacio::SpaceRole;
    use axum::http::StatusCode;

    async fn setup_test_space() -> (tempfile::TempDir, Arc<SpaceManager>) {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = crate::cli::state::open_space_manager_with_kek(
            tmp.path(),
            Arc::new(crate::espacio::MasterNodeKek::for_test()),
        )
        .unwrap();
        let _ = mgr
            .create("space-1", "admin", "admin-pwd", SpaceRole::Admin)
            .await
            .unwrap();
        (tmp, mgr)
    }

    #[tokio::test]
    async fn create_then_accept_issues_one_token() {
        let (_tmp, mgr) = setup_test_space().await;
        let ext = Some(axum::extract::Extension(mgr.clone()));
        let root = Some(axum::extract::Extension(RootCredential));

        let res = espacio_invite_create_handler(
            ext.clone(),
            root.clone(),
            None,
            axum::extract::Path("space-1".to_string()),
            Json(CreateInviteRequest {
                target_node: "new-member".to_string(),
                role: "member".to_string(),
                ttl_hours: 24,
            }),
        )
        .await;
        assert_eq!(res.status(), StatusCode::CREATED);

        let body_bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .unwrap();
        let invite: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
        let invite_id = invite["id"].as_str().unwrap().to_string();

        let res_accept = espacio_invite_accept_handler(
            ext.clone(),
            root.clone(),
            None,
            axum::extract::Path(invite_id.clone()),
            Json(AcceptInviteRequest {
                member_display: "new-member".to_string(),
            }),
        )
        .await;
        assert_eq!(res_accept.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn second_accept_is_refused() {
        let (_tmp, mgr) = setup_test_space().await;
        let ext = Some(axum::extract::Extension(mgr.clone()));
        let root = Some(axum::extract::Extension(RootCredential));

        let invite = mgr
            .invites()
            .create(
                "space-1".to_string(),
                "admin".to_string(),
                "m1".to_string(),
                SpaceRole::Member,
            )
            .await
            .unwrap();

        let res_accept1 = espacio_invite_accept_handler(
            ext.clone(),
            root.clone(),
            None,
            axum::extract::Path(invite.id.clone()),
            Json(AcceptInviteRequest {
                member_display: "m1".to_string(),
            }),
        )
        .await;
        assert_eq!(res_accept1.status(), StatusCode::OK);

        let res_accept2 = espacio_invite_accept_handler(
            ext,
            root,
            None,
            axum::extract::Path(invite.id.clone()),
            Json(AcceptInviteRequest {
                member_display: "m1".to_string(),
            }),
        )
        .await;
        assert_eq!(res_accept2.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn expired_invite_is_refused() {
        let (_tmp, mgr) = setup_test_space().await;
        let ext = Some(axum::extract::Extension(mgr.clone()));
        let root = Some(axum::extract::Extension(RootCredential));

        let invite = mgr
            .invites()
            .create_with_expiry(
                "space-1".to_string(),
                "admin".to_string(),
                "m1".to_string(),
                SpaceRole::Member,
                chrono::Utc::now() - chrono::Duration::hours(1), // expired
            )
            .await
            .unwrap();

        let res_accept = espacio_invite_accept_handler(
            ext.clone(),
            root.clone(),
            None,
            axum::extract::Path(invite.id.clone()),
            Json(AcceptInviteRequest {
                member_display: "m1".to_string(),
            }),
        )
        .await;
        assert_eq!(res_accept.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn ttl_out_of_range_is_400() {
        let (_tmp, mgr) = setup_test_space().await;
        let ext = Some(axum::extract::Extension(mgr.clone()));
        let root = Some(axum::extract::Extension(RootCredential));

        let res = espacio_invite_create_handler(
            ext.clone(),
            root.clone(),
            None,
            axum::extract::Path("space-1".to_string()),
            Json(CreateInviteRequest {
                target_node: "new-member".to_string(),
                role: "member".to_string(),
                ttl_hours: 800,
            }),
        )
        .await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn reader_token_is_refused() {
        let (_tmp, mgr) = setup_test_space().await;
        let ext = Some(axum::extract::Extension(mgr.clone()));
        let auth = Some(axum::extract::Extension(crate::espacio::SpaceAuth {
            space_id: "space-1".to_string(),
            member_id: "reader".to_string(),
            role: SpaceRole::Reader,
        }));

        let res = espacio_invite_create_handler(
            ext.clone(),
            None,
            auth.clone(),
            axum::extract::Path("space-1".to_string()),
            Json(CreateInviteRequest {
                target_node: "new-member".to_string(),
                role: "member".to_string(),
                ttl_hours: 24,
            }),
        )
        .await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }
}
