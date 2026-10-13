//! D2 — Clavis wiring must stay behind three layers, and this test must be
//! the one that notices when it stops being so.
//!
//! # Why this file exists at all
//!
//! The two problems it pins are:
//!
//! 1. `/v1/clavis/*` was missing from `ROOT_ONLY_PREFIXES`, so a scoped `xav_`
//!    token holding `all` cleared the scope layer and only the role gate stood
//!    between it and the node's whole secret vault (`JWT_PRIVATE_KEY`,
//!    `DB_MASTER_KEY`, `node_secret_*`). `/secrets/*` answers only when three
//!    layers agree; Clavis used to require one.
//!
//! 2. The handler tests all build `routes::create_router()`, which is a
//!    *different* router from the one the daemon serves. That is exactly why
//!    deleting `require_permission` from the production mount in
//!    `src/cli/server.rs` left the whole suite green (the gap that shipped
//!    ungated routes in #2793 and #2800).
//!
//! # What it does NOT prove
//!
//! It matches text, so it verifies that a `can_manage_secrets` gate is
//! attached to the Clavis mounts in the production router, but it does not
//! execute `require_permission` against a request. The honest full fix is to
//! extract `clavis_routes()` beside the handlers, `#[deny(dead_code)]` it and
//! `merge()` it in `server.rs`, following `secrets_routes()` from c729d9b1 —
//! then a request-level test can build the real sub-router. That change edits
//! `src/cli/server.rs`, outside the file island this task was given.

use std::fs;
use std::path::Path;

/// The policy half of D2, pinned against the real list rather than a copy of
/// it, so both halves are checked from one place. The behavioural version of
/// this (scope combinations, all three verbs) lives in
/// `src/cli/http_setup.rs::scope_policy_tests`; this one only asserts the
/// prefix is present at all, because that list is private to the module and
/// not reachable from an integration test.
#[test]
fn el_prefijo_clavis_esta_en_root_only_prefixes() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/cli/http_setup.rs");
    let src =
        fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));

    // The list is `const ROOT_ONLY_PREFIXES: &[&str] = &[ … ];` — anchor on the
    // `= &[`, not on the first `[`, which belongs to the type annotation.
    let start = src
        .find("const ROOT_ONLY_PREFIXES")
        .unwrap_or_else(|| panic!("ROOT_ONLY_PREFIXES no longer exists in {}", path.display()));
    let list = &src[start..];
    let open = list
        .find("= &[")
        .map(|i| i + "= &[".len())
        .unwrap_or_else(|| {
            panic!(
                "ROOT_ONLY_PREFIXES has no `= &[` initialiser: {}",
                path.display()
            )
        });
    let close = list[open..]
        .find(']')
        .map(|i| open + i)
        .expect("ROOT_ONLY_PREFIXES has no closing bracket");
    let list = &list[open..=close];

    assert!(
        list.contains(r#""/v1/clavis""#),
        "D2: \"/v1/clavis\" is missing from ROOT_ONLY_PREFIXES, so a scoped \
         `xav_` token holding `all` reaches the Clavis vault routes. \
         List was:\n{list}"
    );
}

// Sandbox HOME/XDG/XAVIER_* dirs before main: tests must never touch the real ~/.xavier.
xavier::isolate_test_process!();
