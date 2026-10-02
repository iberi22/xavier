//! ===========================================================================
//! D1 — NODE SECRET LEAK VIA `GET /v1/clavis/keys/{name}`  (RED ON PURPOSE)
//! ===========================================================================
//!
//! ## What this test documents
//!
//! Defect D1: the Clavis read endpoint serves secrets that belong to OTHER
//! services on the node, not just Clavis provider keys.
//!
//! Root cause (all verified by reading the code, not by guessing):
//!
//! 1. `validate_key_name()` (src/adapters/inbound/http/handlers/clavis.rs:329)
//!    only checks "non-empty, <=128 chars, ASCII alnum + `_ - .`, no leading
//!    `.`". It does NOT require the `api_key_` namespace, so it happily accepts
//!    `JWT_PRIVATE_KEY`, `DB_MASTER_KEY`, `SWAL_DEPLOYER_KEY`,
//!    `node_secret_xv1-a1b2c3d4`, ... i.e. real production node secret names.
//! 2. `HardwareVault`'s fallback backend (src/secrets/vault.rs:24) is a GLOBAL
//!    `static BACKEND: OnceLock<Option<VaultBackend>>` whose `storage_dir` is
//!    `~/.xavier/secrets` and whose AES key is derived WITHOUT the
//!    `service_name` (vault.rs:51-56). Therefore the "xavier" service and the
//!    "xavier-clavis" service read and write THE SAME DIRECTORY with THE SAME
//!    KEY. Namespacing by service name is cosmetic only.
//! 3. `get_secret()` (vault.rs:132) falls through to that shared on-disk
//!    fallback on ANY keyring error, including `NoEntry`.
//! 4. `get_secret_with_legacy_fallback()` (clavis.rs:108) DOES have an
//!    `api_key_` security gate — but it is applied only to step 2 (the legacy
//!    "xavier" vault). Step 1 is `vault.get_secret(key_name)` with the RAW,
//!    UNVALIDATED-AGAINST-NAMESPACE name, against the vault that per (2) is
//!    shared with every other service. That step-1 path has no namespace gate.
//!
//! Net effect: `GET /v1/clavis/keys/JWT_PRIVATE_KEY` returns 200 with the real
//! node JWT signing key in the body. Already demonstrated in production on this
//! machine (~296 unrelated entries live in `~/.xavier/secrets`).
//!
//! ## THIS FILE IS EXPECTED TO FAIL (RED) AGAINST THE CURRENT CODE.
//!
//! That is the point. It is a pinned reproduction of D1, not a passing test.
//! It is written so that fixing the handler turns it GREEN **without editing a
//! single assertion**: every assertion below states the security property, not
//! the current behaviour.
//!
//! Fixing D1 (any correct variant of the fix) makes it pass:
//!   * rejecting non-`api_key_` names outright -> 400, or
//!   * 404/403 for a name outside the Clavis namespace, or
//!   * severing the shared backend / dropping the step-1 raw-name lookup
//!
//! Any 4xx is accepted. What is NOT accepted, ever, is a 2xx carrying the
//! seeded value back to the caller.
//!
//! ## Anti-false-green guards
//!
//! This test would pass *vacuously* if the isolated vault were not actually in
//! use — e.g. if the seed never landed and every read returned 404. Three
//! independent guards make that impossible:
//!   * `hermetic_environment_is_proven_before_anything_is_seeded` proves the
//!     storage dir is the temp dir AND the OS keyring is genuinely unreachable,
//!     before a single secret is written. It refuses to run the D1 assertions
//!     otherwise.
//!   * `seed_and_verify` reads every canary back through `HardwareVault` BEFORE
//!     the handler is invoked. If the seed is not readable, it panics loudly
//!     instead of letting the test go green.
//!   * `api_key_namespaced_secret_is_still_readable` asserts the endpoint still
//!     returns 200 + value for a legitimate `api_key_*` key, so resolving D1 by
//!     blanket-blocking everything is NOT an accepted fix.
//!
//! ## State isolation — and a trap worth knowing about
//!
//! `clavis::set_clavis_vault` is `pub(crate)` + `#[cfg(test)]`, so it is not
//! reachable from `tests/`. The tracked test `tests/clavis_http_e2e.rs` isolates
//! by redirecting `HOME` / `XAVIER_DATA_DIR` — and we reuse that pattern.
//!
//! BUT that pattern alone is NOT sufficient, and this was measured, not guessed:
//! on Linux the `keyring` crate talks to the OS Secret Service over **D-Bus**,
//! which is completely unaffected by `HOME`. Redirecting `HOME` does not stop
//! `store_secret`/`get_secret` from writing to and reading from the REAL system
//! keyring under service `xavier-clavis`. A test written only that way silently
//! seeds the developer's actual keyring — and, worse, can read the node's real
//! `JWT_PRIVATE_KEY`.
//!
//! So this file additionally severs D-Bus (`DBUS_SESSION_BUS_ADDRESS` and
//! `DBUS_SYSTEM_BUS_ADDRESS` -> a nonexistent socket). Every keyring operation
//! then errors, `HardwareVault` falls back to the encrypted file vault, and that
//! file vault is confined to the redirected `HOME`. The guard test PROVES both
//! halves of that before anything is seeded.
//!
//! NOTE on canary values: they are deliberately plain `D1_CANARY_*` strings
//! rather than PEM material. A PEM-shaped canary gets rewritten to
//! `[REDACTED ...]` by secret-scrubbing log/test filters, which would silently
//! break the read-back equality assertion.

use std::sync::OnceLock;

use axum::body::to_bytes;
use axum::extract::Path;
use axum::response::IntoResponse;

use xavier::adapters::inbound::http::handlers::clavis::get_clavis_key_handler;
use xavier::secrets::vault::HardwareVault;

/// Keyring service name of the Clavis vault (clavis.rs:44) — the same one the
/// handler resolves through `clavis_vault()`.
const CLAVIS_SERVICE: &str = "xavier-clavis";

// ---------------------------------------------------------------------------
// Hermetic setup
// ---------------------------------------------------------------------------

/// Redirect `HOME` / `XAVIER_DATA_DIR` to a private temp dir AND cut the D-Bus
/// session/system bus so the OS keyring is unreachable. Runs once per process;
/// the TempDir lives in a `OnceLock` so it cannot be dropped while a later test
/// is still writing into it.
fn hermetic_env() -> &'static tempfile::TempDir {
    static CELL: OnceLock<tempfile::TempDir> = OnceLock::new();
    CELL.get_or_init(|| {
        let tmp = tempfile::tempdir().expect("create hermetic tempdir for D1 test");
        std::env::set_var("HOME", tmp.path());
        std::env::set_var("XAVIER_DATA_DIR", tmp.path());
        // Sever D-Bus so `keyring` cannot reach the real Secret Service.
        const DEAD_SOCKET: &str = "unix:path=/nonexistent-d1-test-secret-service";
        std::env::set_var("DBUS_SESSION_BUS_ADDRESS", DEAD_SOCKET);
        std::env::set_var("DBUS_SYSTEM_BUS_ADDRESS", DEAD_SOCKET);
        tmp
    })
}

/// The vault instance the handler itself will use, built from the hermetic env.
fn handler_vault() -> &'static HardwareVault {
    static CELL: OnceLock<HardwareVault> = OnceLock::new();
    CELL.get_or_init(|| {
        hermetic_env();
        HardwareVault::new(CLAVIS_SERVICE)
    })
}

/// Call the REAL handler (not the router: the defect is in the handler itself)
/// and return `(status, body_as_string)`.
async fn get_via_handler(key_name: &str) -> (u16, String) {
    let response = get_clavis_key_handler(Path(key_name.to_string()))
        .await
        .into_response();
    let status = response.status().as_u16();
    let bytes = to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("collect handler body");
    (status, String::from_utf8_lossy(&bytes).to_string())
}

// ---------------------------------------------------------------------------
// The canaries — realistic names of secrets owned by OTHER node services
// ---------------------------------------------------------------------------

/// `(key name, canary value)`. These are exactly the shape of real node secrets
/// that live in `~/.xavier/secrets` alongside the Clavis keys. The values are
/// unique strings prefixed `D1_CANARY_` so a leak is unmistakable in CI output.
const FOREIGN_SECRETS: &[(&str, &str)] = &[
    ("JWT_PRIVATE_KEY", "D1_CANARY_JWT_SIGNING_KEY_DO_NOT_LEAK"),
    ("DB_MASTER_KEY", "D1_CANARY_DB_MASTER_DO_NOT_LEAK_0001"),
    ("SWAL_DEPLOYER_KEY", "D1_CANARY_DEPLOYER_DO_NOT_LEAK_0002"),
    (
        "node_secret_xv1-a1b2c3d4",
        "D1_CANARY_NODE_SECRET_DO_NOT_LEAK_0003",
    ),
];

/// A legitimately namespaced Clavis key: must keep working after D1 is fixed.
const NAMESPACED_KEY: &str = "api_key_openai";
const NAMESPACED_VALUE: &str = "D1_CANARY_LEGIT_API_KEY_must_remain_readable";

// ---------------------------------------------------------------------------
// Guard 0 — prove isolation BEFORE writing any secret
// ---------------------------------------------------------------------------

/// ANTI-FALSE-GREEN + ANTI-CONTAMINATION GUARD.
///
/// Fails loudly (so the D1 assertions below cannot run) unless:
///   * the vault's storage directory is the temp dir, and
///   * the OS keyring is genuinely unreachable, so the pre-existing production
///     entries this node holds under the same keyring service are NOT visible.
///
/// Without the second check, a test like this would read and overwrite the
/// developer's real node secrets.
#[test]
#[serial_test::serial]
fn hermetic_environment_is_proven_before_anything_is_seeded() {
    let vault = handler_vault();

    let home = dirs::home_dir().expect("home dir must resolve");
    let expected = hermetic_env().path().to_path_buf();
    assert_eq!(
        home,
        expected,
        "D1 test refusing to run: HOME is {} but the hermetic dir is {}",
        home.display(),
        expected.display()
    );

    let probe_key = "D1_ISOLATION_PROBE_KEYRING_MUST_BE_UNREACHABLE";
    vault
        .store_secret(probe_key, "D1_ISOLATION_PROBE_VALUE")
        .expect("probe store must succeed via the file fallback");

    // The write must have landed in the temp dir, as an encrypted .enc file.
    let enc = expected
        .join(".xavier")
        .join("secrets")
        .join(format!("{probe_key}.enc"));
    assert!(
        enc.exists(),
        "D1 test refusing to run: probe write did not land in the isolated \
         fallback dir at {} — the vault is not hermetic",
        enc.display()
    );
    assert!(
        std::fs::metadata(&enc).expect("stat probe .enc").len() > 16,
        "probe file should be an AES-GCM blob, not plaintext"
    );

    // Decisive part: a still-live keyring would answer for a name this file
    // NEVER seeds. It must therefore be absent — if it resolves, the value can
    // only have come from the real OS keyring.
    //
    // NB: this must not reuse a FOREIGN_SECRETS name. Those are seeded on
    // purpose by the leak test, so probing them here would make this guard
    // trip on our own seed depending on test execution order.
    const NEVER_SEEDED_PROBE: &str = "D1_KEYRING_LIVENESS_PROBE_9f2c1a7e";
    assert!(
        vault.get_secret(NEVER_SEEDED_PROBE).is_err(),
        "D1 test refusing to run: the OS keyring is still reachable \
         ({NEVER_SEEDED_PROBE} resolved, but this test never seeds it). \
         HOME redirection alone does NOT isolate the keyring on Linux."
    );

    let _ = vault.delete_secret(probe_key);
}

// ---------------------------------------------------------------------------
// Guard 1 — prove the seed is readable before running the leak assertions
// ---------------------------------------------------------------------------

/// ANTI-FALSE-GREEN GUARD. If this fails, the seeded canaries are not reachable
/// through the vault the handler uses and every assertion in the leak test would
/// be vacuously satisfied by 404s. We want a loud failure instead.
fn seed_and_verify() {
    let vault = handler_vault();

    for (name, canary) in FOREIGN_SECRETS {
        vault
            .store_secret(name, canary)
            .unwrap_or_else(|e| panic!("seed {name} into isolated vault: {e}"));
    }
    vault
        .store_secret(NAMESPACED_KEY, NAMESPACED_VALUE)
        .expect("seed namespaced key");

    for (name, canary) in FOREIGN_SECRETS {
        let read_back = vault.get_secret(name).unwrap_or_else(|e| {
            panic!("seed {name} is NOT readable back: {e} — the D1 test would be a FALSE GREEN")
        });
        assert_eq!(
            &read_back, canary,
            "seed {name} did not round-trip through the isolated vault; \
             the D1 test would be a FALSE GREEN"
        );
    }
    assert_eq!(vault.get_secret(NAMESPACED_KEY).unwrap(), NAMESPACED_VALUE);
}

// ---------------------------------------------------------------------------
// THE D1 REGRESSION TEST — expected to FAIL against the current code
// ---------------------------------------------------------------------------

/// D1: `GET /v1/clavis/keys/{name}` MUST NOT disclose secrets that live outside
/// the Clavis `api_key_` namespace, even though they sit in the same shared
/// `~/.xavier/secrets` directory as the Clavis keys.
///
/// Against the current code this FAILS: `get_secret_with_legacy_fallback` looks
/// the raw name up in the shared vault before any namespace gate is applied,
/// and `validate_key_name` accepts the name in the first place.
#[tokio::test]
#[serial_test::serial]
async fn get_clavis_key_must_not_leak_secrets_from_other_node_services() {
    hermetic_environment_is_proven_before_anything_is_seeded();
    seed_and_verify();

    // Collect every violation instead of panicking on the first one, so the red
    // report shows the FULL blast radius rather than one leaked key.
    let mut leaks: Vec<String> = Vec::new();

    for (name, canary) in FOREIGN_SECRETS {
        let (status, body) = get_via_handler(name).await;

        // The hard rule: a 2xx must never carry the secret back.
        if (200..300).contains(&status) {
            leaks.push(format!(
                "{name}: HTTP {status} returned the secret body: {}",
                body.chars().take(200).collect::<String>()
            ));
            continue;
        }

        // And no error path may echo the value either (e.g. a 4xx that
        // helpfully quotes what it refused to serve).
        if body.contains(canary) {
            leaks.push(format!(
                "{name}: HTTP {status} leaked the value inside an error body"
            ));
            continue;
        }

        // 404 / 403 / 400 are all acceptable rejections.
        assert!(
            (400..500).contains(&status),
            "{name}: expected a 4xx rejection (404/403/400) or a non-2xx, got HTTP {status} \
             with body: {}",
            body.chars().take(200).collect::<String>()
        );
    }

    assert!(
        leaks.is_empty(),
        "SECURITY DEFECT D1 REPRODUCED — the Clavis read endpoint served secrets owned \
         by other node services. Leaked {} of {} foreign secrets:\n  - {}\n\n\
         Root cause: validate_key_name() does not require the `api_key_` namespace, and \
         get_secret_with_legacy_fallback() step 1 queries the shared ~/.xavier/secrets \
         vault with the RAW name before any namespace gate runs.",
        leaks.len(),
        FOREIGN_SECRETS.len(),
        leaks.join("\n  - ")
    );
}

// ---------------------------------------------------------------------------
// Guard 2 — the fix must not be "block everything"
// ---------------------------------------------------------------------------

/// A secret legitimately inside the Clavis namespace MUST still be readable.
/// This exists so that resolving D1 by blanket-blocking all reads (or by making
/// the endpoint always 404) does NOT turn the suite green — functionality has
/// to survive the fix.
#[tokio::test]
#[serial_test::serial]
async fn api_key_namespaced_secret_is_still_readable() {
    hermetic_environment_is_proven_before_anything_is_seeded();
    seed_and_verify();

    let (status, body) = get_via_handler(NAMESPACED_KEY).await;

    assert_eq!(
        status,
        200,
        "D1 was 'fixed' by breaking legitimate functionality: \
         GET /v1/clavis/keys/{NAMESPACED_KEY} must keep returning 200, got {status} \
         with body: {}",
        body.chars().take(200).collect::<String>()
    );

    let parsed: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|e| panic!("200 body must be valid JSON: {e}; body was: {body}"));
    assert_eq!(
        parsed["value"], NAMESPACED_VALUE,
        "200 response did not carry the stored Clavis key value"
    );
}
