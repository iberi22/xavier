//! CLI command enums and subcommand definitions
//!
//! This module defines the [`Command`] enum and all related subcommand enums
//! used for parsing CLI arguments via clap.

use clap::Subcommand;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

/// Global HTTP client shared across all CLI commands
pub static CLI_HTTP_CLIENT: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("xavier-cli/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("failed to build HTTP client")
});

/// Top-level CLI commands for Xavier.
///
/// Each variant maps to a distinct subcommand exposed to the user.
#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    /// Start Xavier HTTP server
    Http { port: Option<u16> },
    /// Start Xavier MCP-stdio server
    Mcp,
    /// Search memories
    Search {
        query: String,
        limit: Option<usize>,
        #[arg(long)]
        cluster: Vec<String>,
        #[arg(long)]
        level: Vec<String>,
    },
    /// Add a memory
    Add {
        content: String,
        title: Option<String>,
        /// Memory type: episodic, semantic, procedural, fact, decision, etc.
        #[arg(short, long)]
        kind: Option<String>,
        #[arg(long)]
        cluster: Option<String>,
        #[arg(long)]
        level: Option<String>,
        #[arg(long)]
        relation: Option<String>,
    },
    /// Recall memories with score-based display
    Recall {
        query: String,
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
    },
    /// Export structured context pack (.xcp) for LLMs
    ExportPack {
        #[arg(short, long)]
        topic: String,
        #[arg(short, long, default_value_t = 3)]
        max_level: usize,
        #[arg(short, long)]
        out: PathBuf,
    },
    /// Show statistics
    Stats,
    /// Query Xavier code graph
    Code {
        #[command(subcommand)]
        cmd: CodeCommand,
    },
    /// Save current session context to Xavier
    SessionSave { session_id: String, content: String },
    /// Spawn multiple agents with provider routing
    Spawn {
        #[arg(long, default_value_t = 1)]
        count: usize,
        #[arg(short, long)]
        provider: Vec<String>,
        #[arg(short, long)]
        model: Vec<String>,
        #[arg(short, long = "skill")]
        skills: Vec<String>,
        #[arg(short = 'x', long)]
        context: Vec<String>,
        #[arg(short, long)]
        task: Option<String>,
    },
    /// Launch parallel agents with a swarm configuration file (JSON)
    Swarm {
        #[arg(short, long)]
        config: PathBuf,
        #[arg(short, long, default_value_t = 4)]
        parallel: usize,
    },
    /// Batch spawn agents with provider/model routing
    MultiSpawn {
        #[arg(long, default_value_t = 10)]
        agents: usize,
        #[arg(long, default_value_t = 4)]
        batch: usize,
        #[arg(short, long)]
        provider: Vec<String>,
        #[arg(short, long)]
        model: Vec<String>,
        #[arg(short, long)]
        skills: Vec<String>,
        #[arg(short, long)]
        task: Option<String>,
    },
    /// Subcomando para gestionar Chronicle
    Chronicle {
        #[command(subcommand)]
        cmd: xavier::chronicle::cli::ChronicleCommand,
    },
    /// Manage ephemeral secrets (Clavis)
    Secrets {
        #[command(subcommand)]
        cmd: SecretsCommand,
    },
    /// Manage provider usage and rate limits
    Usage {
        #[command(subcommand)]
        cmd: UsageCommand,
    },
    /// Generate authentication tokens
    Token {
        #[command(subcommand)]
        cmd: TokenCommand,
    },
    /// Export memories to JSON
    Export {
        /// Export only public memories (exclude is_private: true)
        #[arg(long)]
        public: bool,
        /// Output file path
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

/// Provider usage subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum UsageCommand {
    /// Show current usage status for all providers
    Status,
    /// Manually update a provider's used percentage (for providers without API)
    Update { provider: String, percentage: f32 },
    /// Set a manual cooldown for a provider (in minutes)
    Cooldown { provider: String, minutes: i64 },
}

/// Code graph query subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum CodeCommand {
    /// Scan and index a codebase path
    Scan { path: String },
    /// Find symbols by name
    Find {
        query: String,
        #[arg(short, long, default_value_t = 10)]
        limit: usize,
        #[arg(short, long)]
        kind: Option<String>,
    },
    /// Find outgoing dependencies for a symbol/query
    Dependencies {
        query: String,
        #[arg(short, long, default_value_t = 3)]
        depth: usize,
        #[arg(short, long, default_value_t = 50)]
        limit: usize,
        #[arg(short, long)]
        edge_type: Option<String>,
    },
    /// Find incoming dependencies for a symbol/query
    ReverseDependencies {
        query: String,
        #[arg(short, long, default_value_t = 3)]
        depth: usize,
        #[arg(short, long, default_value_t = 50)]
        limit: usize,
        #[arg(short, long)]
        edge_type: Option<String>,
    },
    /// Trace a basic call chain
    CallChain {
        query: String,
        #[arg(short, long, default_value_t = 3)]
        depth: usize,
        #[arg(short, long, default_value_t = 50)]
        limit: usize,
    },
    /// Show highly connected symbols
    Hubs,
    /// Show complexity hotspots
    Hotspots,
    /// Show code graph stats
    Stats,
}

/// Token generation subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum TokenCommand {
    /// Generate a new random token for XAVIER_TOKEN
    New,
    /// Generate a signed HMAC token for a user
    Gen { user_id: String },
}

/// Secrets management subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum SecretsCommand {
    /// Lend a secret to an agent
    Lend {
        secret_name: String,
        agent: String,
        /// Time to live in seconds (default 3600)
        #[arg(short, long, default_value_t = 3600)]
        ttl: u64,
    },
    /// List all active secret leases
    ListLeases,
    /// Revoke a specific lease
    Revoke { token: String },
    /// Check the status of a lease
    Status { token: String },
}
