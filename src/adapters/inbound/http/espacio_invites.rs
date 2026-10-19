use axum::{
    extract::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::adapters::inbound::http::routes::{get_space_manager, RootCredential};

fn espacio_disabled() -> axum::response::Response {
    use axum::response::IntoResponse;
    (
        axum::http::StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({"status":"error","message":"espacio is disabled on this node"})),
    )
        .into_response()
}

fn espacio_forbidden() -> axum::response::Response {
    use axum::response::IntoResponse;
    (
        axum::http::StatusCode::FORBIDDEN,
        Json(serde_json::json!({"status":"error","message":"Forbidden"})),
    )
        .into_response()
}

fn espacio_join_error() -> axum::response::Response {
    use axum::response::IntoResponse;
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"status":"error","message":"internal error"})),
    )
        .into_response()
}

fn espacio_error_response(err: anyhow::Error) -> axum::response::Response {
    use crate::espacio::SpaceError;
    use axum::http::StatusCode;
    use axum::response::IntoResponse;
    let msg = err.to_string();
    let status = match err.downcast_ref::<SpaceError>() {
        Some(SpaceError::AlreadyExists(_)) => StatusCode::CONFLICT,
        Some(SpaceError::InvalidId(_) | SpaceError::InvalidProtocol(_)) => StatusCode::BAD_REQUEST,
        Some(SpaceError::NotFound(_)) => StatusCode::NOT_FOUND,
        _ => match err.downcast_ref::<crate::espacio::manager::MembersCapReached>() {
            Some(_) => StatusCode::CONFLICT,
            _ => {
                if msg.contains("already used") || msg.contains("already exists") {
                    StatusCode::CONFLICT
                } else if msg.contains("expired")
                    || msg.contains("revoked")
                    || msg.contains("invalid")
                    || msg.contains("not addressed")
                    || msg.contains("signature")
                {
                    StatusCode::BAD_REQUEST
                } else if msg.contains("not found") {
                    StatusCode::NOT_FOUND
                } else {
                    StatusCode::INTERNAL_SERVER_ERROR
                }
            }
        },
    };
    (
        status,
        Json(serde_json::json!({"status":"error","message":msg})),
    )
        .into_response()
}

async fn run_blocking<T, F>(fut: F) -> Option<T>
where
    T: Send + 'static,
    F: std::future::Future<Output = T> + Send + 'static,
{
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || handle.block_on(fut))
        .await
        .ok()
}
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

    let has_access = match &auth {
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

    let invites = crate::espacio::InviteManager::with_stores(manager.stores());
    match run_blocking(async move {
        manager.get(&id).await?;
        invites
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

    let has_access = match &auth {
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

    let invites = crate::espacio::InviteManager::with_stores(manager.stores());
    match run_blocking(async move {
        manager.get(&id).await?;
        Ok::<_, anyhow::Error>(invites.list_for_space(&id).await)
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
    _auth: Option<axum::extract::Extension<crate::espacio::SpaceAuth>>,
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

    let invites = crate::espacio::InviteManager::with_stores(manager.stores());
    match run_blocking(async move {
        invites
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
            let msg = err.to_string();
            if msg.contains("already used") || msg.contains("revoked") {
                return (
                    axum::http::StatusCode::CONFLICT,
                    Json(serde_json::json!({"status":"error","message":msg})),
                )
                    .into_response();
            } else if msg.contains("expired") {
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
        let kek = Arc::new(crate::espacio::StaticNodeKek::new([9u8; 32]));
        let mgr = crate::cli::state::open_space_manager_with_kek(tmp.path(), kek).unwrap();
        let _ = mgr
            .create(
                "space-1".to_string(),
                "Space 1".to_string(),
                "".to_string(),
                "admin".to_string(),
                false,
            )
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
            root,
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
            root,
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

        let invites = crate::espacio::InviteManager::with_stores(mgr.stores());
        let invite = invites
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
            root,
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

        let invites = crate::espacio::InviteManager::with_stores(mgr.stores());
        let invite = invites
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
            root,
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
            root,
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
