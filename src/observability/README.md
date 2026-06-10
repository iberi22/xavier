# Observability Module

Autonomous logging, monitoring, error detection, analysis, and self-healing for Xavier.

## Architecture

```
                     ┌─────────────────┐
                     │   HTTP Request   │
                     └────────┬────────┘
                              │
                     ┌────────▼────────┐
                     │   Middleware     │─── tracing!(stdout + file)
                     │  request_logger  │─── ServiceLogStore (5xx errors)
                     └────────┬────────┘
                              │
                     ┌────────▼────────┐
                     │  ServiceLogStore  │
                     │  (SQLite + FTS5)  │
                     └────────┬────────┘
                              │
                    ┌─────────▼──────────┐
                    │   LogDetector      │ ← cron: every 5 min
                    │  (pattern detection │
                    │   burst detection)  │
                    └─────────┬──────────┘
                              │
                    ┌─────────▼──────────┐
                    │   ErrorAnalyzer    │
                    │  (root cause +     │
                    │   fix suggestion)  │
                    └─────────┬──────────┘
                              │
              ┌───────────────┼───────────────┐
              ▼               ▼               ▼
        ┌──────────┐   ┌──────────┐   ┌──────────────┐
        │  Fixer   │   │ Notifier │   │ GitHub Issue │
        │ (auto-PR)│   │(Telegram)│   │(manual fix)  │
        └──────────┘   └──────────┘   └──────────────┘
```

## Module Structure

| File | Purpose |
|------|---------|
| `mod.rs` | Module root, re-exports, `init_logger()` |
| `service_log.rs` | SQLite-backed log store with FTS5 search |
| `middleware.rs` | Axum middleware for request/response logging |
| `detector.rs` | Periodic error pattern detection |
| `analyzer.rs` | Root cause analysis with heuristic classification |
| `fixer.rs` | GitHub Issue/PR generation via `gh` CLI |
| `notifier.rs` | Telegram alerts for critical errors |

## Key Concepts

### ServiceLogStore
Writes to Xavier's `vec-store.sqlite3` under the `service_logs` table with FTS5 full-text search. All logs are structured with level, source, module, correlation_id, and metadata.

### LogDetector
Runs every 5 minutes. Detects:
- **Patterns**: Same module + message repeated > 3 times in 1 hour
- **Bursts**: Same module with > 15 errors in 1 hour
- **New Errors**: First-time error signature (deduplicated)

### ErrorAnalyzer
Uses heuristic classification based on error message patterns:
- Authentication failures (0.9 confidence)
- HTTP 500 errors (0.7 confidence)
- Network/database issues (0.8-0.85 confidence)
- AI/ML model errors (0.75 confidence)
- OOM/memory errors (0.6 confidence)
- Generic fallback with frequency context

### Fixer
Creates GitHub Issues with the `gh` CLI. Supports:
- **Critical**: Immediate Issue creation
- **High**: Issue + notification
- **Medium**: Issue creation
- **Low**: Logged for periodic report

## Initialization

```rust
// At application startup
observability::init_logger(&log_dir, "info");

// Create service log store
let store = ServiceLogStore::new().await?;

// Attach HTTP middleware (in Router setup)
.route_layer(axum::middleware::from_fn_with_state(
    Arc::new(ObservabilityState::new()),
    request_logger,
));

// Start detector
let detector = Arc::new(LogDetector::new(store.clone(), DetectorConfig::default()));
LogDetector::spawn(detector);
```

## Monitoring Endpoints

| Endpoint | Description |
|----------|-------------|
| `GET /api/observability/stats` | Aggregate statistics (errors/hour, patterns, uptime) |
| `GET /api/observability/errors?module=X&limit=10` | Recent errors for a module |
| `GET /api/observability/search?q=error&limit=10` | Full-text search across logs |
| `GET /api/observability/patterns` | Active error patterns detected |
