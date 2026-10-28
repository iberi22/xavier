# Xavier CLI

## Connecting an agent

Discover the configured REST and MCP endpoints without reading credential files:

```sh
xavier agent onboard --client claude --json
xavier agent onboard --client codex
xavier agent onboard --client opencode --json
xavier agent onboard --client generic --json
```

The REST URL follows `XAVIER_URL`, then `XAVIER_HOST`/`XAVIER_PORT`, then
settings/defaults. MCP uses the same host and scheme, `XAVIER_MCP_PORT`
(default `8100`), and `/mcp`. Port `0` reports MCP as disabled.
Onboarding probes REST `/health`; it does not test MCP connectivity or validate
the credential against an authenticated endpoint. A healthy probe alone does
not prove that subsequent authenticated requests will succeed.

JSON output has `schema_version: 1`, `base_url`, `mcp_url`, `health`
(`status`, `http_status`), `token_source` (`kind`, `reference`), `client`, and
`mcp_config` (a configuration snippet string). Token sources are the environment
variable `XAVIER_TOKEN`, vault reference `vault:xavier/XAVIER_TOKEN`, or `unset`.
The token value is omitted. Claude and generic snippets use `${XAVIER_TOKEN}`;
OpenCode uses `{env:XAVIER_TOKEN}`; Codex receives TOML with
`bearer_token_env_var = "XAVIER_TOKEN"`. Paste the snippet into the client's MCP
configuration and supply the credential through its environment.
The formats follow the [Claude Code MCP reference](https://code.claude.com/docs/en/mcp),
[Codex MCP reference](https://developers.openai.com/codex/mcp), and
[OpenCode MCP reference](https://opencode.ai/docs/mcp-servers/).
The OpenCode snippet disables OAuth discovery for this bearer-token connection.

An intentional token export requires both `--reveal` and
`--i-understand-output-is-not-a-tty`, and refuses terminal stdout even with
confirmation. Treat the redirected output as a credential:

```sh
(umask 077; xavier agent onboard --reveal --i-understand-output-is-not-a-tty --json > onboarding.json)
```

### Scoped REST tokens

A node administrator can issue an expiring token through the existing
`/security/tokens` API:

```sh
xavier token issue --agent indexer --scope write --ttl 2h --out-file ./indexer.key --json
xavier token issue --agent reader --scope read --ttl 7d --out-file ./reader.key --json
xavier keys list --json
xavier keys list --name existing-vault-key --json
```

Durations are positive integers with `s`, `m`, `h`, `d`, or `w` suffixes;
a bare integer means seconds. Scopes follow the server's existing policy:
`read` permits read operations and search; `write` permits mutations. `write`
does not implicitly grant `read`. Neither scope grants token administration or
other root-only operations. These scoped tokens authenticate REST requests;
the separate MCP transport currently requires a root token or supported JWT.

`--out-file` creates a new file with Unix permissions `0600` and never replaces
an existing file or follows an existing symlink. Its parent directory must
already exist. The file contains the secret once, followed by a newline.
JSON output contains only `schema_version`, `status`, `metadata` (`id`, `agent`,
`scope`, `expires_at`), and `out_file`. `--json` requires `--out-file` so stdout
remains a single metadata document. Without a file, token output requires
non-terminal stdout and `--i-understand-output-is-not-a-tty`; metadata goes to
stderr:

```sh
(umask 077; xavier token issue --agent reader --scope read --ttl 1h --i-understand-output-is-not-a-tty > reader.key)
```

Without `--name`, `keys list` lists all persistent scoped REST token metadata
from the configured server. It never includes token values or hashes. With
`--name`, it preserves the existing local vault metadata lookup; vault entries
are separate from server tokens and are not included in the server listing.

These commands never prompt. Exit codes are `0` for success, `1` for operation
failure, `2` for invalid usage, `3` for authentication/authorization failure
(HTTP 401/403), and `4` for an unreachable server. JSON operation errors use
`schema_version: 1` and `error` (`code`, `message`). Parser errors remain on
stderr. HTTP response bodies are not echoed in error messages.
