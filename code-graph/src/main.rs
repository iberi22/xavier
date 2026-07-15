//! CLI for code-graph - Server mode with HTTP token auth

use axum::{
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::Json,
    routing::{get, post},
    Router,
};
use clap::{Parser, Subcommand};
use code_graph::api::plugin_routes::{self, PluginApiState};
use code_graph::db::CodeGraphDB;
use code_graph::indexer::Indexer;
use code_graph::mcp::McpServer;
use code_graph::plugin::PluginManager;
use code_graph::query::QueryEngine;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

// ============================================================================
// State
// ============================================================================

#[derive(Clone)]
struct AppState {
    token: String,
    indexer: Arc<Indexer>,
    query_engine: Arc<QueryEngine>,
    manager: Arc<PluginManager>,
}

#[derive(Serialize, Deserialize, Clone)]
struct SymbolEntry {
    name: String,
    kind: String,
    file: String,
    line: usize,
    lang: String,
}

#[derive(Serialize, Deserialize)]
struct ScanRequest {
    path: String,
    incremental: Option<bool>,
}

#[derive(Serialize, Deserialize)]
struct ScanResponse {
    status: String,
    files: usize,
    symbols: usize,
    languages: HashMap<String, usize>,
    duration_ms: u64,
}

#[derive(Serialize, Deserialize)]
struct FindRequest {
    query: String,
    lang: Option<String>,
    limit: Option<usize>,
}

#[derive(Serialize, Deserialize)]
struct FindResponse {
    symbols: Vec<SymbolEntry>,
    count: usize,
}

#[derive(Serialize, Deserialize)]
struct ErrorResponse {
    error: String,
}

#[derive(Serialize, Deserialize)]
struct HealthResponse {
    status: String,
    version: String,
}

// ============================================================================
// Auth Middleware
// ============================================================================

async fn auth_middleware(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Result<axum::response::Response, (StatusCode, Json<ErrorResponse>)> {
    let auth_header = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim_start_matches("Bearer "));

    match auth_header {
        // Constant-time comparison to avoid leaking the token via timing.
        Some(token) if constant_time_eq(token.as_bytes(), state.token.as_bytes()) => {
            Ok(next.run(request).await)
        }
        _ => Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Invalid or missing token".to_string(),
            }),
        )),
    }
}

/// Constant-time byte-slice comparison. Compares over the length of the
/// *expected* value and always processes the full length, so a shorter
/// attacker-supplied value does not short-circuit early. Returns `false` if
/// the lengths differ.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Returns true when the bind host is loopback (so the server is not reachable
/// from the network). Used to relax the token-default policy. Note: `0.0.0.0`
/// intentionally counts as non-loopback because it binds to all interfaces.
fn is_loopback_host(host: &str) -> bool {
    matches!(
        host.to_ascii_lowercase().as_str(),
        "localhost" | "127.0.0.1" | "::1" | "[::1]"
    )
}

/// Generate a 32-byte random token, hex-encoded (64 chars). Seeded from the
/// current time and pid (xorshift PRNG). This is an EPHEMERAL fallback for when
/// no `CODE_GRAPH_TOKEN` is set and the server binds off-loopback; it is not a
/// substitute for setting an explicit token. The recommended path is always to
/// set `CODE_GRAPH_TOKEN`.
fn generate_ephemeral_token() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let mut buf = [0u8; 32];
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    // xorshift128-style fill over the seed.
    let mut state = nanos ^ (pid as u128).wrapping_mul(0x9E3779B97F4A7C15);
    for byte in buf.iter_mut() {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        *byte = (state >> 64) as u8 ^ (state as u8);
    }
    buf.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Build the CORS layer from `CODE_GRAPH_ALLOWED_ORIGINS` (comma-separated).
/// Defaults to allowing only localhost origins. A value of `*` restores the
/// previous permissive behavior (must be opted into explicitly).
fn build_cors_layer() -> CorsLayer {
    let raw = std::env::var("CODE_GRAPH_ALLOWED_ORIGINS").unwrap_or_default();
    let origins: Vec<String> = raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if origins.iter().any(|o| o == "*") {
        // Explicit opt-in to permissive CORS.
        return CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);
    }

    let allowed: Vec<_> = if origins.is_empty() {
        // Default: localhost only.
        vec![
            "http://localhost".to_string(),
            "http://127.0.0.1".to_string(),
        ]
    } else {
        origins
    };

    let parsed: Vec<_> = allowed
        .iter()
        .filter_map(|o| o.parse::<axum::http::HeaderValue>().ok())
        .collect();
    CorsLayer::new()
        .allow_origin(parsed)
        .allow_methods(Any)
        .allow_headers(Any)
}

// ============================================================================
// Security Helpers
// ============================================================================

/// Validate and canonicalize path to prevent path traversal
fn validate_path(base: &Path, requested: &str) -> Result<PathBuf, String> {
    // Reject null bytes and control characters
    if requested.contains('\0') || requested.chars().any(|c| c.is_control()) {
        return Err("Invalid characters in path".to_string());
    }

    let path = PathBuf::from(requested);

    // Get canonical path and verify it's within base
    // If path is relative, it will be relative to current dir
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("Invalid path: {}", e))?;

    let base_canonical = base
        .canonicalize()
        .map_err(|e| format!("Invalid base path: {}", e))?;

    // Verify the canonical path starts with base (prevents traversal)
    if !canonical.starts_with(&base_canonical) {
        return Err("Path traversal attempt detected".to_string());
    }

    Ok(canonical)
}

// ============================================================================
// Routes
// ============================================================================

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: "0.6.1-beta".to_string(),
    })
}

async fn scan(
    State(state): State<AppState>,
    Json(req): Json<ScanRequest>,
) -> Result<Json<ScanResponse>, (StatusCode, Json<ErrorResponse>)> {
    // For the sidecar, we might want to allow scanning any path if it's running standalone,
    // but the original code had some validation.
    // Let's use current dir as base if it's relative.
    let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let validated_path = match validate_path(&base, &req.path) {
        Ok(p) => p,
        Err(e) => {
            return Err((StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })));
        }
    };

    let incremental = req.incremental.unwrap_or(true);
    match state.indexer.index(&validated_path, incremental).await {
        Ok(stats) => {
            let mut languages = HashMap::new();
            for lang_count in stats.languages {
                languages.insert(format!("{:?}", lang_count.lang), lang_count.count as usize);
            }
            Ok(Json(ScanResponse {
                status: "ok".to_string(),
                files: stats.total_files as usize,
                symbols: stats.total_symbols as usize,
                languages,
                duration_ms: stats.duration_ms,
            }))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

async fn find(State(state): State<AppState>, Json(req): Json<FindRequest>) -> Json<FindResponse> {
    let limit = req.limit.unwrap_or(20).min(100);

    match state.query_engine.search(&req.query, limit) {
        Ok(result) => {
            let symbols = result
                .symbols
                .into_iter()
                .map(|s| SymbolEntry {
                    name: s.name,
                    kind: format!("{:?}", s.kind),
                    file: s.file_path,
                    line: s.start_line as usize,
                    lang: format!("{:?}", s.lang),
                })
                .collect();
            Json(FindResponse {
                count: result.total,
                symbols,
            })
        }
        Err(_) => Json(FindResponse {
            count: 0,
            symbols: vec![],
        }),
    }
}

async fn stats(
    State(state): State<AppState>,
) -> Result<Json<ScanResponse>, (StatusCode, Json<ErrorResponse>)> {
    match state.query_engine.stats() {
        Ok(stats) => {
            let mut languages = HashMap::new();
            for lang_count in stats.languages {
                languages.insert(format!("{:?}", lang_count.lang), lang_count.count as usize);
            }
            Ok(Json(ScanResponse {
                status: "ok".to_string(),
                files: stats.total_files as usize,
                symbols: stats.total_symbols as usize,
                languages,
                duration_ms: stats.duration_ms,
            }))
        }
        Err(e) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: e.to_string(),
            }),
        )),
    }
}

// ============================================================================
// CLI
// ============================================================================

#[derive(Parser)]
#[command(name = "code-graph")]
#[command(about = "Codebase Understanding without RAG - Tree-sitter + Agentic Search", long_about = None)]
struct Cli {
    /// HTTP Token for authentication (env: CODE_GRAPH_TOKEN)
    #[arg(long, env = "CODE_GRAPH_TOKEN")]
    token: Option<String>,

    /// Server port (env: CODE_GRAPH_PORT, default: 8080)
    #[arg(long, env = "CODE_GRAPH_PORT", default_value = "8080")]
    port: u16,

    /// Server host (env: CODE_GRAPH_HOST, default: 0.0.0.0)
    #[arg(long, env = "CODE_GRAPH_HOST", default_value = "0.0.0.0")]
    host: String,

    /// Database path (env: CODE_GRAPH_DB_PATH)
    #[arg(long, env = "CODE_GRAPH_DB_PATH")]
    db_path: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Start HTTP server (default when no command)
    Serve,

    /// Scan and index a codebase (CLI mode)
    Scan {
        /// Path to scan
        path: PathBuf,

        /// Disable incremental indexing
        #[arg(long)]
        no_incremental: bool,
    },

    /// Find symbols by name (CLI mode)
    Find {
        /// Search query
        query: String,

        /// Max results
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },

    /// Show statistics (CLI mode)
    Stats,

    /// Start MCP stdio server
    Mcp {
        /// Project root path to serve
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let db_path = cli
        .db_path
        .or_else(|| std::env::var("CODE_GRAPH_DB_PATH").ok().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("code_graph.db"));

    let db = Arc::new(CodeGraphDB::new(&db_path)?);
    let indexer = Arc::new(Indexer::new(Arc::clone(&db)));
    let query_engine = Arc::new(QueryEngine::new(Arc::clone(&db)));
    let manager = Arc::new(PluginManager::new());
    if let Err(e) = manager.load_config() {
        eprintln!("⚠️  Failed to load plugin config: {}", e);
    }

    // Server mode
    if cli.command.is_none() || matches!(cli.command, Some(Commands::Serve)) {
        let is_loopback = is_loopback_host(&cli.host);

        // Token resolution: explicit flag/env wins. Otherwise, when binding to
        // a non-loopback address we refuse the known-public default and generate
        // a fresh random token instead. On loopback the default is still allowed
        // (local-only development) but warns loudly.
        let default_token = "default-token-change-me".to_string();
        let mut token = cli
            .token
            .clone()
            .or_else(|| std::env::var("CODE_GRAPH_TOKEN").ok())
            .unwrap_or(default_token.clone());

        if token == default_token {
            if is_loopback {
                eprintln!(
                    "⚠️  WARNING: Using the known-public default token on loopback. \
                     Set CODE_GRAPH_TOKEN for anything beyond local development."
                );
            } else {
                // Non-loopback bind with the public default — generate an
                // ephemeral random token so the server never exposes an
                // unauthenticated surface to the network.
                token = generate_ephemeral_token();
                eprintln!(
                    "⚠️  No CODE_GRAPH_TOKEN set and binding to a non-loopback address. \
                     Generated an EPHEMERAL random token for this session (it will NOT \
                     persist across restarts). Set CODE_GRAPH_TOKEN explicitly."
                );
            }
        }

        let state = AppState {
            token: token.clone(),
            indexer,
            query_engine,
            manager: Arc::clone(&manager),
        };

        // CORS: configurable via CODE_GRAPH_ALLOWED_ORIGINS (comma-separated).
        // Defaults to localhost-only origins; `*` opts back into the old
        // permissive behavior explicitly.
        let cors = build_cors_layer();

        let plugin_state = PluginApiState {
            manager: Arc::clone(&manager),
            health: None,    // #485
            discovery: None, // #486
        };

        let protected_routes = Router::new()
            .route("/code/scan", post(scan))
            .route("/code/find", post(find))
            .route("/code/stats", get(stats))
            .nest("/", plugin_routes::router(plugin_state))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                auth_middleware,
            ));

        let app = Router::new()
            .route("/health", get(health))
            .merge(protected_routes)
            .layer(cors)
            .with_state(state);

        let addr: SocketAddr = format!("{}:{}", cli.host, cli.port).parse()?;

        println!("🚀 Starting code-graph server");
        println!(
            "🔐 Token: ***{} (length: {})",
            &token[token.len().saturating_sub(4)..],
            token.len()
        );
        println!("📍 Address: http://{}", addr);
        println!("🗄️  Database: {:?}", db_path);
        println!("\nEndpoints:");
        println!("  GET  /health          - Health check (public)");
        println!("  POST /code/scan        - Scan and index codebase (auth required)");
        println!("  POST /code/find        - Find symbols (auth required)");
        println!("  GET  /code/stats       - Get index statistics (auth required)");
        println!("  [Plugin Management API mounted at /api/v1/plugins]");

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;

        return Ok(());
    }

    // CLI mode
    match cli.command.expect("test assertion") {
        Commands::Serve => unreachable!(),

        Commands::Scan {
            path,
            no_incremental,
        } => {
            println!("🔍 Scanning: {:?}", path);
            let stats = indexer.index(&path, !no_incremental).await?;

            println!("\n✅ Indexed in {}ms", stats.duration_ms);
            println!("📁 Files: {}", stats.total_files);
            println!("🔤 Symbols: {}", stats.total_symbols);
            println!("\nLanguages:");
            for lang_count in stats.languages {
                println!("  {:?}: {}", lang_count.lang, lang_count.count);
            }
        }

        Commands::Find { query, limit } => {
            println!("🔍 Searching for: {}", query);
            let result = query_engine.search(&query, limit)?;
            println!("Found {} results (showing up to {}):", result.total, limit);
            for sym in result.symbols {
                println!(
                    "  - {} ({:?}) in {}:{}",
                    sym.name, sym.kind, sym.file_path, sym.start_line
                );
            }
        }

        Commands::Stats => {
            let stats = db.stats()?;
            println!("📊 code-graph v0.6.1-beta");
            println!("📁 Total Files: {}", stats.total_files);
            println!("🔤 Total Symbols: {}", stats.total_symbols);
            println!("🔗 Total Imports: {}", stats.total_imports);
            println!("\nLanguages:");
            for lang_count in stats.languages {
                println!("  {:?}: {}", lang_count.lang, lang_count.count);
            }
        }

        Commands::Mcp { path } => {
            let mcp_server = McpServer::new(indexer, query_engine, path);
            mcp_server.run().await?;
        }
    }

    Ok(())
}
