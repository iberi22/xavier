# Xavier CLI Reference

Version: `0.2.15 (2026-09-27)`

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

### `xavier http [PORT]`

Start the Xavier HTTP server. `PORT` is **positional** (not a flag) and falls back to settings/`XAVIER_PORT`, then `8006`. The `serve` alias is equivalent.

Flags:

| Flag | Default | Description |
|---|---|---|
| `--mcp-port <port>` | `8100` | MCP HTTP+SSE port. `0` disables it. |
| `--no-ui` | off | Serve only the REST/MCP API, without the admin web panel. |
| `--host <addr>` | `XAVIER_HOST` | Host binding address, e.g. `0.0.0.0` or `127.0.0.1`. Overrides `XAVIER_HOST` for this process. |
| `--http` | — | Compatibility flag for HTTP server start. |

```bash
xavier http
xavier http 8006
xavier http --mcp-port 8100 --host 127.0.0.1
```

### `xavier mcp`

Start the stdio MCP server.

```bash
xavier mcp
```

### `xavier health`

Print the node's system health: status, version, and — whenever the status is not `healthy` — the degraded reasons.

Flags:

| Flag | Description |
|---|---|
| `--cloud` | Report cloud backend health instead of the node (`/health/cloud`). |

The report always shows the version reported by the server payload; when the payload carries no `version`, the CLI fills in its own and says so inline, because a version string from the other process is not the same fact as one from this one. When the status is not `healthy`, each entry of `degraded_reasons` is printed, prefixed by its class: `host:<check>` for resource pressure on the machine, `subsystem:<check>` for a failing Xavier component.

The command exits non-zero when the node is unhealthy, so it can gate a script or a CI job. An HTTP error, or a missing `XAVIER_TOKEN`, also aborts non-zero.

```bash
xavier health
xavier health --cloud
```

### `xavier doctor`

Diagnose the local-first stack (Ollama, models, database, config, vector store, mesh, HTTP/LLM services, security posture, scheduler).

Flags:

| Flag | Default | Description |
|---|---|---|
| `--format <format>` | `table` | `table`, `json`, or `markdown`. |
| `--verbose` | `false` | Include soft/warning checks in the output. |

Every probe is individually bounded, so no single hanging check can stall the run: each database, embedding, memory, mesh, HTTP, security and scheduler probe gets its own timeout (`5s` per check), the system scan is separately bounded (`5s`), and the whole diagnostic is wrapped in an overall safety-net deadline (`25s`). Blocking probes run on `spawn_blocking` so the timeout can actually fire.

A timed-out probe is not silence: it is recorded as a check with `timed_out` set and status `Warn`, so a hung check surfaces as a warning rather than vanishing. Exceeding the overall deadline yields a single `Fail` check instead. The report is therefore **always** printed, even when the total deadline is exceeded. Exit code is `1` only when some check is `Fail`; `Ok` and `Warn` both exit `0`.

```bash
xavier doctor
xavier doctor --format json
xavier doctor --verbose
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

### `xavier code install`

Install the direct CodeGraph binary / sidecar. Runs locally, no HTTP server required.

Flags:

| Flag | Default | Description |
|---|---|---|
| `--from-source` | off | Build/install from the local source repository instead of the pre-built release binary. |
| `--force` | off | Force re-installation even when already present and up to date. |
| `--json` | off | Emit progress and status as JSON (`status`, `installed_path`, `version`, `verified`, `message`). |

```bash
xavier code install
xavier code install --from-source --json
```

### `xavier code scan <path>`

Scan and index a codebase path.

> **Note:** The native AST-backed tree-sitter parser/indexer is the canonical code graph engine. Although previous designs referenced an external "Colby" sidecar, this sidecar is currently **mock-disabled / stubbed out** and is not active in this release (the runtime sidecar loader is a stub). All Colby configuration parameters and prompts are stubbed out to bypass the installer and always use the native indexer directly.

On first scan of a workspace, Xavier does not ask to install the optional Colby CodeGraph sidecar since it is currently disabled. It always executes the native tree-sitter graph indexer directly.

Every successful index or scan operation automatically triggers an asynchronous, soft-fail dump of the full code graph to `.xavier/codegraph.json` (unless the graph contains more than 25,000 symbols). This portable JSON dump is used for lightweight offline parsing, while downstream assessors (such as Layer 1 Maturity Scanning) and document harvesters prioritize direct, fast querying against the live SQLite index (`.xavier/code_graph.db`).

Flags:

| Flag | Default | Description |
|---|---|---|
| `--reprompt-codegraph` | off | *(Stub/Ignored)* Ask again even if previously declined/skipped. |

Env:

| Variable | Values | Description |
|---|---|---|
| `XAVIER_CODEGRAPH_INSTALL` | `ask` · `yes` · `no` · `auto` | *(Stub/Ignored)* Consent policy. |
| `XAVIER_CODEGRAPH_REPROMPT` | `1` | *(Stub/Ignored)* Same as `--reprompt-codegraph`. |
| `XAVIER_CODE_GRAPH_NATIVE_ONLY` | `1` | *(Stub/Always Active)* Always runs native-only code-graph parsing. |
| `XAVIER_CODEGRAPH_BIN` | path | *(Stub/Ignored)* Path to Colby launcher. |

```bash
xavier code scan .
```

See `docs/adr/007-codegraph-native-vs-colby.md`.

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
| `--memory` | `false` | Upsert short symbol summaries into Xavier memory (`code/{repo}/{stable_id}`). |

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

| Flag | Default | Description |
|---|---|---|
| `-d, --depth <n>` | `3` | Traversal depth. |
| `-l, --limit <n>` | `50` | Max results. |
| `-e, --edge-type <type>` | none | Edge type filter. |

```bash
xavier code reverse-dependencies "MemoryRecord" --depth 3 --limit 50
```

### `xavier code call-chain <query>`

Trace a basic call chain (`Calls` edges only).

| Flag | Default | Description |
|---|---|---|
| `-d, --depth <n>` | `3` | Traversal depth. |
| `-l, --limit <n>` | `50` | Max results. |

```bash
xavier code call-chain "search_handler" --depth 3
```

### `xavier code blast-radius <query>`

Calculate the blast radius of a symbol by BFS over incoming `Calls` edges.

| Flag | Default | Description |
|---|---|---|
| `-d, --depth <n>` | `3` | Maximum caller depth. |

```bash
xavier code blast-radius "handle_core_tool" --depth 3
```

### `xavier code hubs`

Show highly connected symbols.

### `xavier code hotspots`

Show complexity hotspots.

### `xavier code stats`

Show code graph stats.

### `xavier code dump [PATH]`

Dump the portable code graph to `<root>/.xavier/codegraph.json`.

`PATH` is optional and positional. Resolution is explicit path → this process's working directory → the daemon default (`.`), so a bare `xavier code dump` always targets where you are standing, not the daemon's startup workspace. The CLI makes the path absolute before sending it, and the daemon canonicalizes it again against its own cwd; both steps are reported back in the response as `requested_path` (what you typed) and `resolved_path` (what was actually used), on success and on failure alike. The response also carries `path`, the dump file written.

Dumping a target outside the daemon workspace is blocked — targets must reside within the workspace root.

```bash
xavier code dump
xavier code dump /home/user/other-repo
```

### `xavier code load [PATH]`

Load a portable code graph dump back into the server. Resolves `PATH` with the same explicit → cwd → daemon default chain and reports `requested_path` / `resolved_path` in the same way as `dump`.

```bash
xavier code load
xavier code load /home/user/other-repo
```

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

Secret values are never accepted as command-line arguments: they would land in
shell history and agent transcripts. `xavier vault set` and `xavier secrets put`
read the value from a hidden interactive prompt when stdin is a TTY, and from
stdin when it is piped.

### `xavier secrets put <name>`

Store one secret in the hardware vault, reading the value from the hidden
prompt or from stdin.

```bash
xavier secrets put OPENAI_API_KEY
printf '%s' "$VALUE" | xavier secrets put OPENAI_API_KEY
```

### `xavier secrets import-env --from <file> --dry-run|--apply`

Import secrets from a `.env` file into the hardware vault. Exactly one of
`--dry-run` or `--apply` is required.

Flags:

| Flag | Default | Description |
|---|---|---|
| `--from <file>` | — | Path to the `.env` file to import. Required. |
| `--dry-run` | — | Parse and validate the file, print the plan, store nothing. |
| `--apply` | — | Store every parsed entry in the hardware vault. |

```bash
xavier secrets import-env --from .env --dry-run
xavier secrets import-env --from .env --apply
```

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

Manage secrets in the hardware vault. `set` takes no value argument: the value
comes from the hidden prompt, or from stdin when it is piped.

```bash
xavier vault set OPENAI_API_KEY
printf '%s' "$VALUE" | xavier vault set OPENAI_API_KEY
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
| `-q, --search <query>` | Search filter. |

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

### `xavier verify health`

Run the full system health check.

| Flag | Default | Description |
|---|---|---|
| `-f, --format <format>` | `table` | `table`, `json`, or `markdown`. |

### `xavier verify save`

Verify a memory save/retrieve round-trip.

| Flag | Default | Description |
|---|---|---|
| `-c, --content <text>` | `xavier verification test content` | Content to verify with. |

### `xavier verify features`

Scan `.gitcore/features.json` and calculate the real implementation percentage.

| Flag | Default | Description |
|---|---|---|
| `-p, --path <path>` | cwd, walking up | Project root to scan. |
| `-f, --format <format>` | `table` | `table` or `json`. |

## Onboarding

### `xavier init`

Initialize Xavier onboarding and the interactive RAG installer wizard.

| Flag | Default | Description |
|---|---|---|
| `-p, --profile <list>` | none | Data modality profiles to initialize (comma-separated), e.g. `legal,code,video,image,audio`. |
| `--storage-quota-gb <gb>` | none | Maximum storage quota to allocate. |
| `--local-only` | off | Restrict the system to local-only operation, without cloud backends. |
| `-o, --output-path <path>` | none | Output path for the generated configuration file. |
| `--non-interactive` | off | Headless mode, using defaults or the flags above. |

```bash
xavier init --profile code,docs --local-only --non-interactive
```

## Auto-Improvement

### `xavier improve run`

Run the auto-improvement loop: benchmark → gaps → experiments → (optionally) validate. Runs against the locally-loaded memory store.

Flags — these three only:

| Flag | Default | Description |
|---|---|---|
| `--autonomous` | off | Let the engine validate experiments autonomously (re-benchmark and accept beneficial changes). Without it, experiments are proposed but not executed. |
| `--json` | off | Emit the cycle result as JSON, for scripting or scheduler capture. Progress lines are suppressed so stdout stays pure JSON. |
| `--ci` | off | Autonomous and non-interactive; fails when a regression or a critical gap is present. |

The cycle is bounded, not open-ended. Every stage runs under a per-stage deadline, and the whole cycle under an overall deadline:

| Variable | Default | Description |
|---|---|---|
| `XAVIER_IMPROVE_STAGE_TIMEOUT_SECS` | `120` | Per-stage deadline for stages that can perform unbounded work. |
| `XAVIER_IMPROVE_TOTAL_TIMEOUT_SECS` | `300` | Overall deadline for one full cycle. Each stage effectively gets `min(stage, remaining overall)`. |

Out-of-range values are capped rather than rejected.

Exit codes:

| Code | Outcome |
|---|---|
| `0` | Cycle completed. |
| `124` | Cycle truncated by a deadline. The partial report is printed, the stage that did not finish is named, and partial progress is persisted to `.xavier/improvement-history.json`. |
| non-zero (`1`) | Genuine failure, e.g. partial progress could not be persisted. |

A truncated cycle is a timeout, not a success: it is reported and persisted rather than silently swallowed, and it does not exit `0`.

### `xavier improve status`

Show the last improvement cycle and the benchmark history.

```bash
xavier improve run --autonomous --json
xavier improve status
```

## Retrieval Regeneration

### `xavier regen benchmark`

Measure recall@k against a benchmark dataset and print the metrics.

| Flag | Default | Description |
|---|---|---|
| `--dataset <path>` | bundled dataset | Benchmark dataset JSON. |
| `--json` | off | Emit metrics as JSON. |

### `xavier regen tune`

Run the RRF tuner over the benchmark dataset and print the proposal. Proposals are persisted to `.xavier/tuning-history.json` so later runs can detect recall drift against the last baseline.

| Flag | Default | Description |
|---|---|---|
| `--dataset <path>` | bundled dataset | Benchmark dataset JSON. |
| `--json` | off | Emit the tuning proposal as JSON. |

### `xavier regen history`

Print the recent tuning history from `.xavier/tuning-history.json`.

| Flag | Default | Description |
|---|---|---|
| `-l, --limit <n>` | `5` | Number of recent proposals to show, newest last. |
| `--json` | off | Emit the history as JSON. |

```bash
xavier regen benchmark --json
xavier regen tune
xavier regen history --limit 10
```

## Maintenance

### `xavier cleanup`

Clean up empty conversation databases and the legacy store. Purges empty `.xavier/conversations/*.db` files (4KB or smaller) older than `--days`, reports the legacy sqlite store, and removes it under `--apply`.

| Flag | Default | Description |
|---|---|---|
| `--dry-run` | on unless `--apply` | Report only; no file is deleted. |
| `--apply` | off | Actually delete the files. |
| `-d, --days <n>` | `0` | Only purge empty databases older than N days. |

`--dry-run` and `--apply` are not exclusive: dry run wins unless `--apply` is explicitly passed.

```bash
xavier cleanup
xavier cleanup --apply --days 30
```

### `xavier encrypt-records`

Encrypt legacy plaintext memory records at rest, per record.

| Flag | Default | Description |
|---|---|---|
| `--dry-run` | on unless `--apply` | Report only. |
| `--apply` | off | Actually encrypt the rows. |

### `xavier mirror-export`

Export a neutral mirror package (JSONL) carrying content and graph but no embeddings. Embeddings are intentionally excluded: different models live in different vector spaces, and each side recomputes its own.

| Flag | Default | Description |
|---|---|---|
| `-o, --out <path>` | required | Output JSONL file. |
| `-l, --limit <n>` | none | Limit the number of exported memory records. |
| `--since <date>` | none | Only memories created at/after this ISO-8601 date (e.g. `2026-09-01`). The graph follows the window: an edge is exported only when both endpoints are inside it, so no dangling references travel. |

### `xavier mirror-import`

Import a neutral mirror package (JSONL) with content-hash dedupe.

| Flag | Default | Description |
|---|---|---|
| `-i, --in <path>` | required | Input JSONL package file. |

```bash
xavier mirror-export --out package.jsonl --since 2026-09-01
xavier mirror-import --in package.jsonl
```

## Air Gap

Offline storage capsule protocol for USB transport. Capsules are `.swal_capsule` files encrypted with AES-256-GCM under an Argon2id-derived key.

### `xavier airgap detect`

Detect removable USB storage devices and show mount points and capacity.

| Flag | Default | Description |
|---|---|---|
| `--json` | off | Emit the device list as JSON. |

### `xavier airgap pack`

Pack a file, directory, or database backup into an encrypted capsule.

| Flag | Default | Description |
|---|---|---|
| `-i, --input <path>` | required | File or directory to pack. |
| `-o, --output <path>` | `<input>.swal_capsule` | Output capsule file path. |
| `-p, --passphrase <text>` | prompt | Passphrase; prompted interactively when omitted. |
| `-k, --kind <kind>` | inferred from the input | `file`, `directory`, `db_backup`, `memory_dump`, `secrets_vault`. |
| `--author <name>` | `$USER` / `xavier-node` | Author or operator identity tag. |

### `xavier airgap unpack`

Unpack an encrypted capsule to a destination directory.

| Flag | Default | Description |
|---|---|---|
| `-c, --capsule <path>` | required | Path to the `.swal_capsule` file. |
| `-o, --output-dir <path>` | required | Destination directory. |
| `-p, --passphrase <text>` | prompt | Passphrase; prompted interactively when omitted. |

### `xavier airgap inspect`

Inspect the unencrypted header metadata of a capsule.

| Flag | Default | Description |
|---|---|---|
| `-c, --capsule <path>` | required | Path to the `.swal_capsule` file. |
| `--json` | off | Emit the header as JSON. |

```bash
xavier airgap detect
xavier airgap pack --input ./backup.db --kind db_backup
xavier airgap inspect --capsule ./backup.db.swal_capsule --json
```

## Local Users

Manages local accounts directly in `<state>/.xavier/auth.db` — the same database the HTTP server uses — so no server has to be running.

### `xavier users list`

List local accounts (id, email, role). Never prints password hashes.

| Flag | Default | Description |
|---|---|---|
| `--json` | `false` | Emit JSON instead of a table. |

### `xavier users set-role <email> <role>`

Change the role of an account. `role` is `user` or `admin`.

| Flag | Default | Description |
|---|---|---|
| `--json` | `false` | Emit JSON instead of human-readable text. |

### `xavier users reset-password <email>`

Reset an account password: generates a strong random password, stores it with the standard Argon2id hash, and prints it **once**.

| Flag | Default | Description |
|---|---|---|
| `--json` | `false` | Emit JSON instead of human-readable text. |

### `xavier users create --email <email>`

Create a local account interactively: hidden double password prompt, the same validation and Argon2id hashing as `POST /auth/register`, plus a 24-word Spanish BIP39 recovery phrase printed **once**.

| Flag | Default | Description |
|---|---|---|
| `--email <email>` | required | Normalized to lowercase; must be unique. |
| `--role <role>` | `user` | `user` or `admin`. |
| `--name <name>` | the email | Display name. |
| `--i-understand-output-is-not-a-tty` | `false` | Allow printing the recovery phrase when stdout is not a TTY. Off by default so secrets are never dumped to a redirected stdout. |

### `xavier users totp-enroll --email <email>`

Enroll TOTP 2FA: prints a scannable Unicode QR plus the base32 secret, asks for a 6-digit confirmation code, then shows the 10 one-time backup codes. Compatible with Google and Microsoft Authenticator (SHA1, 6 digits, 30s step, issuer `Xavier`). Uses the same secret/QR generation and code verification as `POST /auth/2fa/setup` + `/auth/2fa/verify`, with no server required.

| Flag | Default | Description |
|---|---|---|
| `--email <email>` | required | Account email (case-insensitive). |
| `--i-understand-output-is-not-a-tty` | `false` | Allow printing the QR, secret and backup codes to a non-TTY stdout. |

### `xavier users totp-disable --email <email>`

Disable TOTP 2FA. Requires a valid current TOTP code or an unused backup code (consumed on use); refuses to disable otherwise.

| Flag | Default | Description |
|---|---|---|
| `--email <email>` | required | Account email (case-insensitive). |
| `--code <code>` | prompt | TOTP code or backup code; prompted interactively (hidden) when omitted. |

```bash
xavier users list --json
xavier users set-role admin@example.com admin
xavier users totp-enroll --email admin@example.com
```

## Other Commands

| Command | Description |
|---|---|
| `xavier setup` | Run interactive system detection and setup. `--local` runs the 100% local guided flow (Ollama). |
| `xavier quota` | Show provider quotas and limits. |
| `xavier chronicle <subcommand>` | Manage Chronicle docs/devlog workflows. |
| `xavier chat [prompt]` / `xavier ask <prompt>` | Conversational chat with memory retrieval. `-a, --agent`, `-i, --interactive`, `--json`, `-l, --limit` (default `5`), `-m, --model`. |
| `xavier exec <command...>` | Run a shell command through the RTK kernel proxy with token reduction. `-s, --session`, `-C, --cwd`. |
| `xavier reindex` | Re-index all memories missing embeddings. |
| `xavier issue pack <id>` | Package context for a GitHub issue. `-r, --repo`. |
| `xavier license <status\|accept\|show>` | Show, accept (enables mesh features), or print the license terms. |
| `xavier memory <subcommand>` | `index-self`, `consolidate` (`--start`/`--stop`/`--status`/`--nightly`), `import-markdown <dir>`, `export-markdown <dir>` (`--public-only`), `prune` (`--prefix`, `--older-than-days`, `--dry-run`, `-y`, `--json`). |
| `xavier sync <subcommand>` | `status`, `now` (`-m, --mode`, default `bidirectional`), `check`. |
| `xavier scan <subcommand>` | `system` (`-f, --format`, `-d, --detailed`) and `security` (`-f, --format`). |
| `xavier task <subcommand>` | `list` (`-p`, `-s`, `--search`, `-f`), `create <title>` (`-p`, `-d`), `run <id>`, `move <id> <status>`. |
| `xavier agent <subcommand>` | `scan`, `index`, `push`, `pull`, `chat`, `converse` — IDE agent session import. `index` also takes `--codex`, `--jules`, `--antigravity`, `--opencode`. |
| `xavier cloud <subcommand>` | `status`, `set-backend <backend>`, `sync`, `verify` — each with `--json`. |
| `xavier plugin <subcommand>` | `install <name>`, `list`. |
| `xavier mini-expert <subcommand>` | `add` (`--name`, `--segment`, `--domain`, `--language`, `--clearance`, `--source-dataset`, `--model-gguf-path`, `--provider`, `--endpoint`, `--version`, `--metrics-file`, `--candidate`; all stored in the SQLite registry), `list`, `activate <name> <version>`, `retire <name> [--version]`, `serve` (`--name`; makes sure active experts exist in Ollama, whose address comes from `XAVIER_LOCAL_LLM_URL`; `--port` is rejected). Only `local`/`ollama` providers are served by the expert router and `ask_expert`; other providers are honoured only by the legacy HTTP invoke route. `XAVIER_EXPERT_THRESHOLD` is clamped to [-1, 1] (NaN/inf use the default 0.6). |
| `xavier node <subcommand>` | SWAL node identity: `create`, `recover`, `status`, `anchor`, `anchor-pack`. |
| `xavier nodes <subcommand>` | Node provisioning: `add`, `list`, `show`, `rotate`, `remove`, `status`. |
| `xavier governance <subcommand>` | `list`, `create <title> <description>`, `status <id>`, `vote <id>`, `council`. |
| `xavier wallet <subcommand>` | `balance`, `transactions` (`-l, --limit`, default `10`). Skeleton. |
| `xavier maturity <subcommand>` | Feature maturity scan and reporting. |
| `xavier telecom <subcommand>` | Private node-to-node telecom. |
| `xavier index-self` | Index foundational documentation into the memory store. |
| `xavier nav telemetry [kind]` | Navigation telemetry; pass `hotspots` for the top-10 visited nodes. |
