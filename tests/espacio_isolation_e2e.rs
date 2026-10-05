//! WP13o: end-to-end isolation matrix against the REAL daemon binary.
//!
//! Spawns `xavier http <port>` on a random free port with every path (HOME,
//! XDG dirs, data, state, workspace, db files) inside a tempdir and runs the
//! A/B matrix over real HTTP: memory isolation, one-way links, revoke,
//! non-member zero, root-only admin plane, encrypted-space refusal, trashed
//! space tokens, and a daemon restart. THIS MACHINE MAY BE A PRODUCTION
//! NODE: the child gets a scrubbed environment (no `XAVIER_*` from the
//! parent, no DBus/XDG runtime so the OS keyring is unreachable, a temp
//! HOME so `~/.xavier` is never touched) and never talks to port 8006.

use reqwest::{Client, Method, StatusCode};
use serde_json::{json, Value};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use xavier::security::auth::{generate_jwt, User, UserRole};

const ROOT: &str = "wp13o-e2e-root-token";
const JWT_SECRET: &str = "wp13o-e2e-jwt-secret-0123456789abcdef";

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local addr")
        .port()
}

struct Daemon {
    _child: ChildGuard,
    url: String,
    http: Client,
}

impl Daemon {
    /// Spawn the daemon with everything under `home` (a tempdir root).
    async fn spawn(bin: &Path, home: &Path, spaces_off: bool) -> Self {
        assert!(
            home.starts_with(std::env::temp_dir()),
            "scratch root must be an absolute temp path"
        );
        let port = free_port();
        assert_ne!(port, 8006, "never the live daemon port");
        let url = format!("http://127.0.0.1:{port}");
        for d in ["home", "data", "state", "ws", "cwd"] {
            std::fs::create_dir_all(home.join(d)).unwrap();
        }
        let mut cmd = Command::new(bin);
        // Scrub: nothing from the parent's XAVIER_* config, and no route to
        // the desktop session bus (OS keyring) or runtime dir.
        for (k, _) in std::env::vars() {
            if k.starts_with("XAVIER_") || k.starts_with("XDG_") || k.starts_with("DBUS_") {
                cmd.env_remove(k);
            }
        }
        cmd.arg("http")
            .arg(port.to_string())
            .arg("--mcp-port")
            .arg("0")
            .current_dir(home.join("cwd"))
            .env("HOME", home.join("home"))
            .env("XDG_CONFIG_HOME", home.join("home/.config"))
            .env("XDG_DATA_HOME", home.join("home/.local/share"))
            .env("XDG_CACHE_HOME", home.join("home/.cache"))
            .env("XDG_STATE_HOME", home.join("home/.local/state"))
            .env("XAVIER_HOST", "127.0.0.1")
            .env("XAVIER_PORT", port.to_string())
            .env("XAVIER_URL", &url)
            .env("XAVIER_MCP_PORT", "0")
            .env("XAVIER_TOKEN", ROOT)
            .env("XAVIER_JWT_SECRET", JWT_SECRET)
            .env("XAVIER_DATA_DIR", home.join("data"))
            .env("XAVIER_STATE_DIR", home.join("state"))
            .env("XAVIER_WORKSPACE_DIR", home.join("ws"))
            .env("XAVIER_INGESTION_INTERVAL_SECS", "0")
            .env("XAVIER_CODE_GRAPH_DB_PATH", home.join("data/codegraph.db"))
            .env(
                "XAVIER_MEMORY_VEC_PATH",
                home.join("data/vec-store.sqlite3"),
            )
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        if spaces_off {
            cmd.env("XAVIER_SPACES", "off");
        }
        let child = cmd.spawn().expect("failed to spawn xavier binary");
        let guard = ChildGuard(child);
        let http = Client::new();
        let mut healthy = false;
        for _ in 0..120 {
            if let Ok(r) = http.get(format!("{url}/health")).send().await {
                if r.status().is_success() {
                    healthy = true;
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        assert!(healthy, "daemon not healthy at {url} within 60s");
        Self {
            _child: guard,
            url,
            http,
        }
    }

    async fn call(
        &self,
        method: Method,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut rb = self.http.request(method, format!("{}{path}", self.url));
        if let Some(t) = token {
            rb = rb.header("X-Xavier-Token", t);
        }
        if let Some(b) = body {
            rb = rb.json(&b);
        }
        let resp = rb.send().await.expect("request");
        let status = resp.status();
        let bytes = resp.bytes().await.unwrap_or_default();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn add(&self, token: &str, path: &str, text: &str) {
        let (st, b) = self
            .call(
                Method::POST,
                "/memory/add",
                Some(token),
                Some(json!({"content": text, "path": path})),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "add {path}: {b}");
    }

    /// Search; returns the raw `results` text for substring checks and the
    /// parsed body.
    async fn search(&self, token: &str, q: &str, linked: bool) -> Value {
        let (st, b) = self
            .call(
                Method::POST,
                "/memory/search",
                Some(token),
                Some(json!({"query": q, "limit": 20, "include_linked": linked})),
            )
            .await;
        assert_eq!(st, StatusCode::OK, "search {q}: {b}");
        b
    }
}

fn results(b: &Value) -> String {
    b["results"].to_string()
}

fn sources(b: &Value) -> Vec<String> {
    b["results"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|r| r["source_space"].as_str().unwrap_or("").to_string())
        .collect()
}

async fn create_space(d: &Daemon, id: &str, encrypt: bool) -> String {
    let (st, b) = d
        .call(
            Method::POST,
            "/api/v1/espacio/admin/spaces",
            Some(ROOT),
            Some(json!({
                "id": id, "name": id, "description": "", "owner_node": format!("owner-{id}"),
                "encrypt_records": encrypt
            })),
        )
        .await;
    assert_eq!(st, StatusCode::CREATED, "create {id}: {b}");
    let tok = b["owner_token"].as_str().expect("owner token").to_string();
    assert!(tok.starts_with(&format!("xsp_{id}_")), "{tok}");
    tok
}

/// A private copy (hard link when possible) of the daemon binary. The cargo
/// target dir can be shared between worktrees, and another build could
/// replace `xavier` between the daemon's first start and its restart; the
/// test must keep running the binary it was built with.
struct BinCopy(PathBuf);

impl BinCopy {
    fn new() -> Self {
        let src = PathBuf::from(env!("CARGO_BIN_EXE_xavier"));
        let dst = src.with_file_name(format!(
            "xavier-wp13o-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        if std::fs::hard_link(&src, &dst).is_err() {
            std::fs::copy(&src, &dst).expect("copy daemon binary");
        }
        Self(dst)
    }
}

impl Drop for BinCopy {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn scratch() -> (tempfile::TempDir, PathBuf, BinCopy) {
    let t = tempfile::tempdir().expect("tempdir");
    let p = t.path().to_path_buf();
    (t, p, BinCopy::new())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn isolation_matrix_over_real_http_and_across_restart() {
    let (_keep, home, bin) = scratch();
    let d = Daemon::spawn(&bin.0, &home, false).await;

    // --- boot migration: the legacy workspace is recorded by reference ---
    let desc: Value = serde_json::from_slice(
        &std::fs::read(home.join("state/spaces/default/space.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(desc["legacy"], true, "{desc}");
    let (st, l) = d
        .call(
            Method::GET,
            "/api/v1/espacio/admin/spaces?adoptable=true",
            Some(ROOT),
            None,
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{l}");
    assert_eq!(l["default"]["legacy"], true);
    assert!(l["adoptable"].is_array());
    assert_eq!(l["spaces"].as_array().unwrap().len(), 0, "{l}");

    // --- spaces and tokens ---
    let ta = create_space(&d, "esp_a", false).await;
    let tb = create_space(&d, "esp_b", false).await;
    let tc = create_space(&d, "esp_c", true).await; // encrypting
    let td = create_space(&d, "esp_d", false).await; // non-member, trashed later

    // No / wrong credential on the admin plane.
    for tok in [None, Some("nope")] {
        let (st, _) = d
            .call(Method::GET, "/api/v1/espacio/admin/spaces", tok, None)
            .await;
        assert_eq!(st, StatusCode::UNAUTHORIZED);
    }

    // --- A/B memory isolation, including identical content and paths ---
    d.add(&ta, "notes/a", "alphaonly quokka").await;
    d.add(&tb, "pub/b", "betaonly wombat").await;
    d.add(&tb, "priv/b", "betaprivate wombat").await;
    d.add(&ta, "same/p", "sharedtext zebra from a").await;
    d.add(&tb, "same/p", "sharedtext zebra from b").await;
    let a = d.search(&ta, "alphaonly quokka", false).await;
    assert!(results(&a).contains("alphaonly"), "{a}");
    for q in ["betaonly wombat", "betaprivate wombat"] {
        let r = d.search(&ta, q, false).await;
        assert!(!results(&r).contains("wombat"), "A saw B: {r}");
    }
    let r = d.search(&tb, "alphaonly quokka", false).await;
    assert!(!results(&r).contains("quokka"), "B saw A: {r}");
    let ra = d.search(&ta, "sharedtext zebra", false).await;
    let rb = d.search(&tb, "sharedtext zebra", false).await;
    assert!(
        results(&ra).contains("from a") && !results(&ra).contains("from b"),
        "{ra}"
    );
    assert!(
        results(&rb).contains("from b") && !results(&rb).contains("from a"),
        "{rb}"
    );
    // Root serves the DEFAULT workspace: it sees neither space.
    let rr = d.search(ROOT, "alphaonly quokka", false).await;
    assert!(
        !results(&rr).contains("quokka"),
        "root default saw a space: {rr}"
    );

    // --- one-way read-only link B -> A ---
    let links_b = "/api/v1/espacio/spaces/esp_b/links";
    let (st, lk) = d
        .call(
            Method::POST,
            links_b,
            Some(&tb),
            Some(json!({"target_space": "esp_a", "path_prefix": "pub/"})),
        )
        .await;
    assert_eq!(st, StatusCode::CREATED, "{lk}");
    let link_id = lk["link"]["id"].as_str().unwrap().to_string();
    let r = d.search(&ta, "betaonly wombat", true).await;
    assert!(results(&r).contains("betaonly"), "linked read failed: {r}");
    assert!(sources(&r).contains(&"esp_b".to_string()), "{r}");
    let r = d.search(&ta, "betaprivate wombat", true).await;
    assert!(!results(&r).contains("betaprivate"), "filter leaked: {r}");
    // Without opting in, nothing of B is merged.
    let r = d.search(&ta, "betaonly wombat", false).await;
    assert!(!results(&r).contains("betaonly"), "{r}");
    // Reverse direction: B cannot read A.
    let r = d.search(&tb, "alphaonly quokka", true).await;
    assert!(
        !results(&r).contains("quokka"),
        "B read A through B's own link: {r}"
    );
    // A cannot manage B's links, nor write into B through the link.
    let (st, _) = d
        .call(
            Method::POST,
            links_b,
            Some(&ta),
            Some(json!({"target_space": "esp_a"})),
        )
        .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    d.add(&ta, "x/leak", "intruder emu").await;
    let r = d.search(&tb, "intruder emu", false).await;
    assert!(!results(&r).contains("intruder"), "A wrote into B: {r}");
    // Non-member (D, no link) gets zero from A and B even when opting in.
    for q in ["alphaonly quokka", "betaonly wombat"] {
        let r = d.search(&td, q, true).await;
        assert_eq!(r["count"], 0, "non-member saw data: {r}");
    }

    // --- revoke ---
    let (st, _) = d
        .call(
            Method::DELETE,
            &format!("{links_b}/{link_id}"),
            Some(&tb),
            None,
        )
        .await;
    assert_eq!(st, StatusCode::OK);
    let r = d.search(&ta, "betaonly wombat", true).await;
    assert!(
        !results(&r).contains("betaonly"),
        "revoked link still reads: {r}"
    );

    // --- admin plane is root-only: JWT, ephemeral session, xav_, xsp_ ---
    let admin_user = User::new("a@swal.local".into(), "Admin".into(), UserRole::Admin);
    let jwt = generate_jwt(&admin_user, JWT_SECRET.as_bytes()).unwrap();
    let (st, sess) = d
        .call(Method::POST, "/v1/auth/session", Some(ROOT), None)
        .await;
    assert_eq!(st, StatusCode::OK, "{sess}");
    let ephemeral = sess["session_id"].as_str().expect("session id").to_string();
    let (st, xt) = d
        .call(
            Method::POST,
            "/security/tokens",
            Some(ROOT),
            Some(json!({"name": "e2e", "scopes": ["all"]})),
        )
        .await;
    assert_eq!(st, StatusCode::CREATED, "{xt}");
    let xav = xt["token"].as_str().expect("xav_ token").to_string();
    assert!(xav.starts_with("xav_"), "{xav}");
    for (label, tok) in [
        ("jwt", jwt.as_str()),
        ("ephemeral", ephemeral.as_str()),
        ("xav_", xav.as_str()),
        ("xsp_ admin of another space", ta.as_str()),
    ] {
        for (m, p) in [
            (Method::GET, "/api/v1/espacio/admin/spaces"),
            (Method::GET, "/api/v1/espacio/admin/spaces/esp_a"),
        ] {
            let (st, b) = d.call(m, p, Some(tok), None).await;
            assert!(
                st == StatusCode::FORBIDDEN || st == StatusCode::UNAUTHORIZED,
                "{label} reached the admin plane on {p}: {st} {b}"
            );
        }
        let (st, b) = d
            .call(
                Method::POST,
                "/api/v1/espacio/admin/spaces",
                Some(tok),
                Some(json!({"id":"esp_evil","name":"x","owner_node":"x"})),
            )
            .await;
        assert!(
            st == StatusCode::FORBIDDEN || st == StatusCode::UNAUTHORIZED,
            "{label} created a space: {st} {b}"
        );
    }
    let (st, _) = d
        .call(
            Method::GET,
            "/api/v1/espacio/admin/spaces/esp_evil",
            Some(ROOT),
            None,
        )
        .await;
    assert_eq!(
        st,
        StatusCode::NOT_FOUND,
        "a non-root credential created a space"
    );

    // --- encrypted space: memory is served and sealed with the space's own key ---
    let (st, b) = d
        .call(
            Method::POST,
            "/memory/add",
            Some(&tc),
            Some(json!({"content": "secretc", "path": "c/1"})),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{b}");
    let (st, b) = d
        .call(
            Method::POST,
            "/memory/search",
            Some(&tc),
            Some(json!({"query": "secretc"})),
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{b}");
    assert!(b.to_string().contains("secretc"), "{b}");
    let space_dir = home.join("state/spaces/esp_c");
    for name in ["memory.sqlite", "memory.sqlite-wal"] {
        if let Ok(bytes) = std::fs::read(space_dir.join(name)) {
            assert!(
                !bytes.windows(7).any(|w| w == b"secretc"),
                "plaintext in {name} of an encrypting space"
            );
        }
    }

    // --- trashed space: its token stops working at once ---
    let (st, b) = d
        .call(
            Method::DELETE,
            "/api/v1/espacio/admin/spaces/esp_d?confirm=esp_d",
            Some(ROOT),
            None,
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{b}");
    let (st, _) = d
        .call(
            Method::POST,
            "/memory/search",
            Some(&td),
            Some(json!({"query": "x"})),
        )
        .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
    assert!(
        home.join("state/spaces/.trash").is_dir(),
        "trash, not removal"
    );

    // --- restart: persistence + isolation hold ---
    drop(d);
    let d = Daemon::spawn(&bin.0, &home, false).await;
    let (st, l) = d
        .call(
            Method::GET,
            "/api/v1/espacio/admin/spaces",
            Some(ROOT),
            None,
        )
        .await;
    assert_eq!(st, StatusCode::OK, "{l}");
    let mut ids: Vec<String> = l
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap().to_string())
        .collect();
    ids.sort();
    assert_eq!(
        ids,
        ["esp_a", "esp_b", "esp_c"],
        "esp_d must stay trashed: {l}; descriptor now: {}",
        std::fs::read_to_string(home.join("state/spaces/default/space.json")).unwrap_or_default()
    );
    let (st, _) = d
        .call(
            Method::POST,
            "/memory/search",
            Some(&td),
            Some(json!({"query": "x"})),
        )
        .await;
    assert_eq!(
        st,
        StatusCode::UNAUTHORIZED,
        "trashed token revived by restart"
    );
    let r = d.search(&ta, "alphaonly quokka", false).await;
    assert!(results(&r).contains("alphaonly"), "A lost its memory: {r}");
    let r = d.search(&ta, "betaonly wombat", true).await;
    assert!(
        !results(&r).contains("betaonly"),
        "revoked link came back after restart: {r}"
    );
    let r = d.search(&tb, "alphaonly quokka", true).await;
    assert!(!results(&r).contains("quokka"), "{r}");
    let (st, _) = d
        .call(Method::GET, "/api/v1/espacio/admin/spaces", Some(&ta), None)
        .await;
    assert_eq!(st, StatusCode::FORBIDDEN);
    // A fresh link works against the reloaded grantor.
    let (st, lk) = d
        .call(
            Method::POST,
            links_b,
            Some(&tb),
            Some(json!({"target_space": "esp_a", "path_prefix": "pub/"})),
        )
        .await;
    assert_eq!(st, StatusCode::CREATED, "{lk}");
    let r = d.search(&ta, "betaonly wombat", true).await;
    assert!(results(&r).contains("betaonly"), "{r}");
    // The legacy record is still the same single descriptor.
    let desc2: Value = serde_json::from_slice(
        &std::fs::read(home.join("state/spaces/default/space.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(desc2, desc, "legacy descriptor rewritten on restart");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn spaces_off_leaves_the_legacy_path_unchanged() {
    let (_keep, home, bin) = scratch();
    let d = Daemon::spawn(&bin.0, &home, true).await;
    // Espacio is disabled: 503 for root, and nothing was created on disk.
    let (st, _) = d
        .call(
            Method::GET,
            "/api/v1/espacio/admin/spaces",
            Some(ROOT),
            None,
        )
        .await;
    assert_eq!(st, StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        !home.join("state/spaces").exists(),
        "XAVIER_SPACES=off must not create the spaces dir or a legacy descriptor"
    );
    // The legacy memory path (root token, default workspace) works as before.
    d.add(ROOT, "legacy/1", "legacyonly narwhal").await;
    let r = d.search(ROOT, "legacyonly narwhal", false).await;
    assert!(results(&r).contains("narwhal"), "{r}");
    // An xsp_ token is just an invalid credential when spaces are off.
    let (st, _) = d
        .call(
            Method::POST,
            "/memory/search",
            Some("xsp_esp_a_forged"),
            Some(json!({"query": "x"})),
        )
        .await;
    assert_eq!(st, StatusCode::UNAUTHORIZED);
}
