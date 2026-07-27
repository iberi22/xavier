# Xavier CLI Reference

Version: `0.10.0-12-06-2026`

```bash
xavier [COMMAND]
```

The CLI talks to the local HTTP server for most remote operations and falls back to local storage for selected memory commands.

## Global Configuration

The current `Cli` parser exposes subcommands directly. The following global flags are planned/documented operational conventions and may be wired by wrappers or future parser updates:

| Flag | Description |
|---|---|
| `--config <path>` | Use an explicit Xavier config file. Equivalent environment pattern: `XAVIER_CONFIG_PATH=<path>`. |
| `--verbose` | Enable verbose logs. Equivalent environment pattern: `RUST_LOG=debug` or `XAVIER_LOG_LEVEL=debug`. |
| `--json` | Prefer JSON output where command handlers support a format flag or already return JSON. |

Important environment variables:

| Variable | Description |
|---|---|
| `XAVIER_TOKEN` | Token sent to HTTP endpoints as `X-Xavier-Token`. |
| `XAVIER_BASE_URL` | Base URL used by HTTP-backed CLI commands, usually `http://localhost:8006`. |
| `XAVIER_PORT` | HTTP server port. |
| `XAVIER_WORKSPACE_DIR` | Workspace data directory. |

## Core Commands

### `xavier http [port]`

Start the Xavier HTTP server.

```bash
xavier http
xavier http 8006
```

### `xavier mcp`

Start the stdio MCP server.

```bash
xavier mcp
```

### `xavier add <content> [title]`

Add a memory.

Flags:

| Flag | Description |
|---|---|
| `-k, --kind <kind>` | Memory type, such as `episodic`, `semantic`, `procedural`, `fact`, or `decision`. |
| `--cluster <id>` | Cluster ID. |
| `--level <level>` | Memory level. |
| `--relation <relation>` | Relation name. |

```bash
xavier add "Use signed manifests for mesh sync" "mesh decision" --kind decision --cluster mesh --level semantic
```

### `xavier search <query> [limit]`

Search memories.

Flags:

| Flag | Description |
|---|---|
| `-n, --max-results <n>` | Preferred result count. Overrides positional `limit`. |
| `--cluster <id>` | Filter by cluster. Can be repeated. |
| `--level <level>` | Filter by level. Can be repeated. |

```bash
xavier search "mesh sync" --max-results 5 --cluster mesh
```

### `xavier recall <query>`

Recall memories with score-oriented display.

Flags:

| Flag | Default | Description |
|---|---|---|
| `-l, --limit <n>` | `10` | Result count. |

```bash
xavier recall "Data Commons encryption" --limit 8
```

### `xavier stats`

Show Xavier statistics.

```bash
xavier stats
```

### `xavier export`

Export memories to JSON.

Flags:

| Flag | Description |
|---|---|
| `--public` | Export only public memories. |
| `-o, --output <path>` | Write output to a file. |
| `-l, --limit <n>` | Limit exported memories. |

```bash
xavier export --public --output public-memories.json --limit 1000
```

### `xavier export-pack`

Export a structured context pack.

Flags:

| Flag | Default | Description |
|---|---|---|
| `-t, --topic <topic>` | required | Topic to retrieve. |
| `-m, --max-level <n>` | `3` | Max context level. |
| `-o, --out <path>` | required | Output `.xcp` file. |

```bash
xavier export-pack --topic "mesh roadmap" --max-level 3 --out mesh-roadmap.xcp
```

### `xavier session-save <session_id> <content>`

Save session context to Xavier.

```bash
xavier session-save session-001 "Summary of current work"
```

## Billing

### `xavier billing`

Show API usage and account balance through `/v1/account/usage`.

```bash
xavier billing
```

## Code Graph

### `xavier code scan <path>`

Scan and index a codebase path.

On first scan of a workspace, Xavier may ask to install the optional Colby
CodeGraph sidecar (consent-first). Decline or failure → Xavier native graph
continues. Consent is stored in `.xavier/codegraph-sidecar.json`.

Flags:

| Flag | Default | Description |
|---|---|---|
| `--reprompt-codegraph` | off | Ask again even if previously declined/skipped. |

Env:

| Variable | Values | Description |
|---|---|---|
| `XAVIER_CODEGRAPH_INSTALL` | `ask` · `yes` · `no` · `auto` | Consent policy (default `ask`). |
| `XAVIER_CODEGRAPH_REPROMPT` | `1` | Same as `--reprompt-codegraph`. |
| `XAVIER_CODE_GRAPH_NATIVE_ONLY` | `1` | Skip Colby entirely. |
| `XAVIER_CODEGRAPH_BIN` | path | Existing Colby launcher. |

```bash
xavier code scan .
XAVIER_CODEGRAPH_INSTALL=yes xavier code scan .   # CI / non-interactive install
xavier code scan . --reprompt-codegraph
```

See `.sidecars/README.md` and `docs/ADR/008-codegraph-sidecar-consent.md`.

### `xavier code sync --git`

Incremental CodeGraph update from git deltas (no full tree walk):

```
git diff → affected paths → AST reparse → symbols/edges patch → dump JSON
```

Flags:

| Flag | Default | Description |
|---|---|---|
| `--git` | required | Enable git-driven sync. |
| `--base <commit>` | checkpoint / `HEAD~1` | Diff base commit-ish. |
| `--staged` | off | Diff staged changes (`git diff --cached`). |

Checkpoint: `.xavier/codegraph-sync-commit` (updated to `HEAD` after sync).
If the CodeGraph DB is empty, performs one full scan of the repo root first.

Runs **locally** against `data/code_graph.db` (no HTTP server required). Soft-dumps `.xavier/codegraph.json`.

```bash
xavier code sync --git
xavier code sync --git --base HEAD~5
xavier code sync --git --staged
```

Optional post-commit hook (not installed by default):
`scripts/hooks/post-commit-codegraph.sh` — see `docs/guides/CODEGRAPH_GIT_SYNC.md`.

### `xavier code find <query>`

Find symbols by name.

Flags:

| Flag | Default | Description |
|---|---|---|
| `-l, --limit <n>` | `10` | Max results. |
| `-k, --kind <kind>` | none | Symbol kind filter. |

```bash
xavier code find "MemoryManager" --kind struct --limit 10
```

### `xavier code dependencies <query>`

Find outgoing dependencies.

Flags:

| Flag | Default | Description |
|---|---|---|
| `-d, --depth <n>` | `3` | Traversal depth. |
| `-l, --limit <n>` | `50` | Max results. |
| `-e, --edge-type <type>` | none | Edge type filter. |

```bash
xavier code dependencies "v1_memories_add" --depth 2
```

### `xavier code reverse-dependencies <query>`

Find incoming dependencies.

```bash
xavier code reverse-dependencies "MemoryRecord" --depth 3 --limit 50
```

### `xavier code call-chain <query>`

Trace a basic call chain.

```bash
xavier code call-chain "search_handler" --depth 3
```

### `xavier code hubs`

Show highly connected symbols.

### `xavier code hotspots`

Show complexity hotspots.

### `xavier code stats`

Show code graph stats.

## Data Commons

### `xavier data-commons export-training-bundle`

Export anonymized telemetry to a training bundle.

Flags:

| Flag | Default | Description |
|---|---|---|
| `-o, --output <path>` | required | Output directory. |
| `-s, --seed <n>` | `42` | Deterministic split/anonymization seed. |
| `-e, --eval-ratio <ratio>` | `0.2` | Eval split ratio from `0.0` to `1.0`. |

```bash
xavier data-commons export-training-bundle --output ./bundle --seed 42 --eval-ratio 0.2
```

### `xavier data-commons validate <bundle_path>`

Validate a training bundle for fine-tuning readiness.

```bash
xavier data-commons validate ./bundle
```

## Mesh

### `xavier mesh id`

Show this node's identity.

### `xavier mesh add-peer <node_id> <endpoint>`

Add a trusted peer.

Flags:

| Flag | Description |
|---|---|
| `--alias <name>` | Friendly peer alias. |
| `--cloud` | Mark peer as cloud-backed. |

```bash
xavier mesh add-peer node_abc http://peer:8006 --alias lab-node
```

### `xavier mesh list`

List known peers.

### `xavier mesh remove-peer <node_id>`

Remove a peer.

### `xavier mesh ping <node_id>`

Run a handshake test with a peer.

### `xavier mesh sync <node_id>`

Sync memories with a peer.

Flags:

| Flag | Default | Description |
|---|---|---|
| `--mode <mode>` | `bidirectional` | `pull`, `push`, or `bidirectional`. |

```bash
xavier mesh sync node_abc --mode bidirectional
```

### `xavier mesh pairing-code`

Generate a temporary pairing code.

Flags:

| Flag | Description |
|---|---|
| `--endpoint <url>` | Public endpoint to embed in the pairing code. |

```bash
xavier mesh pairing-code --endpoint http://localhost:8006
```

### `xavier mesh join <code>`

Join a mesh using a pairing code.

```bash
xavier mesh join "<PAIRING_CODE>"
```

### `xavier mesh status`

Show mesh status.

## Navigation

Top-level aliases:

```bash
xavier ls [path]
xavier cd <path>
xavier pwd
```

Grouped commands:

```bash
xavier nav ls [path]
xavier nav cd <path>
xavier nav pwd
```

### `xavier nav affected <path>`

Show nodes affected by a change.

Flags:

| Flag | Default | Description |
|---|---|---|
| `-d, --depth <n>` | `2` | BFS traversal depth. |
| `-f, --format <format>` | `table` | `table` or `json`. |
| `--exclude-file-type <type>` | none | Filter, for example `code`. |

```bash
xavier nav affected docs/API.md --depth 2 --format json
```

### `xavier nav visualize`

Render the memory graph.

Flags:

| Flag | Default | Description |
|---|---|---|
| `-f, --format <format>` | `text` | `text` or `json`. |

## Provider

### `xavier provider status`

Show current provider status.

### `xavier provider list`

List providers and strategies.

### `xavier provider set <name>`

Manually switch provider.

```bash
xavier provider set openai
```

### `xavier provider auto <strategy>`

Set automatic provider selection strategy.

```bash
xavier provider auto balanced
```

### `xavier provider fallback <providers...>`

Declare a fallback chain. The current handler reports intent; full HTTP fallback persistence is not yet implemented.

```bash
xavier provider fallback local groq openai
```

## Secrets and Vault

### `xavier secrets lend <secret_name> <agent>`

Lend a secret to an agent.

Flags:

| Flag | Default | Description |
|---|---|---|
| `-t, --ttl <seconds>` | `3600` | Lease lifetime. |

```bash
xavier secrets lend OPENAI_API_KEY agent-1 --ttl 900
```

### `xavier secrets list-leases`

List active leases.

### `xavier secrets revoke <token>`

Revoke a lease.

### `xavier secrets status <token>`

Check lease status.

### `xavier vault set|get|delete`

Manage secrets in the hardware vault.

```bash
xavier vault set OPENAI_API_KEY sk-...
xavier vault get OPENAI_API_KEY
xavier vault delete OPENAI_API_KEY
```

## Session

### `xavier session export <session_id>`

Export a session bundle.

Flags:

| Flag | Description |
|---|---|
| `-o, --output <path>` | Write bundle to file. |

```bash
xavier session export session-001 --output session-001.json
```

### `xavier session import <input>`

Import a session bundle.

```bash
xavier session import session-001.json
```

### `xavier session share <session_id>`

Share a session with a mesh peer.

Flags:

| Flag | Description |
|---|---|
| `-p, --peer <node_id>` | Target peer node ID. |

```bash
xavier session share session-001 --peer node_abc
```

## Spawn and Swarm

### `xavier spawn`

Spawn one or more agents.

Flags:

| Flag | Default | Description |
|---|---|---|
| `--count <n>` | `1` | Agent count. |
| `-p, --provider <name>` | `local` | Provider. Can be repeated. |
| `-m, --model <name>` | default | Model. Can be repeated. |
| `-s, --skill <name>` | none | Skill to load. Can be repeated. |
| `-x, --context <k=v>` | none | Custom context. Can be repeated. |
| `-t, --task <task>` | none | Task to execute. |

```bash
xavier spawn --count 3 --provider local --skill research --task "summarize mesh roadmap"
```

### `xavier multi-spawn`

Batch spawn agents.

Flags:

| Flag | Default | Description |
|---|---|---|
| `--agents <n>` | `10` | Total agents. |
| `--batch <n>` | `4` | Batch size. |
| `-p, --provider <name>` | `local` | Provider. Can be repeated. |
| `-m, --model <name>` | default | Model. Can be repeated. |
| `-s, --skills <name>` | none | Skill. Can be repeated. |
| `-t, --task <task>` | none | Task to execute. |

### `xavier swarm`

Launch agents from a JSON config.

Flags:

| Flag | Default | Description |
|---|---|---|
| `-c, --config <path>` | required | Swarm JSON config. |
| `-p, --parallel <n>` | `4` | Max parallel agents. |

```bash
xavier swarm --config swarm.json --parallel 4
```

## Tasks

### `xavier tasks list`

List tasks.

Flags:

| Flag | Description |
|---|---|
| `-p, --project <project>` | Project filter. |
| `-s, --status <status>` | Status filter. |
| `--search <query>` | Search filter. |

```bash
xavier tasks list --project xavier --status open
```

### `xavier tasks sync`

Synchronize tasks with configured backends.

## Token

### `xavier token new`

Generate a random token for `XAVIER_TOKEN`.

### `xavier token gen <user_id>`

Generate a signed HMAC token for a user. Requires `XAVIER_TOKEN_SECRET`.

```bash
xavier token gen belal
```

## Usage

### `xavier usage status`

Show usage status for known providers.

### `xavier usage update <provider> <percentage>`

Manually update provider usage percentage.

```bash
xavier usage update openai 73.5
```

### `xavier usage cooldown <provider> <minutes>`

Set provider cooldown.

```bash
xavier usage cooldown openai 30
```

## Verify

### `xavier verify scan`

Run system verification.

Flags:

| Flag | Default | Description |
|---|---|---|
| `-f, --format <format>` | `table` | `table`, `json`, or `markdown`. |
| `-d, --detailed` | false | Include detailed masked API key status. |

```bash
xavier verify scan --format markdown --detailed
```

## Other Commands

| Command | Description |
|---|---|
| `xavier setup` | Run interactive system detection and setup. |
| `xavier quota` | Show provider quotas and limits. |
| `xavier chronicle <subcommand>` | Manage Chronicle docs/devlog workflows. |
