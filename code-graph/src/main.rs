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
use code_graph::db::CodeGraphDB;
use code_graph::indexer::Indexer;
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
        Some(token) if token == state.token => Ok(next.run(request).await),
        _ => Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "Invalid or missing token".to_string(),
            }),
        )),
    }
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

    match state.indexer.index(&validated_path).await {
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
        },
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
            let symbols = result.symbols.into_iter().map(|s| {
                SymbolEntry {
                    name: s.name,
                    kind: format!("{:?}", s.kind),
                    file: s.file_path,
                    line: s.start_line as usize,
                    lang: format!("{:?}", s.lang),
                }
            }).collect();
            Json(FindResponse {
                count: result.total,
                symbols,
            })
        },
        Err(_) => Json(FindResponse {
            count: 0,
            symbols: vec![],
        }),
    }
}

async fn stats(State(state): State<AppState>) -> Result<Json<ScanResponse>, (StatusCode, Json<ErrorResponse>)> {
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
        },
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let db_path = cli.db_path.or_else(|| {
        std::env::var("CODE_GRAPH_DB_PATH").ok().map(PathBuf::from)
    }).unwrap_or_else(|| PathBuf::from("code_graph.db"));

    let db = Arc::new(CodeGraphDB::new(&db_path)?);
    let indexer = Arc::new(Indexer::new(Arc::clone(&db)));
    let query_engine = Arc::new(QueryEngine::new(Arc::clone(&db)));

    // Server mode
    if cli.command.is_none() || matches!(cli.command, Some(Commands::Serve)) {
        let token = cli.token.unwrap_or_else(|| {
            std::env::var("CODE_GRAPH_TOKEN")
                .unwrap_or_else(|_| "default-token-change-me".to_string())
        });

        if token == "default-token-change-me" {
            eprintln!("⚠️  WARNING: Using default token. Set CODE_GRAPH_TOKEN env var for security.");
        }

        let state = AppState {
            token: token.clone(),
            indexer,
            query_engine,
        };

        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        let protected_routes = Router::new()
            .route("/code/scan", post(scan))
            .route("/code/find", post(find))
            .route("/code/stats", get(stats))
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

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app).await?;

        return Ok(());
    }

    // CLI mode
    match cli.command.expect("test assertion") {
        Commands::Serve => unreachable!(),

        Commands::Scan { path } => {
            println!("🔍 Scanning: {:?}", path);
            let stats = indexer.index(&path).await?;

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
                println!("  - {} ({:?}) in {}:{}", sym.name, sym.kind, sym.file_path, sym.start_line);
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
    }

    Ok(())
}
