# Test Battery — Ola Graph / full feature probe
Started: 2026-07-17T22:56:23.8290801-05:00

## panel-ui-test — PASS (s)
```n
> xavier-panel-ui@0.10.0-12-06-2026 test E:\proyectosSWAL\xavier\panel-ui
> vitest run


 RUN  v4.1.8 E:/proyectosSWAL/xavier/panel-ui

 Ô£ô tests/roadmapGraph.test.ts (3 tests) 5ms
 Ô£ô tests/graphAdapters.test.ts (6 tests) 10ms
 Ô£ô tests/inputArea.a11y.test.tsx (2 tests) 188ms
 Ô£ô tests/operationModeBadge.test.tsx (4 tests) 128ms
 Ô£ô tests/auth.test.tsx (3 tests) 205ms

 Test Files  5 passed (5)
      Tests  18 passed (18)
   Start at  22:56:24
   Duration  2.88s (transform 308ms, setup 969ms, import 1.88s, tests 536ms, environment 7.26s)


```

## panel-ui-typecheck — PASS (s)
```n
> xavier-panel-ui@0.10.0-12-06-2026 typecheck E:\proyectosSWAL\xavier\panel-ui
> tsc --noEmit -p tsconfig.generative.json


```

## panel-ui-build — PASS (s)
```n
> xavier-panel-ui@0.10.0-12-06-2026 build E:\proyectosSWAL\xavier\panel-ui
> vite build

vite v8.0.16 building client environment for production...
[2K
transforming...Ô£ô 3635 modules transformed.
rendering chunks...
computing gzip size...
build/index.html            0.38 kB Ôöé gzip:   0.24 kB
build/assets/index.css     92.01 kB Ôöé gzip:  13.45 kB
build/assets/index.js   1,103.82 kB Ôöé gzip: 321.02 kB

Ô£ô built in 856ms

```

## cargo-check-ci-safe — PASS (s)
```n    Checking hashbrown v0.17.1
   Compiling winapi v0.3.9
    Checking either v1.16.0
    Checking time v0.3.53
    Checking rusqlite v0.32.1
    Checking tower-http v0.6.11
    Checking pulldown-cmark v0.13.4
    Checking libsql v0.9.30
    Checking git2 v0.21.0
    Checking rayon v1.12.0
    Checking indexmap v2.14.0
    Checking r2d2_sqlite v0.25.0
    Checking h2 v0.4.15
    Checking sqlx-core v0.8.6
    Checking serde_yaml v0.9.34+deprecated
    Checking quanta v0.10.1
    Checking ntapi v0.4.3
    Checking metrics-util v0.14.0
    Checking simple_asn1 v0.6.4
    Checking tracing-appender v0.2.5
    Checking jsonwebtoken v10.4.0
    Checking metrics-exporter-prometheus v0.11.0
    Checking autometrics v0.3.3
    Checking sqlx-postgres v0.8.6
    Checking sysinfo v0.36.1
    Checking hyper v1.10.1
    Checking sqlx v0.8.6
    Checking hyper-util v0.1.20
    Checking axum v0.8.9
    Checking hyper-rustls v0.27.9
    Checking axum-server v0.8.0
    Checking reqwest v0.13.4
    Checking code-graph v0.6.1-beta (E:\proyectosSWAL\xavier\code-graph)
    Checking xavier v0.12.0 (E:\proyectosSWAL\xavier)
warning: unused import: `Context`
 --> src\agents\provider\client.rs:6:22
  |
6 | use anyhow::{anyhow, Context, Result};
  |                      ^^^^^^^
  |
  = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

warning: unused import: `VirtualMemoryEntry`
  --> src\context\orchestrator.rs:19:52
   |
19 | use crate::memory::virtual_memory::{VirtualMemory, VirtualMemoryEntry};
   |                                                    ^^^^^^^^^^^^^^^^^^

warning: unused import: `rand::rngs::OsRng`
 --> src\security\auth.rs:6:5
  |
6 | use rand::rngs::OsRng;
  |     ^^^^^^^^^^^^^^^^^

warning: unused import: `rand::RngCore`
 --> src\security\auth.rs:7:5
  |
7 | use rand::RngCore;
  |     ^^^^^^^^^^^^^

warning: unused import: `crate::security::auth_store::AuthStore`
  --> src\security\auth.rs:15:5
   |
15 | use crate::security::auth_store::AuthStore;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: unused import: `Context`
 --> src\security\auth_store.rs:5:22
  |
5 | use anyhow::{Result, Context, anyhow};
  |                      ^^^^^^^

warning: unused imports: `Deserialize` and `Serialize`
 --> src\security\auth_store.rs:8:13
  |
8 | use serde::{Deserialize, Serialize};
  |             ^^^^^^^^^^^  ^^^^^^^^^

warning: unused import: `DateTime`
 --> src\security\auth_store.rs:9:14
  |
9 | use chrono::{DateTime, Utc};
  |              ^^^^^^^^

warning: unused import: `Payload`
  --> src\security\auth_store.rs:11:27
   |
11 |     aead::{Aead, KeyInit, Payload},
   |                           ^^^^^^^

warning: unused import: `UserRole`
  --> src\security\auth_store.rs:16:29
   |
16 | use crate::security::auth::{UserRole, User};
   |                             ^^^^^^^^

warning: unused variable: `lifecycle`
   --> src\agents\mod.rs:146:9
    |
146 |         lifecycle: Option<Arc<dyn AgentLifecyclePort>>,
    |         ^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_lifecycle`
    |
    = note: `#[warn(unused_variables)]` (part of `#[warn(unused)]`) on by default

warning: unused variable: `result`
   --> src\agents\mod.rs:189:13
    |
189 |         let result = runtime.run(&task, None, None).await;
    |             ^^^^^^ help: if this is intentional, prefix it with an underscore: `_result`

warning: variable does not need to be mutable
   --> src\context\builder.rs:100:13
    |
100 |         let mut compressed = context.replace("  ", " ").replace("\n\n\n", "\n\n");
    |             ----^^^^^^^^^^
    |             |
    |             help: remove this `mut`
    |
    = note: `#[warn(unused_mut)]` (part of `#[warn(unused)]`) on by default

warning: unused variable: `compressed`
   --> src\context\builder.rs:100:13
    |
100 |         let mut compressed = co
```

## cargo-test-lib-graph — PASS (s)
```n   Compiling hashbrown v0.17.1
   Compiling winapi v0.3.9
   Compiling raw-cpuid v11.6.0
   Compiling time v0.3.53
   Compiling rusqlite v0.32.1
   Compiling png v0.18.1
   Compiling tower-http v0.6.11
   Compiling proptest v1.11.0
   Compiling pulldown-cmark v0.13.4
   Compiling libsql v0.9.30
   Compiling git2 v0.21.0
   Compiling indexmap v2.14.0
   Compiling serde_json v1.0.150
   Compiling h2 v0.4.15
   Compiling quanta v0.10.1
   Compiling sqlx-core v0.8.6
   Compiling ntapi v0.4.3
   Compiling pulp v0.22.3
   Compiling metrics-util v0.14.0
   Compiling simple_asn1 v0.6.4
   Compiling metrics-exporter-prometheus v0.11.0
   Compiling sysinfo v0.36.1
   Compiling tree-sitter v0.26.11
   Compiling jsonwebtoken v10.4.0
   Compiling tracing-appender v0.2.5
   Compiling autometrics v0.3.3
   Compiling sqlx-postgres v0.8.6
   Compiling serde_yaml v0.9.34+deprecated
   Compiling r2d2_sqlite v0.25.0
   Compiling hyper v1.10.1
   Compiling exr v1.74.2
   Compiling hyper-util v0.1.20
   Compiling hyper-rustls v0.27.9
   Compiling axum v0.8.9
   Compiling mockito v1.7.2
   Compiling axum-server v0.8.0
   Compiling reqwest v0.13.4
   Compiling sqlx-macros-core v0.8.6
   Compiling image v0.25.10
   Compiling sqlx-macros v0.8.6
   Compiling code-graph v0.6.1-beta (E:\proyectosSWAL\xavier\code-graph)
   Compiling sqlx v0.8.6
   Compiling qrcode v0.14.1
   Compiling xavier v0.12.0 (E:\proyectosSWAL\xavier)
warning: unused import: `Context`
 --> src\agents\provider\client.rs:6:22
  |
6 | use anyhow::{anyhow, Context, Result};
  |                      ^^^^^^^
  |
  = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

warning: unused import: `VirtualMemoryEntry`
  --> src\context\orchestrator.rs:19:52
   |
19 | use crate::memory::virtual_memory::{VirtualMemory, VirtualMemoryEntry};
   |                                                    ^^^^^^^^^^^^^^^^^^

warning: unused import: `rand::rngs::OsRng`
 --> src\security\auth.rs:6:5
  |
6 | use rand::rngs::OsRng;
  |     ^^^^^^^^^^^^^^^^^

warning: unused import: `rand::RngCore`
 --> src\security\auth.rs:7:5
  |
7 | use rand::RngCore;
  |     ^^^^^^^^^^^^^

warning: unused import: `crate::security::auth_store::AuthStore`
  --> src\security\auth.rs:15:5
   |
15 | use crate::security::auth_store::AuthStore;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: unused import: `Context`
 --> src\security\auth_store.rs:5:22
  |
5 | use anyhow::{Result, Context, anyhow};
  |                      ^^^^^^^

warning: unused imports: `Deserialize` and `Serialize`
 --> src\security\auth_store.rs:8:13
  |
8 | use serde::{Deserialize, Serialize};
  |             ^^^^^^^^^^^  ^^^^^^^^^

warning: unused import: `DateTime`
 --> src\security\auth_store.rs:9:14
  |
9 | use chrono::{DateTime, Utc};
  |              ^^^^^^^^

warning: unused import: `Payload`
  --> src\security\auth_store.rs:11:27
   |
11 |     aead::{Aead, KeyInit, Payload},
   |                           ^^^^^^^

warning: unused import: `UserRole`
  --> src\security\auth_store.rs:16:29
   |
16 | use crate::security::auth::{UserRole, User};
   |                             ^^^^^^^^

warning: unused variable: `lifecycle`
   --> src\agents\mod.rs:146:9
    |
146 |         lifecycle: Option<Arc<dyn AgentLifecyclePort>>,
    |         ^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_lifecycle`
    |
    = note: `#[warn(unused_variables)]` (part of `#[warn(unused)]`) on by default

warning: unused variable: `result`
   --> src\agents\mod.rs:189:13
    |
189 |         let result = runtime.run(&task, None, None).await;
    |             ^^^^^^ help: if this is intentional, prefix it with an underscore: `_result`

warning: variable does not need to be mutable
   --> src\context\builder.rs:100:13
    |
100 |         let mut compressed = context.replace("  ", " ").replace("\n\n\n", "\n\
```

## cargo-test-lib-panel — PASS (s)
```nwarning: unused import: `Context`
 --> src\agents\provider\client.rs:6:22
  |
6 | use anyhow::{anyhow, Context, Result};
  |                      ^^^^^^^
  |
  = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

warning: unused import: `VirtualMemoryEntry`
  --> src\context\orchestrator.rs:19:52
   |
19 | use crate::memory::virtual_memory::{VirtualMemory, VirtualMemoryEntry};
   |                                                    ^^^^^^^^^^^^^^^^^^

warning: unused import: `rand::rngs::OsRng`
 --> src\security\auth.rs:6:5
  |
6 | use rand::rngs::OsRng;
  |     ^^^^^^^^^^^^^^^^^

warning: unused import: `rand::RngCore`
 --> src\security\auth.rs:7:5
  |
7 | use rand::RngCore;
  |     ^^^^^^^^^^^^^

warning: unused import: `crate::security::auth_store::AuthStore`
  --> src\security\auth.rs:15:5
   |
15 | use crate::security::auth_store::AuthStore;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: unused import: `Context`
 --> src\security\auth_store.rs:5:22
  |
5 | use anyhow::{Result, Context, anyhow};
  |                      ^^^^^^^

warning: unused imports: `Deserialize` and `Serialize`
 --> src\security\auth_store.rs:8:13
  |
8 | use serde::{Deserialize, Serialize};
  |             ^^^^^^^^^^^  ^^^^^^^^^

warning: unused import: `DateTime`
 --> src\security\auth_store.rs:9:14
  |
9 | use chrono::{DateTime, Utc};
  |              ^^^^^^^^

warning: unused import: `Payload`
  --> src\security\auth_store.rs:11:27
   |
11 |     aead::{Aead, KeyInit, Payload},
   |                           ^^^^^^^

warning: unused import: `UserRole`
  --> src\security\auth_store.rs:16:29
   |
16 | use crate::security::auth::{UserRole, User};
   |                             ^^^^^^^^

warning: unused variable: `lifecycle`
   --> src\agents\mod.rs:146:9
    |
146 |         lifecycle: Option<Arc<dyn AgentLifecyclePort>>,
    |         ^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_lifecycle`
    |
    = note: `#[warn(unused_variables)]` (part of `#[warn(unused)]`) on by default

warning: unused variable: `result`
   --> src\agents\mod.rs:189:13
    |
189 |         let result = runtime.run(&task, None, None).await;
    |             ^^^^^^ help: if this is intentional, prefix it with an underscore: `_result`

warning: variable does not need to be mutable
   --> src\context\builder.rs:100:13
    |
100 |         let mut compressed = context.replace("  ", " ").replace("\n\n\n", "\n\n");
    |             ----^^^^^^^^^^
    |             |
    |             help: remove this `mut`
    |
    = note: `#[warn(unused_mut)]` (part of `#[warn(unused)]`) on by default

warning: unused variable: `compressed`
   --> src\context\builder.rs:100:13
    |
100 |         let mut compressed = context.replace("  ", " ").replace("\n\n\n", "\n\n");
    |             ^^^^^^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_compressed`

warning: method `with_lease_token` is never used
   --> src\agents\provider\config.rs:625:19
    |
 63 | impl ModelProviderConfig {
    | ------------------------ method in this implementation
...
625 |     pub(crate) fn with_lease_token(mut self, token: Option<String>) -> Self {
    |                   ^^^^^^^^^^^^^^^^
    |
    = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: `xavier` (lib test) generated 15 warnings (run `cargo fix --lib -p xavier --tests` to apply 14 suggestions)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1.07s
     Running unittests src\lib.rs (target\debug\deps\xavier-95008356d5db51f4.exe)

running 5 tests
test server::panel::tests::widgets_crud ... ok
test server::panel::tests::creates_and_fetches_threads_via_http ... ok
test server::panel::tests::bookmarks_crud ... ok
test server::panel::tests::graphs_crud ... ok
Response body: {"thread":{"id":"2593c613-6397-4e27-bb2a-32811c
```

## cargo-test-entity-graph — PASS (s)
```nwarning: unused import: `Context`
 --> src\agents\provider\client.rs:6:22
  |
6 | use anyhow::{anyhow, Context, Result};
  |                      ^^^^^^^
  |
  = note: `#[warn(unused_imports)]` (part of `#[warn(unused)]`) on by default

warning: unused import: `VirtualMemoryEntry`
  --> src\context\orchestrator.rs:19:52
   |
19 | use crate::memory::virtual_memory::{VirtualMemory, VirtualMemoryEntry};
   |                                                    ^^^^^^^^^^^^^^^^^^

warning: unused import: `rand::rngs::OsRng`
 --> src\security\auth.rs:6:5
  |
6 | use rand::rngs::OsRng;
  |     ^^^^^^^^^^^^^^^^^

warning: unused import: `rand::RngCore`
 --> src\security\auth.rs:7:5
  |
7 | use rand::RngCore;
  |     ^^^^^^^^^^^^^

warning: unused import: `crate::security::auth_store::AuthStore`
  --> src\security\auth.rs:15:5
   |
15 | use crate::security::auth_store::AuthStore;
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^

warning: unused import: `Context`
 --> src\security\auth_store.rs:5:22
  |
5 | use anyhow::{Result, Context, anyhow};
  |                      ^^^^^^^

warning: unused imports: `Deserialize` and `Serialize`
 --> src\security\auth_store.rs:8:13
  |
8 | use serde::{Deserialize, Serialize};
  |             ^^^^^^^^^^^  ^^^^^^^^^

warning: unused import: `DateTime`
 --> src\security\auth_store.rs:9:14
  |
9 | use chrono::{DateTime, Utc};
  |              ^^^^^^^^

warning: unused import: `Payload`
  --> src\security\auth_store.rs:11:27
   |
11 |     aead::{Aead, KeyInit, Payload},
   |                           ^^^^^^^

warning: unused import: `UserRole`
  --> src\security\auth_store.rs:16:29
   |
16 | use crate::security::auth::{UserRole, User};
   |                             ^^^^^^^^

warning: unused variable: `lifecycle`
   --> src\agents\mod.rs:146:9
    |
146 |         lifecycle: Option<Arc<dyn AgentLifecyclePort>>,
    |         ^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_lifecycle`
    |
    = note: `#[warn(unused_variables)]` (part of `#[warn(unused)]`) on by default

warning: unused variable: `result`
   --> src\agents\mod.rs:189:13
    |
189 |         let result = runtime.run(&task, None, None).await;
    |             ^^^^^^ help: if this is intentional, prefix it with an underscore: `_result`

warning: variable does not need to be mutable
   --> src\context\builder.rs:100:13
    |
100 |         let mut compressed = context.replace("  ", " ").replace("\n\n\n", "\n\n");
    |             ----^^^^^^^^^^
    |             |
    |             help: remove this `mut`
    |
    = note: `#[warn(unused_mut)]` (part of `#[warn(unused)]`) on by default

warning: unused variable: `compressed`
   --> src\context\builder.rs:100:13
    |
100 |         let mut compressed = context.replace("  ", " ").replace("\n\n\n", "\n\n");
    |             ^^^^^^^^^^^^^^ help: if this is intentional, prefix it with an underscore: `_compressed`

warning: method `with_lease_token` is never used
   --> src\agents\provider\config.rs:625:19
    |
 63 | impl ModelProviderConfig {
    | ------------------------ method in this implementation
...
625 |     pub(crate) fn with_lease_token(mut self, token: Option<String>) -> Self {
    |                   ^^^^^^^^^^^^^^^^
    |
    = note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: `xavier` (lib test) generated 15 warnings (run `cargo fix --lib -p xavier --tests` to apply 14 suggestions)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 1.05s
     Running unittests src\lib.rs (target\debug\deps\xavier-95008356d5db51f4.exe)

running 6 tests
test memory::entity_graph::tests::test_decay ... ok
test memory::entity_graph::tests::serialization_roundtrip ... ok
test memory::entity_graph::tests::indexes_entities_relations_and_traversal ... ok
test memory::entity_graph::tests::merges_entities_and_preserves_aliases ... o
```
