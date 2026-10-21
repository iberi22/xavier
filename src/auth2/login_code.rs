//! One-time login codes: 6 digits, 120 s TTL, single use, bound to one user.
//! Only the SHA-256 hex of the code is persisted.

use crate::auth2::db::{AuthDb, User};
use anyhow::Result;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

pub const LOGIN_CODE_TTL_SECS: i64 = 120;
/// Wrong guesses allowed before a code is burned.
pub const LOGIN_CODE_MAX_FAILED: i64 = 3;

fn hash_code(code: &str) -> String {
    crate::crypto::hex_encode(Sha256::digest(code.as_bytes()))
}

/// Uniform 6-digit code from the OS CSPRNG (rejection sampling, no modulo bias).
fn generate_code() -> String {
    const LIMIT: u32 = 4_294_000_000; // largest multiple of 1_000_000 <= u32::MAX
    loop {
        let n = rand::RngCore::next_u32(&mut rand::rngs::OsRng);
        if n < LIMIT {
            return format!("{:06}", n % 1_000_000);
        }
    }
}

/// Mint a code for `user_id`, invalidating its older unused codes. Returns the plaintext.
pub fn issue_login_code(db: &AuthDb, user_id: &str, now: i64) -> Result<String> {
    let code = generate_code();
    db.insert_login_code(
        &ulid::Ulid::new().to_string(),
        user_id,
        &hash_code(&code),
        now + LOGIN_CODE_TTL_SECS,
        now,
    )?;
    Ok(code)
}

/// Redeem a code. `None` on any failure (unknown email, wrong, expired, used, burned).
pub fn redeem_login_code(db: &AuthDb, email: &str, code: &str, now: i64) -> Result<Option<User>> {
    let Some(user) = db.get_user_by_email(email)? else {
        return Ok(None);
    };
    let Some((id, stored)) = db.get_active_login_code(&user.id, now, LOGIN_CODE_MAX_FAILED)? else {
        return Ok(None);
    };
    let supplied = hash_code(code.trim());
    if !bool::from(supplied.as_bytes().ct_eq(stored.as_bytes())) {
        db.record_login_code_failure(&id)?;
        return Ok(None);
    }
    if !db.consume_login_code(&id, now)? {
        return Ok(None);
    }
    Ok(Some(user))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth2::{auth_routes, HasAuthDb};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use std::sync::Arc;
    use tower::ServiceExt;

    const NOW: i64 = 1_000_000;
    const EMAIL: &str = "op@example.com";

    fn setup() -> (tempfile::TempDir, AuthDb) {
        let dir = tempfile::tempdir().unwrap();
        let db = AuthDb::new(&dir.path().join("auth.db")).unwrap();
        db.create_user(&User {
            id: "u1".into(),
            email: EMAIL.into(),
            password_hash: "h".into(),
            name: "Op".into(),
            role: "user".into(),
            totp_secret: None,
            totp_enabled: false,
            recovery_seed_hash: None,
            backup_codes: None,
            created_at: NOW,
            updated_at: NOW,
        })
        .unwrap();
        (dir, db)
    }

    fn now_secs() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    fn wrong(code: &str) -> String {
        if code == "000000" { "000001" } else { "000000" }.to_string()
    }

    #[test]
    fn login_code_happy_path() {
        let (_d, db) = setup();
        let code = issue_login_code(&db, "u1", NOW).unwrap();
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| c.is_ascii_digit()));
        let u = redeem_login_code(&db, EMAIL, &code, NOW + 5).unwrap();
        assert_eq!(u.unwrap().id, "u1");
    }

    #[test]
    fn login_code_expired() {
        let (_d, db) = setup();
        let code = issue_login_code(&db, "u1", NOW).unwrap();
        let at = NOW + LOGIN_CODE_TTL_SECS;
        assert!(redeem_login_code(&db, EMAIL, &code, at).unwrap().is_none());
    }

    #[test]
    fn login_code_reuse_rejected() {
        let (_d, db) = setup();
        let code = issue_login_code(&db, "u1", NOW).unwrap();
        assert!(redeem_login_code(&db, EMAIL, &code, NOW).unwrap().is_some());
        assert!(redeem_login_code(&db, EMAIL, &code, NOW).unwrap().is_none());
    }

    #[test]
    fn login_code_three_wrong_burns_code() {
        let (_d, db) = setup();
        let code = issue_login_code(&db, "u1", NOW).unwrap();
        for _ in 0..3 {
            assert!(redeem_login_code(&db, EMAIL, &wrong(&code), NOW)
                .unwrap()
                .is_none());
        }
        assert!(redeem_login_code(&db, EMAIL, &code, NOW).unwrap().is_none());
    }

    #[test]
    fn login_code_new_issue_invalidates_old() {
        let (_d, db) = setup();
        let old = issue_login_code(&db, "u1", NOW).unwrap();
        let new = issue_login_code(&db, "u1", NOW + 1).unwrap();
        if old != new {
            assert!(redeem_login_code(&db, EMAIL, &old, NOW + 2)
                .unwrap()
                .is_none());
        }
        assert!(redeem_login_code(&db, EMAIL, &new, NOW + 2)
            .unwrap()
            .is_some());
    }

    #[test]
    fn login_code_unknown_email_none() {
        let (_d, db) = setup();
        let code = issue_login_code(&db, "u1", NOW).unwrap();
        assert!(redeem_login_code(&db, "nobody@example.com", &code, NOW)
            .unwrap()
            .is_none());
    }

    #[test]
    fn login_code_stores_hash_not_plaintext() {
        let (_d, db) = setup();
        let code = issue_login_code(&db, "u1", NOW).unwrap();
        let (stored, failed, used) = db.latest_login_code_row("u1").unwrap();
        assert_ne!(stored, code);
        assert_eq!(stored, hash_code(&code));
        assert_eq!(stored.len(), 64);
        assert_eq!((failed, used), (0, false));
    }

    #[derive(Clone)]
    struct TestState(Arc<parking_lot::Mutex<AuthDb>>);
    impl HasAuthDb for TestState {
        fn auth_db(&self) -> Option<Arc<parking_lot::Mutex<AuthDb>>> {
            Some(self.0.clone())
        }
    }

    async fn post_exchange(app: &axum::Router, email: &str, code: &str) -> (StatusCode, Vec<u8>) {
        let body = serde_json::json!({ "email": email, "code": code }).to_string();
        let req = Request::builder()
            .method("POST")
            .uri("/exchange")
            .header("content-type", "application/json")
            .header("x-forwarded-for", "login-code-test")
            .body(Body::from(body))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), 1 << 20)
            .await
            .unwrap();
        (status, bytes.to_vec())
    }

    #[tokio::test]
    async fn login_code_exchange_handler() {
        let (_d, db) = setup();
        let code = issue_login_code(&db, "u1", now_secs()).unwrap();
        let state = TestState(Arc::new(parking_lot::Mutex::new(db)));
        let app = auth_routes::<TestState>("/unused").with_state(state);

        let (s_unknown, b_unknown) = post_exchange(&app, "nobody@example.com", &code).await;
        let (s_wrong, b_wrong) = post_exchange(&app, EMAIL, &wrong(&code)).await;
        assert_eq!(s_unknown, StatusCode::UNAUTHORIZED);
        assert_eq!(s_wrong, StatusCode::UNAUTHORIZED);
        assert_eq!(b_unknown, b_wrong);
        assert_eq!(b_wrong, br#"{"error":"invalid_code"}"#);

        let (s_ok, b_ok) = post_exchange(&app, EMAIL, &code).await;
        assert_eq!(s_ok, StatusCode::OK);
        let v: serde_json::Value = serde_json::from_slice(&b_ok).unwrap();
        assert_eq!(v["requires_2fa"], false);
        assert_eq!(v["user"]["email"], EMAIL);
        assert!(v["access_token"].as_str().is_some_and(|t| !t.is_empty()));
        assert!(v["refresh_token"].as_str().is_some_and(|t| !t.is_empty()));
    }
}
