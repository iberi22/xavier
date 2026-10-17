# Espacio guide (memory vaults)

Companion to [ADR-018](../adr/ADR-018-espacio-architecture.md). This guide describes what the
runtime does today, where each part lives, and what is still pending. Ledger entry:
`feat-espacio-runtime` (status `planned`, see `.gitcore/features.json`).

## What a space is

A space (espacio) is a memory vault: its own store, its own data key, its own tokens and its own
members. Its id is 1 to 64 characters from `[A-Za-z0-9_-]` (`validate_space_id`, `src/espacio/store.rs`).
Anything else is rejected before a path is built, so an id cannot escape the spaces directory.

Namespace: `xavier://{space_id}/{appId}/{instanceId}` (`SpaceManager::namespace_for`,
`src/espacio/manager.rs`). The namespace is a label. Isolation comes from the storage layout below.

## Storage layout and isolation

Spaces live under the daemon data dir: `{root}/spaces/{space_id}/`.

| File | Contents |
|------|----------|
| `space.json` | Descriptor. No secrets. Written atomically. |
| `keystore.json` | The space data key (DEK), wrapped under each enabled key-encryption key. |
| `espacio.sqlite` | Members, invites, messages, links, tokens. Versioned with `PRAGMA user_version`. |

Isolation is per file. Each space has its own SQLite database, so no table is shared between spaces.
Spaces do not go through `multi_db` or the legacy `WorkspaceRegistry`. Deleting a space moves its
directory to trash, and the cached handle is dropped first.

Open stores are cached per space (`SpaceStores`). Opening a store creates its directory and database
on first use. Spaces opened as workspaces are cached separately in `src/workspace/state.rs`, bounded
by `XAVIER_SPACE_WORKSPACE_CACHE_MAX` (default 64, clamped to 1..=1024). When that variable is unset,
the shared `XAVIER_STORE_BUDGET` applies instead.

## Keys and unlock modes

Each space owns one random 256-bit data key. The key never touches disk in clear. Each wrapper is an
AES-256-GCM envelope under a different key-encryption key (KEK). Full details are in the module
header of `src/espacio/keys.rs`.

| Unlock mode | Stored wrappers | Behaviour after restart |
|-------------|-----------------|-------------------------|
| `node_unlock` (default) | `node`, `recovery` (plus `password` when set) | Unlocks itself at boot from the node master key. |
| `password_required` | `password`, `recovery` | Stays locked until unlocked with the password or the recovery code. |

- A locked space refuses every store access. Its HTTP answer is 423 and it leaks nothing about its
  contents (`locked_space_is_423_and_leaks_nothing`).
- Records are sealed under the DEK when the space's `encrypt_records` flag is on. There is no
  plaintext fallback for an encrypted space. A space that any of the three persisted markers
  (`space.json`, the `meta` row, the `xr1:` row prefix) flags as encrypted, but whose keystore is
  missing, fails closed.
- Secret types redact their `Debug` output.

## Tokens

Space-scoped bearer tokens have the form `xsp_{space_id}_{43-char base64url}`.

- Only the SHA-256 of the full token string is stored, in the space's own `tokens` table. The raw
  token is returned once, at issue time.
- The space id in the prefix selects the database to open. It is opened with `get_existing`, so a
  forged prefix never creates a directory.
- Verification fails closed. Unknown, malformed, revoked, expired, or belonging to a removed member
  all give the same error.
- Default lifetime is 90 days. `XAVIER_SPACE_TOKEN_TTL_DAYS` overrides it (valid range 1 to 3650).

Roles, from `SpaceRole` in `src/espacio/invite.rs`: `Admin`, `Moderator`, `Member`, `Reader`.
Capabilities are checked with `can(role, action)` in `src/espacio/permissions.rs`. A space token
can reach memory only through the own-space allowlist (`space_token_may_access` in
`src/cli/http_setup.rs`). Routes that are global by nature refuse space tokens.

## Links

A link lets space A read space B's memory. B grants, A reads, never the reverse.

- A link is a `CrossGrant` stored in the grantor's `espacio.sqlite` (`links` table).
- It can carry a namespace and a path-prefix filter.
- Expiry and revocation are checked on every read.
- Nothing in the link path writes into the grantor's memory.
- Only the grantor's admin, or the node root credential, can create or revoke a link.

## HTTP and MCP surface

Production routes (mounted by `espacio_production_routes`, `src/adapters/inbound/http/routes.rs`):

| Method and path | Who | Purpose |
|-----------------|-----|---------|
| `POST /api/v1/espacio/admin/spaces` | root credential | Create a space. Accepts `password_required`. |
| `GET /api/v1/espacio/admin/spaces` | root credential | List all spaces. |
| `GET, DELETE /api/v1/espacio/admin/spaces/{id}` | root credential | Get or delete (to trash, with confirmation) any space. |
| `GET, DELETE /api/v1/espacio/spaces/{id}` | the space's own `xsp_` token | Get or delete its own space. |
| `POST, GET /api/v1/espacio/spaces/{id}/links` | own space admin or root | Create and list links. |
| `DELETE /api/v1/espacio/spaces/{id}/links/{link_id}` | own space admin or root | Revoke a link. |

With `XAVIER_SPACES=off|0|false|no|disabled`, no `SpaceManager` is opened. Every espacio route
answers 503 and `xsp_` tokens answer 401. The legacy path behaves as before.

MCP tools (`src/server/mcp/tools_core.rs`): `espacio_channel_list` and `espacio_channel_create`.
Both need the root credential. An admin JWT is not enough.

Memory through a space token is limited to that space. Its searches and writes do not reach other
spaces. Linked search includes only the spaces that granted a link, and skips locked ones.

## Legacy data

Pre-espacio data stays where it was. At boot, a descriptor with `legacy: true` is written to
`{spaces}/{id}/space.json` and points at the legacy paths by reference. Legacy data is not moved,
copied, rewritten or re-encrypted. The legacy descriptor is not a registered, token-addressable space
and gets no store, links or tokens. It only appears in the admin plane. `*.sqlite` files under the
legacy db dir are listed as adoptable. There is no adopt action yet (`src/espacio/migrate.rs`).

## Not built yet

These items are pending. Do not rely on them until their ledger entry moves past `planned`.

- **Per-space rules.** The `SpaceProtocol` type lives in `src/workspace/protocol.rs` and is used by the
  workspace code. The espacio store builder does not consume it, so every space gets the same rules.
- **Retention.** `retention_days` is not enforced for spaces.
- **Member and token management over HTTP and MCP.** There are no routes to add or remove members or
  to issue member tokens. The corresponding tests assert that such paths are refused.
- **Invites over HTTP.** `InviteManager` exists (invites expire after 24 hours, with an optional Ed25519
  signature checked only against a trusted key). No route creates or accepts invites.
- **`xavier space` CLI.** Not present.
- **Node-to-node transport.** `src/espacio/p2p.rs` models the registry and the AES-256-GCM envelope.
  The transport dial is not wired.
- **Channel replication.** `src/espacio/channel.rs` keeps a local append-only log. Gossip and offline
  queue are not wired.

The modules for public packs, marketplace folders and interest search (`public.rs`, `marketplace.rs`,
`search.rs`) are not covered by the guarantees in this guide.

## Tests

The CI job `espacio-tests` (`.github/workflows/ci.yml`) runs six integration suites:
`espacio_memory_encryption`, `espacio_isolation_memory`, `espacio_links`, `espacio_space_paths`,
`espacio_production_router` and `espacio_isolation_e2e`. The integration module
`tests/integration/espacio_lifecycle_test.rs` covers channels, namespaces, invites and membership.

Passing tests show what is built today. They do not show that the pending items exist.
