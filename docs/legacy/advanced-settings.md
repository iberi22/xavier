# Advanced Settings

This document covers advanced configuration for specialized Xavier modules: PgHeart, Chronicle, and Agent Swarms.

---

## PgHeart (PostgreSQL Monitoring)

Xavier integrates with [PgHeart](https://github.com/iberi22/pgheart) to provide health monitoring and heartbeat synchronization for PostgreSQL instances.

### Configuration

You can configure PgHeart via the `pgheart` section in `xavier.config.json` or via environment variables.

| JSON Key | Env Var | Default | Description |
|----------|---------|---------|-------------|
| `url` | `PGHEART_URL` | - | Base URL of the PgHeart API |
| `token` | `PGHEART_TOKEN` | - | API authentication token |
| `instance_id` | `PGHEART_INSTANCE_ID` | - | Unique identifier for the instance being monitored |
| `sync_interval_ms` | `PGHEART_SYNC_INTERVAL_MS` | `60000` | Frequency of heartbeat updates in milliseconds |
| `auto_heartbeat` | `PGHEART_AUTO_HEARTBEAT` | `true` | Automatically send heartbeats while the server is running |

### Example Configuration (YAML)

```yaml
pgheart:
  url: "https://pgheart.example.com"
  token: "your-secret-token"
  instance_id: "production-db-1"
  sync_interval_ms: 30000
  auto_heartbeat: true
```

### Alerts and Thresholds

PgHeart monitors system health and can be configured to alert when specific thresholds are exceeded. Alerts are typically managed on the PgHeart server side, but Xavier's integration ensures that heartbeats carry the necessary metadata for anomaly detection.

---

## Chronicle (Daily Log System)

Chronicle is a semi-automated technical journaling system that harvests your daily work and generates human-readable blog posts.

### Workflow

1.  **Harvest**: `xavier chronicle harvest` collects commits, code changes, and session metadata.
2.  **Redact**: Automatically removes sensitive information (IPs, paths, tokens) using predefined patterns.
3.  **Generate**: `xavier chronicle generate` uses an LLM to transform data into a Markdown post.
4.  **Publish**: `xavier chronicle publish` exports the post to a directory or stdout.
5.  **Build**: `xavier chronicle build` generates a static blog in `public/devlog/`.

### Automation (Cron Schedule)

Chronicle tasks can be automated using standard cron jobs. A typical setup for daily logs might look like:

```bash
# Harvest data every night at 23:00
0 23 * * * xavier chronicle harvest --workspace /path/to/project

# Generate and publish post at 23:30
30 23 * * * xavier chronicle generate && xavier chronicle publish --to /path/to/project/docs/devlog
```

### Content Templates

Chronicle uses internal LLM prompt templates to structure generated posts. These are defined in `src/chronicle/prompts.rs`. You can influence the output by configuring `XAVIER_CHRONICLE_MODEL` to a more capable model (e.g., `gpt-4o` or `claude-3-5-sonnet`).

### Advanced Chronicle Configuration

| JSON Key | Env Var | Description |
|----------|---------|-------------|
| `model` | `XAVIER_CHRONICLE_MODEL` | LLM model for generation (overrides defaults) |

---

## Agents (Multi-Agent Systems)

Xavier supports complex multi-agent workflows, allowing you to spawn "swarms" of agents with different roles and skills.

### Spawning Agents

- **Manual Spawn**:
  ```bash
  xavier spawn --count 3 --providers openai,anthropic --skills research,coding
  ```
- **Swarm Config (TOML)**:
  ```toml
  [[agents]]
  name = "architect"
  provider = "anthropic"
  model = "claude-3-5-sonnet"
  task = "Analyze codebase architecture"
  skills = ["architecture", "research"]

  [[agents]]
  name = "implementer"
  provider = "local"
  task = "Write unit tests for new modules"
  skills = ["coding"]
  ```

### Provider Routing

Xavier routes queries based on complexity:

- **Fast Model**: `XAVIER_ROUTER_FAST_MODEL` (e.g., `llama-3-8b`) for simple tasks.
- **Quality Model**: `XAVIER_ROUTER_QUALITY_MODEL` (e.g., `gpt-4o`) for complex reasoning.

### Skill Loading

Skills are loaded from:
1. `skills/{name}/SKILL.md`
2. `skills/{name}.md`

### Budgeting

Set a limit for agent usage to control costs:
- **Env Var**: `XAVIER_WEEKLY_BUDGET` (numeric value)
