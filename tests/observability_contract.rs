//! Observability contract gate — WAVE-29.10.
//!
//! Boots a real `xavier http` server on an ephemeral port against a temp data
//! dir and asserts the contracts the reporting layer must keep:
//!
//!   1. `GET /health` describes the embedder it probed, and the MCP
//!      `health_check` tool agrees on it (WAVE-29.01 / #2554).
//!   2. `GET /health` names every reason it is not healthy, and names none when
//!      it is (WAVE-29.01 / #2554, `degraded_reasons`).
//!   3. `embedding_coverage` never reports 100% over an empty store
//!      (WAVE-29.01 / #2554).
//!   4. The server-reported version is the real build version
//!      (WAVE-29.07 / #2560).
//!   5. The MCP `stats` tool publishes its documented counters.
//!
//! Every field read goes through a helper that asserts presence AND type
//! first: a renamed or dropped field must fail the suite, never silently
//! default. `unwrap_or(false)`-style accessors are what made the previous
//! version of this file pass while asserting nothing.
//!
//! Hermetic: no Ollama, no network, no live :8006, temp data dir, explicit
//! request timeouts, one spawned server per test killed on drop.

use reqwest::Client;
use serde_json::Value;
use std::fs::File;
use std::net::TcpListener;
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

const TOKEN: &str = "observability-contract-token";
/// Every HTTP call is bounded: a wedged server fails the test instead of
/// hanging the single-threaded CI job until the job timeout.
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);
/// Upper bound on the server boot poll, then the test panics with the server log.
const BOOT_TIMEOUT: Duration = Duration::from_secs(90);
const BOOT_POLL_INTERVAL: Duration = Duration::from_millis(500);

struct ServerInstance {
    _data_dir: tempfile::TempDir,
    child: Child,
    log_path: std::path::PathBuf,
}

impl Drop for ServerInstance {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl ServerInstance {
    /// Last log lines, so a boot failure says why instead of only that it happened.
    fn log_tail(&self) -> String {
        let log = std::fs::read_to_string(&self.log_path).unwrap_or_default();
        log.lines().rev().take(20).collect::<Vec<_>>().join("\n")
    }
}

fn test_client() -> Client {
    Client::builder()
        .timeout(HTTP_TIMEOUT)
        .build()
        .expect("build reqwest client")
}

async fn start_server() -> (u16, ServerInstance) {
    let data_dir = tempfile::tempdir().expect("temp data dir");
    let port = TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("read local addr")
        .port();

    let code_db = data_dir.path().join("code.db");
    let mem_db = data_dir.path().join("memory.db");
    let config_path = data_dir.path().join("config.json");
    std::fs::write(&config_path, "{}").expect("write config file");
    let log_path = data_dir.path().join("server.log");
    let log = File::create(&log_path).expect("create server log");

    // Hermetic: state, data and cwd all live in the temp dir, never ~/.xavier.
    let cwd = data_dir.path().join("cwd");
    std::fs::create_dir_all(&cwd).expect("create isolated cwd");

    let child = std::process::Command::new(env!("CARGO_BIN_EXE_xavier"))
        .arg("http")
        .arg(port.to_string())
        // Standalone MCP listener disabled: the suite talks to /mcp over HTTP.
        .arg("--mcp-port")
        .arg("0")
        .current_dir(&cwd)
        .env("XAVIER_HOME", data_dir.path())
        .env("XAVIER_STATE_DIR", data_dir.path())
        .env("XAVIER_DATA_DIR", data_dir.path().join("data"))
        .env("XAVIER_HOST", "127.0.0.1")
        .env("XAVIER_URL", format!("http://127.0.0.1:{port}"))
        .env("XAVIER_PORT", port.to_string())
        .env("XAVIER_MCP_PORT", "0")
        .env("XAVIER_CODE_GRAPH_DB_PATH", &code_db)
        .env("XAVIER_MEMORY_VEC_PATH", &mem_db)
        .env("XAVIER_CONFIG_PATH", &config_path)
        .env("XAVIER_TOKEN", TOKEN)
        .env("XAVIER_ENABLE_MCP", "true")
        .stdout(Stdio::from(log.try_clone().expect("clone log handle")))
        .stderr(Stdio::from(log))
        .spawn()
        .expect("spawn xavier http");

    let server = ServerInstance {
        _data_dir: data_dir,
        child,
        log_path,
    };

    let url = format!("http://127.0.0.1:{port}");
    let client = test_client();
    let deadline = Instant::now() + BOOT_TIMEOUT;
    let mut last_error = "no request attempted".to_string();
    while Instant::now() < deadline {
        match client.get(format!("{url}/health")).send().await {
            Ok(resp) if resp.status().is_success() => return (port, server),
            Ok(resp) => last_error = format!("GET /health -> HTTP {}", resp.status()),
            Err(err) => last_error = err.to_string(),
        }
        tokio::time::sleep(BOOT_POLL_INTERVAL).await;
    }
    panic!(
        "xavier http did not answer GET /health within {BOOT_TIMEOUT:?} (last: {last_error})\n\
         --- server log (tail) ---\n{}",
        server.log_tail()
    );
}

/// Authenticated GET that must return a success status and a JSON object.
async fn get_json(client: &Client, url: &str) -> Value {
    let resp = client
        .get(url)
        .header("Authorization", format!("Bearer {TOKEN}"))
        .header("X-Xavier-Token", TOKEN)
        .send()
        .await
        .unwrap_or_else(|err| panic!("GET {url} failed: {err}"));
    let status = resp.status();
    assert!(status.is_success(), "GET {url} returned HTTP {status}");
    let body: Value = resp
        .json()
        .await
        .unwrap_or_else(|err| panic!("GET {url} returned a non-JSON body: {err}"));
    assert!(
        body.is_object(),
        "GET {url} returned a non-object body: {body}"
    );
    body
}

/// MCP `tools/call` for a tool with no required arguments.
async fn call_mcp_tool(client: &Client, url: &str, tool: &str) -> Value {
    let resp = client
        .post(url)
        .header("Authorization", format!("Bearer {TOKEN}"))
        .header("X-Xavier-Token", TOKEN)
        .header("Content-Type", "application/json")
        .body(serde_json::json!({ "name": tool, "arguments": {} }).to_string())
        .send()
        .await
        .unwrap_or_else(|err| panic!("MCP call to tool {tool} failed: {err}"));
    let status = resp.status();
    assert!(
        status.is_success(),
        "MCP call to tool {tool} returned HTTP {status}"
    );
    let body: Value = resp
        .json()
        .await
        .unwrap_or_else(|err| panic!("MCP call to tool {tool} returned non-JSON: {err}"));
    assert!(
        body.get("error").is_none(),
        "MCP call to tool {tool} returned an error envelope: {body}"
    );
    body
}

// ── Presence + type asserting accessors ─────────────────────────────
// Each one panics with the endpoint and the source-of-truth struct in the
// message, so a shape change is a loud failure, never a silent default.

fn object_field<'a>(value: &'a Value, key: &str, ctx: &str) -> &'a Value {
    let field = value
        .get(key)
        .unwrap_or_else(|| panic!("{ctx}: required object field `{key}` is missing: {value}"));
    assert!(
        field.is_object(),
        "{ctx}: field `{key}` must be an object, got: {field}"
    );
    field
}

fn array_field<'a>(value: &'a Value, key: &str, ctx: &str) -> &'a Vec<Value> {
    let field = value
        .get(key)
        .unwrap_or_else(|| panic!("{ctx}: required array field `{key}` is missing: {value}"));
    field
        .as_array()
        .unwrap_or_else(|| panic!("{ctx}: field `{key}` must be an array, got: {field}"))
}

fn str_field<'a>(value: &'a Value, key: &str, ctx: &str) -> &'a str {
    let field = value
        .get(key)
        .unwrap_or_else(|| panic!("{ctx}: required string field `{key}` is missing: {value}"));
    field
        .as_str()
        .unwrap_or_else(|| panic!("{ctx}: field `{key}` must be a string, got: {field}"))
}

fn bool_field(value: &Value, key: &str, ctx: &str) -> bool {
    let field = value
        .get(key)
        .unwrap_or_else(|| panic!("{ctx}: required boolean field `{key}` is missing: {value}"));
    field
        .as_bool()
        .unwrap_or_else(|| panic!("{ctx}: field `{key}` must be a boolean, got: {field}"))
}

fn u64_field(value: &Value, key: &str, ctx: &str) -> u64 {
    let field = value
        .get(key)
        .unwrap_or_else(|| panic!("{ctx}: required counter field `{key}` is missing: {value}"));
    field
        .as_u64()
        .unwrap_or_else(|| panic!("{ctx}: field `{key}` must be a non-negative integer: {field}"))
}

fn f64_field(value: &Value, key: &str, ctx: &str) -> f64 {
    let field = value
        .get(key)
        .unwrap_or_else(|| panic!("{ctx}: required numeric field `{key}` is missing: {value}"));
    field
        .as_f64()
        .unwrap_or_else(|| panic!("{ctx}: field `{key}` must be a number, got: {field}"))
}

/// `HealthLevel` is `#[serde(rename_all = "lowercase")]`; a renamed variant must
/// break the contract, not pass unnoticed.
const HEALTH_LEVELS: [&str; 3] = ["healthy", "degraded", "unhealthy"];

/// Assertion 1: `/health` describes the embedder it probed consistently with
/// its own status, and the MCP `health_check` payload is complete.
///
/// Source of truth: `/health` is `HealthStatus` serialized as-is by
/// `src/cli/handlers/system.rs::health_handler`, so `embedding` is
/// `observability::health::EmbeddingHealth` (filled by
/// `HealthMonitor::check_embedding`). `embeddingOk` comes from a DIFFERENT
/// subsystem — a live probe in `collect_health_sync()` — and a probe that
/// answers does not imply `encode` works, so the two are not compared.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_observability_embedding_consistency() {
    let (port, _server) = start_server().await;
    let url = format!("http://127.0.0.1:{port}");
    let client = test_client();

    let health = get_json(&client, &format!("{url}/health")).await;
    let embedding = object_field(&health, "embedding", "GET /health (EmbeddingHealth)");
    let http_status = str_field(embedding, "status", "GET /health embedding (HealthLevel)");
    let http_available = bool_field(embedding, "available", "GET /health embedding");
    assert!(
        HEALTH_LEVELS.contains(&http_status),
        "GET /health embedding.status must be one of {HEALTH_LEVELS:?}, got: {http_status}"
    );

    // Intra-payload honesty: `check_embedding` only reports `Healthy` on a
    // successful encode and only reports `Unhealthy` on a failed one, both with
    // the matching `available`. A payload that claims a healthy embedder while
    // reporting it unavailable is the shape of the false `degraded` /
    // false `healthy` reports this wave fixed (#2554).
    assert!(
        http_status != "healthy" || http_available,
        "GET /health reports embedding.status=healthy while embedding.available=false"
    );
    assert!(
        http_status != "unhealthy" || !http_available,
        "GET /health reports embedding.status=unhealthy while embedding.available=true"
    );

    let mcp_health = call_mcp_tool(&client, &format!("{url}/mcp/tools/call"), "health_check").await;
    let structured = object_field(
        &mcp_health,
        "structuredContent",
        "MCP health_check (MCPHealthResult)",
    );
    let mcp_status = str_field(structured, "status", "MCP health_check (MCPHealthResult)");
    let mcp_protocol = str_field(
        structured,
        "mcpProtocol",
        "MCP health_check (MCPHealthResult)",
    );
    let mcp_tools = u64_field(
        structured,
        "toolsCount",
        "MCP health_check (MCPHealthResult)",
    );
    let mcp_store_ok = bool_field(structured, "memoryStoreOk", "MCP health_check");
    bool_field(structured, "embeddingOk", "MCP health_check");
    bool_field(structured, "handshakeOk", "MCP health_check");

    assert!(
        !mcp_status.is_empty(),
        "MCP health_check reported an empty status"
    );
    assert!(
        !mcp_protocol.is_empty(),
        "MCP health_check reported an empty mcpProtocol"
    );
    assert!(
        mcp_tools > 0,
        "MCP health_check reported toolsCount=0 for a server that serves tools"
    );
    assert!(
        mcp_store_ok,
        "MCP health_check reports the memory store as unusable on a live server"
    );
}

/// Assertion 2: the top-level status is not `degraded` while every subsystem is
/// healthy — or the payload names the reason.
///
/// `degraded_reasons` is part of `HealthStatus` (WAVE-29.01 / #2554), so a
/// missing field is a failure, not a skip.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_observability_degraded_status_logic() {
    let (port, _server) = start_server().await;
    let url = format!("http://127.0.0.1:{port}");
    let client = test_client();

    let health = get_json(&client, &format!("{url}/health")).await;
    let overall = str_field(&health, "status", "GET /health (HealthStatus)");
    assert!(
        HEALTH_LEVELS.contains(&overall),
        "GET /health status must be one of {HEALTH_LEVELS:?}, got: {overall}"
    );
    let reasons = array_field(
        &health,
        "degraded_reasons",
        "GET /health (HealthStatus, WAVE-29.01 / #2554)",
    );

    assert_eq!(
        overall == "healthy",
        reasons.is_empty(),
        "GET /health status={overall} with degraded_reasons={reasons:?}: a healthy node must name \
         no reason and any other level must name at least one (WAVE-29.01 / #2554)"
    );

    for reason in reasons {
        let reason = reason.as_str().unwrap_or_else(|| {
            panic!("GET /health degraded_reasons entries must be strings, got: {reason}")
        });
        assert!(
            reason.starts_with("host:") || reason.starts_with("subsystem:"),
            "GET /health degraded reason '{reason}' must be tagged host:<check> or \
             subsystem:<name> (HealthStatus::compute_degraded_reasons)"
        );
    }
}

/// Assertion 3: `embedding_coverage` never reports 100% over an empty store.
///
/// Coverage is NOT part of `HealthStatus`; the only live endpoint that
/// publishes it is `GET /v1/memory/recall/stats` (`src/server/v1_api.rs`).
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_observability_embedding_coverage_anomaly() {
    let (port, _server) = start_server().await;
    let url = format!("http://127.0.0.1:{port}");
    let client = test_client();

    let stats = get_json(&client, &format!("{url}/v1/memory/recall/stats")).await;
    let coverage = object_field(
        &stats,
        "embedding_coverage",
        "GET /v1/memory/recall/stats (EmbeddingCoverage)",
    );
    let indexed = u64_field(coverage, "indexed", "GET /v1/memory/recall/stats coverage");
    let total = u64_field(coverage, "total", "GET /v1/memory/recall/stats coverage");
    let percent = f64_field(coverage, "percent", "GET /v1/memory/recall/stats coverage");
    let coverage_status = str_field(coverage, "status", "GET /v1/memory/recall/stats coverage");

    // Both counters are COUNTs over the same `memory_records` table, the second
    // one filtered (`length(embedding) > 10`), so `indexed` is a subset of
    // `total` and the percentage is a share of `total`.
    assert!(
        indexed <= total,
        "embedding_coverage.indexed ({indexed}) must be <= total ({total})"
    );
    assert!(
        (0.0..=100.0).contains(&percent),
        "embedding_coverage.percent must be within 0..=100, got: {percent}"
    );
    assert!(
        !(percent == 100.0 && total == 0),
        "embedding_coverage reports 100% over an empty store (WAVE-29.01 / #2554)"
    );

    // An unmeasured store is `unknown`, never `healthy` (WAVE-29.01 / #2554).
    if total == 0 {
        assert_eq!(
            percent, 0.0,
            "embedding_coverage with total=0 must report percent 0.0"
        );
        assert_eq!(
            indexed, 0,
            "embedding_coverage with total=0 must report indexed 0"
        );
        assert_eq!(
            coverage_status, "unknown",
            "embedding_coverage with total=0 must report status unknown"
        );
    }
}

/// Assertion 4: the server-reported version is the real build version
/// (WAVE-29.07 / #2560). `HealthStatus.version` is `CARGO_PKG_VERSION`; when it
/// was absent, `xavier health` printed `Version: unknown`.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_observability_version_match() {
    let (port, _server) = start_server().await;
    let url = format!("http://127.0.0.1:{port}");
    let client = test_client();

    let health = get_json(&client, &format!("{url}/health")).await;
    let server_version = str_field(
        &health,
        "version",
        "GET /health (HealthStatus, WAVE-29.07 / #2560)",
    );

    assert!(
        !server_version.is_empty() && server_version != "unknown",
        "GET /health version must carry the real build version, got: '{server_version}'"
    );
    assert_eq!(
        server_version,
        env!("CARGO_PKG_VERSION"),
        "GET /health version must equal CARGO_PKG_VERSION (WAVE-29.07 / #2560)"
    );
}

/// Assertion 5: the MCP `stats` tool publishes its documented counters
/// (`src/server/mcp/tools_memory.rs`).
///
/// No relation between the counters is asserted: they come from four different
/// sources (store usage, working-memory length, entity graph, semantic memory),
/// so cross-counter inequalities are not invariants — one memory can yield many
/// entities. What must hold is that the tool reports all five, as integers,
/// without flagging itself as an error.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn test_observability_mcp_stats_surface() {
    let (port, _server) = start_server().await;
    let url = format!("http://127.0.0.1:{port}");
    let client = test_client();

    let mcp_stats = call_mcp_tool(&client, &format!("{url}/mcp/tools/call"), "stats").await;
    let is_error = mcp_stats
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| {
            panic!(
                "MCP stats result must carry a boolean isError, got: {}",
                mcp_stats["isError"]
            )
        });
    assert!(!is_error, "MCP stats tool reported an error: {mcp_stats}");

    let content = array_field(&mcp_stats, "content", "MCP stats (MCPToolResult)");
    let first = content
        .first()
        .unwrap_or_else(|| panic!("MCP stats result carried no content item: {mcp_stats}"));
    let text = first
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("MCP stats content[0] must be a text block: {first}"));
    let stats: Value = serde_json::from_str(text)
        .unwrap_or_else(|err| panic!("MCP stats text payload is not JSON ({err}): {text}"));

    const COUNTERS: [&str; 5] = [
        "total_memories",
        "storage_bytes",
        "total_entities",
        "semantic_entities",
        "semantic_relations",
    ];
    for counter in COUNTERS {
        u64_field(&stats, counter, "MCP stats tool (tools_memory.rs)");
    }
}
