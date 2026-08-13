# Xavier Dual License — AGPL v3 + Commercial

## What changed

| Before | After |
|--------|-------|
| MIT (standalone) + Mesh License (proprietary) | AGPL v3 (core) + Commercial License (enterprise) |
| Mesh License was separate, no OSI approval | AGPL v3 is OSI-approved, full open source |
| Confusing: same code had different license paths | Simple: AGPL for everyone, Commercial if you need proprietary integration |
| No viral protection for network services | AGPL requires source disclosure for modified network services |

## Why AGPL instead of MIT

MIT is great for libraries. Xavier is **not a library** — it's a **cognitive memory runtime** that runs as a service/daemon. The AGPL is designed for exactly this use case: if you modify Xavier and offer it as a network service, you must release your modifications.

This mirrors:
- **MongoDB** — moved MIT → SSPL after cloud providers extracted value
- **Supabase** — uses Apache 2.0 (permissive) but their value is in managed hosting
- **GitLab** — MIT for CE, proprietary EE
- **HashiCorp** — moved MPL → BSL v1.1 after cloud competition
- **Elementl/Dagster** — Apache 2.0 core, Dagster+ commercial

For Xavier's specific model (mesh p2p + memory sharing + Data Commons), AGPL provides the protection MIT lacks: anyone running a mesh node must contribute improvements back.

## The Dual Model

```
Xavier Core ─── AGPL v3 ─── Free forever
  │
  ├── All source code visible
  ├── All features unlocked
  ├── Self-host, modify, redistribute
  ├── Network service + mesh = must share improvements
  └── Commercial use allowed if no proprietary modifications kept private
  │
Xavier Enterprise ─── Commercial License ─── Paid
  │
  ├── Integrate Xavier in proprietary product
  ├── Closed-source modifications
  ├── Private mesh network without source disclosure
  ├── Priority support + SLA
  ├── Advanced features (advanced-rrf, enterprise-embeddings, advanced-audit-logging)
  └── Per-node or per-organization pricing
```

## How it works technically

The `Cargo.toml` already has feature flags for enterprise features and mesh:

```toml
[features]
enterprise = []  # enterprise-reserved features
mesh = ["dep:libp2p"]
data-commons = ["post-quantum"]
```

The existing `license.rs` in `src/security/` already gates mesh features behind license acceptance. We only need to:
1. Change `LICENSE` from MIT → AGPL v3
2. Update `src/security/license.rs` to detect AGPL + Commercial
3. Add `COMMERCIAL_LICENSE.md` with pricing tiers

## Business model

| Tier | Price | Features |
|------|-------|----------|
| **Community** | Free | Full Xavier under AGPL v3. Self-host, mesh, all features. |
| **Commercial** | $100/node/mo | Commercial license, proprietary integration, modified mesh without source disclosure, support included |
| **Enterprise** | Custom | Above + Advanced features (RRF, embeddings, audit), SLA, dedicated support |

## License compatibility note

AGPL v3 is compatible with:
- SWAL's existing projects (all MIT/Apache 2.0)
- OpenClaw (MIT)
- GitCore (MIT)
- Users who integrate Xavier as a network service (must share modifications)
- Users who run Xavier internally (no disclosure required)

## Migration path

1. Update `LICENSE` file: MIT → AGPL v3
2. Add `COMMERCIAL_LICENSE.md`
3. Update `src/security/license.rs` — add AGPL detection, keep Mesh License for enterprise features
4. Update `Cargo.toml` metadata
5. Update `docs/PRICING.md`
6. Accepting Mesh License = accepting Commercial terms
7. Notify existing contributors (SWAL team)
