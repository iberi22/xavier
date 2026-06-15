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
        #[arg(short = 'n', long)]
        max_results: Option<usize>,
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
    /// Manage the hardware vault
    Vault {
        #[command(subcommand)]
        cmd: VaultCommand,
    },
    /// Manage provider usage and rate limits
    Usage {
        #[command(subcommand)]
        cmd: UsageCommand,
    },
    /// Show API usage and account balance
    Billing,
    /// List and synchronize Xavier tasks
    Tasks {
        #[command(subcommand)]
        cmd: TasksCommand,
    },
    /// Run Xavier system verification
    Verify {
        #[command(subcommand)]
        cmd: VerifyCommand,
    },
    /// Generate authentication tokens
    Token {
        #[command(subcommand)]
        cmd: TokenCommand,
    },
    /// Show API quotas and limits for providers
    Quota,
    /// Manage LLM providers and hot-switching
    Provider {
        #[command(subcommand)]
        cmd: ProviderCommand,
    },
    /// Run interactive system detection and setup
    Setup,

    /// Manage Xavier Data Commons and fine-tuning readiness
    DataCommons {
        #[command(subcommand)]
        cmd: DataCommonsCommand,
    },

    /// Manage Xavier sessions
    Session {
        #[command(subcommand)]
        cmd: SessionCommand,
    },

    /// Manage Xavier Mesh P2P connections
    Mesh {
        #[command(subcommand)]
        cmd: MeshCommand,
    },

    /// Export memories to JSON
    Export {
        /// Export only public memories (exclude is_private: true)
        #[arg(long)]
        public: bool,
        /// Output file path
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Limit the number of exported memories
        #[arg(short, long)]
        limit: Option<usize>,
    },

    /// List memories at current or specified path (navigation)
    Ls {
        /// Optional path to list
        path: Option<String>,
    },

    /// Change current working directory in memory (navigation)
    Cd {
        /// Path to navigate to
        path: String,
    },

    /// Show current working directory (navigation)
    Pwd,
    /// Navigation and impact analysis commands
    Nav {
        #[command(subcommand)]
        cmd: NavCommand,
    },
}

/// Navigation and impact analysis subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum NavCommand {
    /// List memories at current or specified path
    Ls {
        /// Optional path to list
        path: Option<String>,
    },
    /// Change current working directory in memory
    Cd {
        /// Path to navigate to
        path: String,
    },
    /// Show current working directory
    Pwd,
    /// Show nodes affected by a change to a document or concept
    Affected {
        /// Path to the document or name of the concept
        path: String,
        /// Maximum depth for BFS traversal
        #[arg(short, long, default_value_t = 2)]
        depth: usize,
        /// Output format: table or json
        #[arg(short, long, default_value = "table")]
        format: String,
        /// Filter results: 'code' to exclude code-related nodes
        #[arg(long)]
        exclude_file_type: Option<String>,
    },
    /// Render memory graph for debugging (tree, edges, weights)
    Visualize {
        /// Output format: text or json
        #[arg(short, long, default_value = "text")]
        format: String,
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

/// Task management subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum TasksCommand {
    /// List tasks from Xavier
    List {
        #[arg(short, long)]
        project: Option<String>,
        #[arg(short, long)]
        status: Option<String>,
        #[arg(short, long)]
        search: Option<String>,
    },
    /// Synchronize tasks with configured backends
    Sync,
}

/// Verification subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum VerifyCommand {
    /// Run a system scan and verification summary
    Scan {
        /// Output format: table, json, or markdown
        #[arg(short, long, default_value = "table")]
        format: String,
        /// Show detailed information including masked API key status
        #[arg(short, long)]
        detailed: bool,
    },
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

/// Provider management subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum ProviderCommand {
    /// Show current provider status
    Status,
    /// List all available providers and strategies
    List,
    /// Manually switch to a provider
    Set { name: String },
    /// Set an automatic selection strategy
    Auto { strategy: String },
    /// Set the fallback chain of providers
    Fallback { providers: Vec<String> },
}

/// System scan and discovery subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum SystemCommand {
    /// Scan for local AI services, CLI tools, GPU, and environment
    Scan {
        /// Output format: table, json, or markdown
        #[arg(short, long, default_value = "table")]
        format: String,
        /// Show detailed information including API key status (masked)
        #[arg(short, long)]
        detailed: bool,
    },
    /// Show system health summary
    Health,
    /// Detect available GPU and compute resources
    Gpu,
}

/// Token generation subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum TokenCommand {
    /// Generate a new random token for XAVIER_TOKEN
    New,
    /// Generate a signed HMAC token for a user
    Gen { user_id: String },
}

/// Vault management subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum VaultCommand {
    /// Store a secret in the hardware vault
    Set { key: String, value: String },
    /// Retrieve a secret from the hardware vault
    Get { key: String },
    /// Delete a secret from the hardware vault
    Delete { key: String },
}

/// Mesh network management subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum MeshCommand {
    /// Show this node's identity (NodeID + public key)
    Id,
    /// Add a trusted peer node
    AddPeer {
        node_id: String,
        endpoint: String,
        #[arg(long)]
        alias: Option<String>,
        #[arg(long)]
        cloud: bool,
    },
    /// List all known peers
    List,
    /// Remove a peer
    RemovePeer { node_id: String },
    /// Ping a peer (handshake test)
    Ping { node_id: String },
    /// Sync memories with a specific peer
    Sync {
        node_id: String,
        #[arg(long, default_value = "bidirectional")]
        mode: String,
    },
    /// Generate a temporary pairing code
    PairingCode {
        #[arg(long)]
        endpoint: Option<String>,
    },
    /// Join a mesh using a pairing code
    Join { code: String },
    /// Show mesh network status
    Status,
}

/// Secrets management subcommands
#[derive(Subcommand, Debug, Clone)]
pub enum SessionCommand {
    /// Export a session to a JSON bundle
    Export {
        /// ID of the session to export
        session_id: String,
        /// Output file path
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Import a session from a JSON bundle
    Import {
        /// Path to the session bundle file
        input: PathBuf,
    },
    /// Share a session with a peer via mesh
    Share {
        /// ID of the session to share
        session_id: String,
        /// Node ID of the peer to share with
        #[arg(short, long)]
        peer: String,
    },
}

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

#[derive(Subcommand, Debug, Clone)]
pub enum DataCommonsCommand {
    /// Export anonymized telemetry to a training bundle
    ExportTrainingBundle {
        /// Output file path
        #[arg(short, long)]
        output: PathBuf,
        /// Seed for deterministic split and anonymization
        #[arg(short, long, default_value_t = 42)]
        seed: u64,
        /// Ratio for eval split (0.0 to 1.0)
        #[arg(short, long, default_value_t = 0.2)]
        eval_ratio: f32,
    },
    /// Validate a training bundle for fine-tuning readiness
    Validate {
        /// Path to the bundle directory
        bundle_path: PathBuf,
    },
}
