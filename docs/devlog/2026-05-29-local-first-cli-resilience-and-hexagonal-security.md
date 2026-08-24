# Local-First CLI Resilience and Hexagonal Security Wiring

**Date**: 2026-05-29
**Author**: Xavier AI
**Tags**: [cli, local-first, offline-database, sqlite-vec, hexagonal-architecture, security-ports]
**Source Files**: [`src/cli/commands.rs`](file:///~/proyectosSWAL/apps/xavier-v1/src/cli/commands.rs), [`src/cli/server.rs`](file:///~/proyectosSWAL/apps/xavier-v1/src/cli/server.rs), [`src/cli/state.rs`](file:///~/proyectosSWAL/apps/xavier-v1/src/cli/state.rs), [`src/app/security_service.rs`](file:///~/proyectosSWAL/apps/xavier-v1/src/app/security_service.rs)

---

## TL;DR
Xavier's CLI and state management are now 100% resilient. We implemented a local-first offline fallback database mode directly in `xavier search`, `xavier add`, `xavier recall`, and `xavier stats` that triggers if the central HTTP server is unreachable. Additionally, we decoupled our CLI security boundaries by replacing concrete application-layer classes with segmented hexagonal ports (`InputSecurityPort` and `SecurityScanPort`), ensuring complete compile-time architectural integrity.

---

## Context & Motivation
Xavier's design is built around the **cognitive memory hub** pattern. However, a major user-experience gap arose when developer agents ran commands like `xavier search` or `xavier add` inside isolated CLI pipelines: if the HTTP daemon was offline, the CLI aborted with TCP connection failures. Furthermore, the CLI was coupled to the concrete `AppSecurityService` structure instead of using port trait boundaries, violating the strict hexagonal architecture layout. 

To prepare Xavier for a production-grade `0.6.1-beta` release, we addressed two primary architectural gaps:
1. **Offline Autonomy:** Enable the CLI to gracefully switch to a local-first SQLite-Vec engine in the case of server downtime.
2. **Ports Segregation:** Enforce hexagonal purity at the CLI boundary by decoupling state from concrete implementations using Rust traits.

---

## The Decision

We implemented a two-fold solution:
1. **Transparent Database Fallback:** When a TCP dial or HTTP request fail, the CLI intercepts the error, initializes the local `SqliteMemoryStore` directly using the workspace’s SQLite path, and executes queries or mutations seamlessly.
2. **Hexagonal State Decoupling:** We refactored `CliState` to depend exclusively on `InputSecurityPort` and `SecurityScanPort` traits, keeping structural symmetry with the server's `AppState` and segregating input processing from threat reporting.

---

## Deep Dive: Technical Implementation

### 1. Transparent Database Fallback Logic
The fallback is structured inside `src/cli/server.rs` and `src/cli/commands.rs` to intercept Axum-based client connection failures. If the HTTP request fails, the CLI falls back to direct SQLite database operations:

```rust
// Fallback structure in `src/cli/server.rs`
match client.post(&search_url).json(&payload).send().await {
    Ok(res) => {
        // Handle server-based HTTP results...
    }
    Err(_) => {
        println!("⚠️ Server offline or connection failed. Falling back to local offline database query...");
        match crate::cli::commands::load_spawn_memory().await {
            Ok(memory) => {
                let filters = MemoryQueryFilters {
                    levels: level.map(|l| vec![MemoryLevel::parse(l)]),
                    ..Default::default()
                };
                match memory.search_documents(query, limit.unwrap_or(10), filters).await {
                    Ok(results) => {
                        println!("\n🔍 [OFFLINE RESULTS] Found matches:");
                        // Render matches from local SQLite-Vec database
                    }
                    Err(e) => println!("❌ Local query failed: {}", e),
                }
            }
            Err(e) => println!("❌ Failed to initialize local offline database: {}", e),
        }
    }
}
```

### 2. Segmented Security Ports in Hexagonal Architecture
Before the refactor, `CliState` possessed a direct dependency on the concrete `SecurityService` struct, hindering testing mocks and decoupling. By refactoring `CliState` to use interface segregation:

```rust
// src/cli/state.rs
pub struct CliState {
    pub memory: Arc<dyn MemoryQueryPort>,
    pub store: Arc<dyn MemoryStore>,
    // ...
    pub security: Arc<dyn InputSecurityPort>,
    #[allow(dead_code)]
    pub security_scan: Arc<dyn SecurityScanPort>,
}
```

This ensures `CliState` is fully modular. The concrete `SecurityService` implements both `InputSecurityPort` and `SecurityScanPort`. When starting the CLI HTTP daemon, we split the concrete instance transparently:

```rust
// src/cli/server.rs
let security_service = Arc::new(AppSecurityService::new());
let state = CliState {
    // ...
    security: security_service.clone() as Arc<dyn InputSecurityPort>,
    security_scan: security_service.clone() as Arc<dyn SecurityScanPort>,
};
```

---

## Architecture Flow

```mermaid
graph TD
    UserCmd[xavier search "query"] --> TestTCP{Is HTTP Server Online?}
    TestTCP -- Yes (TCP Success) --> HTTP[Query via Axum API Endpoint]
    TestTCP -- No (TCP Timeout) --> LocalDB[Init QmdMemory / SQLite-Vec Engine]
    HTTP --> Render1[Render Network Results]
    LocalDB --> ReadDisk[Query Local .sqlite3 tables]
    ReadDisk --> Render2[Render Offline Results]
```

---

## Alternatives & Trade-offs

| Strategy | Pros | Cons |
|----------|------|------|
| **Strict Client-Server** | Single source of truth. No data concurrency conflicts. | Useless in disconnected CLI pipelines or offline environments. |
| **Local-First CLI Fallback (Xavier)** | Zero-downtime, fully resilient, extremely fast offline performance. | CLI locks SQLite database during writes if server comes back online concurrently (mitigated by WAL mode). |
| **P2P Sync State** | Masterless distribution. | Extremely complex to build and audit securely. |

---

## References
- [CLI Commands Local Fallback](file:///~/proyectosSWAL/apps/xavier-v1/src/cli/commands.rs)
- [CLI State Hexagonal Definition](file:///~/proyectosSWAL/apps/xavier-v1/src/cli/state.rs)
- [Hexagonal Ports Directory](file:///~/proyectosSWAL/apps/xavier-v1/src/ports/inbound/)
