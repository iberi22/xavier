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
//! # Why this is a source-structure test and not a request-level one
//!
//! The production router is not constructible from a test. It is built inline
//! inside `xavier::cli::server::start_http_server` (src/cli/server.rs:128),
//! which is `async fn` and, before it reaches any `.route()` call, resolves
//! the HTTP bind address, registers the sqlite-vec auto-extension, opens the
//! vec-store pools through `ConnectionManager::global()`, resolves the vault
//! token, registers providers and constructs `WorkspaceState::new().await?`.
//! There is no exported `build_router()`/`app()`/`routes()` to call; the only
//! `pub fn`s in the module are `metrics_handler`, `list_segments_handler`,
//! `get_segment_documents_handler`, `ingestion_interval_secs`,
//! `try_begin_cycle` and `end_cycle`. Requiring one would mean moving the
//! router construction out of `start_http_server` — a refactor of a
//! 2 300-line bootstrap that the D2 fix does not need, and that other agents
//! are editing right now.
//!
//! So this test asserts on the *source of the production mount* rather than on
//! a re-declared copy of it. That distinction matters: `create_router()` in
//! the handler tests is a copy, and a copy proves nothing about what the
//! daemon serves. This test reads the file the daemon is compiled from, so
//! removing the `.layer(...)` at src/cli/server.rs:1048 turns it red — see
//! `XAVIER_SERVER_RS_OVERRIDE` below and the mutation run recorded in the
//! task report.
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
use std::path::{Path, PathBuf};

/// The production HTTP bootstrap. Overridable so the mutation run can point
/// the test at a mutated *copy* instead of editing a file other agents are
/// working in. Unset in CI and in every normal run: the assertion is only
/// about the real router when the real router is the one being read.
fn production_router_source() -> String {
    let path = std::env::var_os("XAVIER_SERVER_RS_OVERRIDE")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("src/cli/server.rs"));
    fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read the production router source at {}: {e}. \
             This test is about src/cli/server.rs; if it moved, fix this path \
             rather than deleting the test.",
            path.display()
        )
    })
}

/// Every `.route(...)` block in the source, as strings. Scanning for the
/// method rather than for a path literal is deliberate: a path such as
/// `/v1/clavis/keys/{key_name}` contains braces *inside a string literal*, so
/// anchoring on the literal and counting braces lands inside the string and
/// captures nothing.
fn route_blocks(src: &str) -> Vec<String> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while let Some(rel) = src[i..].find(".route(") {
        let open = i + rel + ".route(".len();
        let mut depth = 1usize;
        let mut in_str = false;
        let mut escaped = false;
        let mut j = open;
        while j < bytes.len() && depth > 0 {
            let c = bytes[j] as char;
            if in_str {
                if escaped {
                    escaped = false;
                } else if c == '\\' {
                    escaped = true;
                } else if c == '"' {
                    in_str = false;
                }
            } else {
                match c {
                    '"' => in_str = true,
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
            }
            j += 1;
        }
        if depth != 0 {
            panic!("unbalanced parentheses in the .route(...) at byte {open}");
        }
        out.push(src[open..j - 1].to_string());
        i = j;
    }
    out
}

/// The heart of it: every Clavis mount in the production router must carry
/// the role gate. This is the assertion that fails if someone deletes the
/// `.layer(...)` in `server.rs`.
#[test]
fn cada_montaje_de_clavis_en_el_router_de_produccion_lleva_el_gate_de_secrets() {
    let src = production_router_source();
    let clavis_blocks: Vec<String> = route_blocks(&src)
        .into_iter()
        .filter(|b| b.contains(r#""/v1/clavis"#))
        .collect();
    let literals = src.matches(r#""/v1/clavis"#).count();

    assert!(
        !clavis_blocks.is_empty(),
        "D2: no .route() in src/cli/server.rs mounts a /v1/clavis path, yet the \
         source contains {literals} \"/v1/clavis literal(s). Clavis may have been \
         moved behind a merged sub-router such as `clavis_routes()` — if so, this \
         test must follow it, because the gate has to be checked wherever it now lives."
    );
    assert_eq!(
        clavis_blocks.len(),
        literals,
        "D2: {literals} \"/v1/clavis literal(s) in src/cli/server.rs but only {} \
         inside a .route() block. A Clavis path mounted outside .route() (or via \
         .merge()/.nest()) is not covered by this check.",
        clavis_blocks.len()
    );

    for block in &clavis_blocks {
        let path = block
            .split('"')
            .find(|s| s.starts_with("/v1/clavis"))
            .unwrap_or("<unknown path>");
        assert!(
            block.contains("require_permission"),
            "D2: the production mount {path:?} has no `require_permission` gate. \
             GET/PUT /v1/clavis/keys/{{key_name}} reaches JWT_PRIVATE_KEY, \
             DB_MASTER_KEY and node_secret_* as well as api_key_*; without this \
             layer only the scope layer is left, and /v1/clavis must be \
             root-only there.\nBlock was:\n{block}"
        );
        assert!(
            block.contains("can_manage_secrets"),
            "D2: the production mount {path:?} is gated, but not by \
             `can_manage_secrets` — a different permission would answer it \
             instead. Gate must stay `require_permission(|r| r.can_manage_secrets())`.\n\
             Block was:\n{block}"
        );
    }
}

/// A Clavis route added to the production router later inherits no gate unless
/// this file is updated. Pin the count so adding a fourth Clavis mount is a
/// deliberate edit here, not a silent hole.
#[test]
fn el_numero_de_montajes_de_clavis_en_produccion_no_crece_sin_revision() {
    let src = production_router_source();
    let mounts = src.matches(r#""/v1/clavis"#).count();
    assert_eq!(
        mounts, 2,
        "src/cli/server.rs now mounts {mounts} Clavis routes (expected 2: \
         /v1/clavis/keys/{{key_name}} and /v1/clavis/proxy). Each new one needs \
         its can_manage_secrets gate and its line here."
    );
}

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
