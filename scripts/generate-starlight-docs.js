const fs = require('fs');
const path = require('path');

const FEATURES_JSON_PATH = path.join(__dirname, '../.gitcore/features.json');
const FEATURES_DIR = path.join(__dirname, '../.gitcore/features');
const TARGET_DIR = path.join(__dirname, '../docs/site/src/content/docs/features');

// Feature ID to FEATURE-*.md file mapping
const featureMapping = {
  'feat-unified-storage': 'FEATURE-feat-unified-storage.md',
  'feat-hybrid-search': 'FEATURE-feat-hybrid-search.md',
  'feat-belief-graph': 'FEATURE-feat-belief-graph.md',
  'feat-mcp-server': 'FEATURE-feat-mcp-server.md',
  'feat-code-graph-index': 'FEATURE-feat-code-graph-index.md',
  'feat-session-management': 'FEATURE-feat-session-management.md',
  'feat-encryption-at-rest': 'FEATURE-api-key-proxy-vault.md',
  'feat-documentation-site': 'FEATURE-feat-documentation-site.md',
  'feat-src-reference': 'FEATURE-feat-src-reference.md',
  'feat-mesh-network': 'FEATURE-feat-mesh-network.md',
  'feat-telegram-bot': 'FEATURE-feat-telegram-bot.md',
  'feat-notification-system': 'FEATURE-feat-notification-system.md',
  'feat-hormer-navigation': 'FEATURE-feat-hormer-navigation.md',
  'feat-governance-dao': 'FEATURE-feat-governance-dao.md',
  'feat-runtime-health': 'FEATURE-feat-runtime-health.md',
  'feat-auto-improvement': 'FEATURE-feat-auto-improvement.md',
  'feat-dual-license': 'FEATURE-feat-dual-license.md',
  'feat-context-regeneration': 'FEATURE-feat-context-regeneration.md',
  'feat-openclaw-scanner': 'FEATURE-feat-openclaw-scanner.md',
  'feat-agent-cli-commands': 'FEATURE-feat-agent-cli-commands.md',
  'feat-local-first': 'FEATURE-llm-provider-dashboard-v2.md',
  'feat-token-savings': 'FEATURE-feat-token-savings.md',
  'feat-plugin-system': 'FEATURE-feat-plugin-system.md',
  'feat-security-hygiene': 'FEATURE-feat-security-hygiene.md',
  'feat-graph-explorer': 'FEATURE-feat-graph-explorer.md'
};

const functionalExamples = {
  'feat-unified-storage': `
### Functional SQLite Storage Example
Ensure SQLite and \`sqlite-vec\` are correctly initialized via the configuration file \`config/xavier.config.json\` and query database connections programmatically or verify with standard CLI tools:

\`\`\`bash
# Check if the unified database is active and inspect schema tables
sqlite3 ~/.xavier/xavier_data/db/default.sqlite \\
  "SELECT name FROM sqlite_master WHERE type='table';"
\`\`\`

Alternatively, initialize a database connection pool inside Rust using \`xavier-core\`:
\`\`\`rust
let pool = xavier::storage::init_pool("~/.xavier/xavier_data/db/default.sqlite")
    .expect("Failed to initialize database pool");
\`\`\`
`,

  'feat-hybrid-search': `
### Functional Hybrid Search Example
Query the hybrid search engine using BM25 and vector integration:

\`\`\`bash
curl -X POST "http://localhost:8006/memory/search" \\
  -H "X-Xavier-Token: $XAVIER_TOKEN" \\
  -H "Content-Type: application/json" \\
  -d '{
    "query": "vector search optimization",
    "limit": 5,
    "include_content": true
  }'
\`\`\`

Query the offline system directly through the CLI tool:
\`\`\`bash
xavier memory search "reputation systems" --limit 3
\`\`\`
`,

  'feat-belief-graph': `
### Functional Belief Graph Example
View concept relationships or serialize relationships to JSON format:

\`\`\`bash
curl -H "X-Xavier-Token: $XAVIER_TOKEN" \\
  "http://localhost:8006/memory/graph/view"
\`\`\`

Response JSON structure of belief nodes and edges:
\`\`\`json
{
  "nodes": [
    {"id": "node_1", "label": "Xavier", "kind": "System"},
    {"id": "node_2", "label": "Sovereign Mesh", "kind": "Concept"}
  ],
  "edges": [
    {"source": "node_1", "target": "node_2", "relation": "IMPLEMENTS"}
  ]
}
\`\`\`
`,

  'feat-mcp-server': `
### Functional MCP Integration Example
Add Xavier as an MCP server inside your Claude Desktop configuration file:

**Configuration (\`claude_desktop_config.json\`):**
\`\`\`json
{
  "mcpServers": {
    "xavier": {
      "command": "xavier",
      "args": ["mcp"],
      "env": {
        "XAVIER_TOKEN": "your-secret-token"
      }
    }
  }
}
\`\`\`

Verify available MCP tools using the standard MCP protocol inspector or client handshakes:
\`\`\`bash
npx @modelcontextprotocol/inspector xavier mcp
\`\`\`
`,

  'feat-code-graph-index': `
### Functional Code Indexing Example
Trigger an AST-backed code-graph scan using the CLI:

\`\`\`bash
# Scan and index the current repository code
xavier code scan --path .

# Find code symbols inside the repository
xavier code find "verify_totp"
\`\`\`

Query stats via HTTP:
\`\`\`bash
curl -H "X-Xavier-Token: $XAVIER_TOKEN" \\
  "http://localhost:8006/code/stats"
\`\`\`
`,

  'feat-session-management': `
### Functional Session Management Example
Create a persistent chat session and bundle session state for transfer:

\`\`\`bash
# Create a new session
curl -X POST "http://localhost:8006/v1/sessions" \\
  -H "X-Xavier-Token: $XAVIER_TOKEN" \\
  -H "Content-Type: application/json" \\
  -d '{
    "title": "Agent Session 1"
  }'

# Export the entire session bundle
curl -H "X-Xavier-Token: $XAVIER_TOKEN" \\
  "http://localhost:8006/v1/sessions/export?id=session_abc123" \\
  -o session_bundle.json
\`\`\`
`,

  'feat-encryption-at-rest': `
### Functional Encryption Example
Enable AES-256-GCM encryption in your \`.env\` configuration:

\`\`\`sh
# Enable secure storage encryption at rest
XAVIER_ENCRYPTION_KEY="argon2-derived-base64-or-raw-32byte-hex-string"
XAVIER_JWT_SECRET="secure-jwt-signing-secret"
\`\`\`

When active, database entries are transparently encrypted prior to persistence, ensuring data safety even in untrusted runtime environments.
`,

  'feat-documentation-site': `
### Functional Starlight Build Example
Build and run the Astro Starlight site locally:

\`\`\`bash
# Navigate to docs/site and run dev server
cd docs/site
pnpm install
pnpm run dev --port 3000

# Compile for production deployment
pnpm run build
\`\`\`
`,

  'feat-src-reference': `
### Functional SRC Verification Example
Review active directory layouts and configurations defined in \`.gitcore/SRC_CONFIG.md\`:

\`\`\`bash
# Verify the directories are conformant to standard layouts
cat .gitcore/SRC.md | grep -A 5 "Directory Structure"
\`\`\`
`,

  'feat-mesh-network': `
### Functional Mesh P2P Example
Share a local workspace database over the P2P Mesh Network:

\`\`\`bash
# Share workspace via HTTP endpoint
curl -X POST "http://localhost:8006/v1/mesh/workspaces/share" \\
  -H "X-Xavier-Token: $XAVIER_TOKEN" \\
  -H "Content-Type: application/json" \\
  -d '{
    "workspace_id": "workspace-123",
    "namespaces": ["core::auth", "cognitive::beliefs"]
  }'
\`\`\`

Verify P2P Node Identity:
\`\`\`bash
xavier mesh status
\`\`\`
`,

  'feat-telegram-bot': `
### Functional Telegram Command Example
Interact with the Xavier bot via direct message commands in Telegram:

\`\`\`txt
# Search system memory
/memory search "reputation score calculation"

# Check offline model engine status
/localstatus

# Retrieve overall system status
/health
\`\`\`

Activate the bot by supplying your token in \`.env\`:
\`\`\`sh
TELEGRAM_BOT_TOKEN="123456789:ABCdefGhIJKlmNoPQRsTUVwxyZ"
\`\`\`
`,

  'feat-notification-system': `
### Functional Notification API Example
Query system and agent notifications from the persistent event bus:

\`\`\`bash
# List all active system notifications
curl -H "X-Xavier-Token: $XAVIER_TOKEN" \\
  "http://localhost:8006/panel/api/notifications?category=System"
\`\`\`

Response format:
\`\`\`json
[
  {
    "id": "notif_01J2F9Y8",
    "title": "Low GPU VRAM",
    "message": "Available VRAM fell below 1GB during consolidation.",
    "category": "System",
    "timestamp": 1721516400
  }
]
\`\`\`
`,

  'feat-hormer-navigation': `
### Functional HORMER Traversal Example
Run hierarchical memory navigation for high-precision retrieval:

\`\`\`bash
# Perform intelligent directory traversal using HORMER agent
xavier navigation explore "Find all security policy notes"
\`\`\`

HTTP Request:
\`\`\`bash
curl -X POST "http://localhost:8006/v1/navigation/explore" \\
  -H "X-Xavier-Token: $XAVIER_TOKEN" \\
  -H "Content-Type: application/json" \\
  -d '{
    "prompt": "Find all security policy notes",
    "max_depth": 3
  }'
\`\`\`
`,

  'feat-governance-dao': `
### Functional DAO Proposal Example
Submit a new Xavier Improvement Proposal (XIP) to the bicameral DAO:

\`\`\`bash
curl -X POST "http://localhost:8006/v1/mesh/governance/proposals" \\
  -H "X-Xavier-Token: $XAVIER_TOKEN" \\
  -H "Content-Type: application/json" \\
  -d '{
    "title": "XIP-12: Standardize Context Bridges",
    "description": "Formally specify the schema for multi-node database bridges.",
    "creator_node_id": "peer-node-998"
  }'
\`\`\`
`,

  'feat-runtime-health': `
### Functional Health Check Example
Retrieve the JSON health diagnostic response from a running node:

\`\`\`bash
curl http://localhost:8006/health
\`\`\`

Sample output:
\`\`\`json
{
  "status": "healthy",
  "database_integrity": "OK",
  "disk_space_bytes": 45189230104,
  "vram_available_bytes": 8542912512,
  "offline_engine_status": "running",
  "offline_engine_port": 11434
}
\`\`\`
`,

  'feat-auto-improvement': `
### Functional Auto-Improvement Example
Execute an optimization run to measure benchmark recall gaps and tune parameters:

\`\`\`bash
# Trigger an auto-improvement experiment loop
xavier improve run --benchmark "rag_recall_test"
\`\`\`

Retrieve historical experiment status:
\`\`\`bash
xavier improve status
\`\`\`
`,

  'feat-dual-license': `
### Functional License Activation Example
Query your current licensing status or accept the Mesh License:

\`\`\`bash
# Check licensing
xavier license status

# Accept the Mesh Network License to unlock P2P capability
xavier license accept --mesh
\`\`\`
`,

  'feat-context-regeneration': `
### Functional Context Regeneration Example
Regenerate agent context using Turn Packs to maintain peak semantic recall:

\`\`\`bash
curl -X POST "http://localhost:8006/v1/context/regenerate" \\
  -H "X-Xavier-Token: $XAVIER_TOKEN" \\
  -H "Content-Type: application/json" \\
  -d '{
    "session_id": "sess_91823",
    "compression_mode": "extractive",
    "target_token_budget": 2048
  }'
\`\`\`
`,

  'feat-openclaw-scanner': `
### Functional OpenClaw Scan Example
Manually trigger an asynchronous scan of your OpenClaw agent workspaces:

\`\`\`bash
# Scan directories for MEMORY.md and SOUL.md files
xavier agent scan

# Force indexing of a specific agent
xavier agent index --agent "claude"
\`\`\`
`,

  'feat-agent-cli-commands': `
### Functional Agent CLI Commands Example
Execute full CLI synchronization with remote Supabase or central servers:

\`\`\`bash
# Perform full scan, index, and push sequence
xavier agent sync --full

# Output synchronization logs as JSON for integration
xavier agent sync --json
\`\`\`
`,

  'feat-local-first': `
### Functional Local Configuration Example
Configure Xavier to function in a fully localized mode using Ollama or other offline engines:

**Environment Setup (\`.env\`):**
\`\`\`sh
XAVIER_LOCAL_LLM_URL="http://127.0.0.1:11434"
XAVIER_EMBEDDING_MODEL="nomic-embed-text"
XAVIER_CHAT_MODEL="gemma-4-E2B-it-uncensored"
\`\`\`

Verify that the local engine status has been detected as "running":
\`\`\`bash
curl http://localhost:8006/v1/offline-models/status
\`\`\`
`,

  'feat-token-savings': `
### Functional Token Savings Example
Implement MemGPT-style progressive retrieval to cut context-window overhead:

1. **Fat Search (Retrieve metadata and snippets only):**
\`\`\`bash
curl -X POST "http://localhost:8006/memory/search" \\
  -H "X-Xavier-Token: $XAVIER_TOKEN" \\
  -H "Content-Type: application/json" \\
  -d '{
    "query": "recursive retrieval parameters",
    "include_content": false
  }'
\`\`\`

2. **Page-In (Retrieve selected full contents when required):**
\`\`\`bash
curl -X POST "http://localhost:8006/memory/context" \\
  -H "X-Xavier-Token: $XAVIER_TOKEN" \\
  -H "Content-Type: application/json" \\
  -d '{
    "ids": ["doc_908", "doc_912"],
    "max_chars": 5000
  }'
\`\`\`
`,

  'feat-plugin-system': `
### Functional Plugin Example
Configure your \`plugins.json\` manifest to register a local AST parser plugin:

**Manifest (\`plugins.json\`):**
\`\`\`json
{
  "plugins": [
    {
      "id": "parser-python-local",
      "name": "Local Python AST Parser",
      "version": "1.0.0",
      "entry_point": "file://plugins/parser-python/main.py",
      "languages": ["python"]
    }
  ]
}
\`\`\`

Point the server to your registry using the environment variable:
\`\`\`sh
XAVIER_PLUGIN_REGISTRY_URL="file://config/plugins.json"
\`\`\`
`,

  'feat-security-hygiene': `
### Functional Security Audit Example
Audit dependencies for security and license compliance:

\`\`\`bash
# Run a dependency security audit via pnpm
cd panel-ui && pnpm audit

# Audit Cargo dependencies
cargo audit
\`\`\`
`,

  'feat-graph-explorer': `
### Functional Graph API Example
Fetch multi-layer force graph data corresponding to the Code Graph representation:

\`\`\`bash
curl -H "X-Xavier-Token: $XAVIER_TOKEN" \\
  "http://localhost:8006/code/graph/view?focus=symbols"
\`\`\`

Response payload format:
\`\`\`json
{
  "nodes": [
    {"id": "sym_register", "name": "register_handler", "type": "function"},
    {"id": "sym_auth_db", "name": "AuthDb", "type": "struct"}
  ],
  "links": [
    {"source": "sym_register", "target": "sym_auth_db", "kind": "uses"}
  ]
}
\`\`\`
`
};

function main() {
  if (!fs.existsSync(TARGET_DIR)) {
    fs.mkdirSync(TARGET_DIR, { recursive: true });
  }

  const featuresData = JSON.parse(fs.readFileSync(FEATURES_JSON_PATH, 'utf8'));
  const features = featuresData.features;

  console.log(`Generating documentation for ${features.length} features...`);

  features.forEach(f => {
    const fileId = f.id;
    const sourceFileName = featureMapping[fileId];
    if (!sourceFileName) {
      console.error(`No source file mapped for feature: ${fileId}`);
      return;
    }

    const sourcePath = path.join(FEATURES_DIR, sourceFileName);
    if (!fs.existsSync(sourcePath)) {
      console.error(`Source file does not exist: ${sourcePath}`);
      return;
    }

    let content = fs.readFileSync(sourcePath, 'utf8');

    // Parse the first header or find the title
    let title = f.name;
    const titleMatch = content.match(/^# FEATURE:\s*(.+)$/m);
    if (titleMatch) {
      title = titleMatch[1].trim();
      // Remove the original first header line to avoid double headers in Starlight
      content = content.replace(/^# FEATURE:\s*(.+)$/m, '').trim();
    } else {
      const h1Match = content.match(/^#\s*(.+)$/m);
      if (h1Match) {
        title = h1Match[1].trim();
        content = content.replace(/^#\s*(.+)$/m, '').trim();
      }
    }

    // Clean up code block language warnings inside source files
    content = content.replace(/```env\b/g, '```sh');
    content = content.replace(/```telegram\b/g, '```txt');

    const description = f.description || `Documentation for the ${title} feature in Xavier.`;

    // Construct Frontmatter
    const frontmatter = `---
title: "${title}"
description: "${description.replace(/"/g, '\\"')}"
---

`;

    // Append the functional usage example
    const example = functionalExamples[fileId] || `
### Usage Example
This feature integrates directly into Xavier's operational runtime. Configure it inside \`config/xavier.config.json\` or verify its operations via CLI.
`;

    // Combine frontmatter + content + example
    const starlightContent = frontmatter + content + '\n' + example;

    // Write to destination
    const targetPath = path.join(TARGET_DIR, `${fileId}.md`);
    fs.writeFileSync(targetPath, starlightContent, 'utf8');
    console.log(`Successfully wrote: ${targetPath}`);
  });

  console.log('Finished generating feature documentation pages.');
}

main();
