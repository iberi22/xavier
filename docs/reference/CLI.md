# Xavier CLI reference

## Code-graph sidecar

```sh
xavier code install --json
xavier code install --from-source --json
xavier doctor --format json
```

`code install` runs locally without a daemon or interactive prompts. It builds
from local source when `code-graph/Cargo.toml` is present or `--from-source` is
specified; otherwise it downloads the precompiled sidecar and its `.sha256`
checksum. A failed download, missing or invalid checksum, or checksum mismatch
aborts installation. Existing installed files are preserved on failure.

Before installation, the binary must be a regular executable file, have an ELF
header on Linux, and return success from `--version` within three seconds.
Installation stages a private temporary file in the destination directory and
atomically renames it to `~/.local/bin/codegraph` with mode `0755`.

Resolution checks the configured override, workspace targets, user installation
directories, and PATH. It skips files that fail the regular-file, executable,
or Linux ELF checks, with one warning per invalid path per process. Resolution
does not execute candidates. Doctor reports invalid candidates as a warning
with a hint to move those files aside; it does not modify them.

`--json` writes one JSON object to stdout; diagnostics and build output go to
stderr. The fields are `status` (`ok` or `error`), `installed_path` (string or
null), `chosen_binary` (string or null), `version` (string), `verified` (boolean),
`message` (string), and `invalid_candidates` (an array of objects with `path`
and `reason` strings). `chosen_binary` names the successfully installed binary,
or an existing usable candidate if installation fails. Installation exits
`0` on success and `1` on failure, including download and verification failures.

## Recovery status and seal safety

`xavier recovery status` reports the local recovery directory, key source,
resolved key path, key fingerprint, KCV, seals, and recovery gaps. It never creates
a record key or contacts the daemon. Use the daemon's data directory and key
source when inspecting its recovery material.

```sh
xavier recovery status --json
xavier recovery status --json --crypt-passphrase-backed-up
xavier recovery seal --mnemonic --words-out /secure/recovery-words.txt
xavier recovery seal --passphrase --passphrase-file /secure/passphrase.txt
```

Status JSON is a single object on stdout; logs go to stderr. Schema version 1 has
these fields:

| Field | Meaning |
| --- | --- |
| `schema_version` | Integer `1`. |
| `recovery_dir` | Local recovery directory. |
| `key_source` | Object with `kind`: `env`, `file`, `missing`, or `unavailable`, and `path`: resolved file path or `null` for an environment key. |
| `key_path` | Resolved file path, matching `key_source.path`. |
| `key_fingerprint` | First 12 hex characters of SHA-256 over `xavier-record-key-fingerprint-v1` followed by a NUL byte and the 32-byte key; `null` if unavailable. |
| `manifest` | `present` and `valid` booleans; validity checks the supported format and record KCV syntax. |
| `kcv` | `present`, `valid`, and `state`. A valid KCV matches the local key, or is structurally valid when the key is missing. |
| `mnemonic_seal`, `passphrase_seal` | `present`, `valid`, `state` (`absent`, `malformed`, `valid`), and `mode_0600` (boolean or `null`). |
| `seals` | Array of the same seal health objects with `kind`: `mnemonic` or `passphrase`. |
| `crypt_passphrase_backed_up` | Operator assertion, false unless the corresponding flag is supplied. |
| `recoverable` | Existing recovery policy: a structurally valid seal, a usable matching KCV, and the crypt passphrase backup assertion. |
| `warnings` | Array of recovery gaps and validity/permission notes. |

Seal validity is structural: verify the actual words or passphrase with `unseal`.
A record-key seal does not back up the master key or the default-space keystore
used by XDK2 records.

Before sealing, Xavier prints the selected key's resolved path (or environment
source) and fingerprint to stderr. It queries the authenticated, read-only daemon
endpoint `GET /admin/recovery/key`, which returns only `key_fingerprint` and
`key_path`. The daemon resolves these using its own environment without creating
a key. The CLI uses the normal `XAVIER_URL` and token resolution. A mismatch
refuses sealing before generating words or writing recovery files.

If the daemon cannot be reached, verify the selected key independently before
using the explicit offline acknowledgement:

```sh
xavier recovery seal --mnemonic --words-out /secure/recovery-words.txt \
  --offline-i-know-the-key-is-live
```

The offline flag never bypasses a reachable daemon's mismatch, authentication
failure, unsupported endpoint, or malformed response. Existing KCV checks and
explicit `--rekey`, `--replace-mnemonic`, and `--replace-passphrase` flows remain
available. `--rekey` explicitly permits a changed KCV and skips daemon comparison.

With redirected stdin, each selected seal path needs its file flag:
`--mnemonic` requires `--words-out`, and `--passphrase` requires
`--passphrase-file`. No prompt is opened. Keys, passphrases and recovery words
are never accepted as argv values or printed by these commands.

Exit codes: `0` success; `1` failed operation or key mismatch; `2` missing
non-interactive file flags or invalid CLI syntax; `3` daemon HTTP 401/403;
`4` unreachable daemon without the offline acknowledgement.

## `xavier config`

Inspect the current process's effective settings without contacting the daemon,
loading a vault, seeding files, or starting the service. Keys use the dotted paths
of `XavierSettings`. Precedence is compiled defaults, config file, runtime state,
then the environment inherited by the CLI. The command cannot inspect environment
variables available only to another process or to systemd.

```sh
xavier config list --json
xavier config get memory.data_dir --json
xavier config explain models.embedding_model --json
xavier config path --json
xavier config set server.port 8200 --json
xavier config set tgd.enabled false
xavier config set models.local_model_dirs '["/srv/models"]'
xavier config unset server.port --json
xavier config validate --json
xavier config set license.commercial_key --value-file /run/license
# Or pipe the value from a secret manager:
secret-manager read license | xavier config set license.commercial_key --value-stdin
```

`list`, `get KEY`, and `explain KEY` include the effective `value`, `source`
(`default`, `config-file`, `runtime-state`, or `env`), and `env_var` (the active
variable name or the documented override variable, or null). `explain` also shows
both file paths and `restart_required: true`. `path` shows the same config and
runtime-state paths used by the settings loader and `save()`.

`set KEY VALUE` validates strings, booleans, integers (including their bounds),
floats, enums, and JSON lists against the settings deserializer. Strings are literal;
use JSON array syntax for lists. Unknown keys and invalid types exit 2. `unset KEY`
removes just that runtime override, revealing the config/default value unless an
environment override is present. The versioned config file is read-only. Writes
use an atomic replacement with mode 0600 on Unix. Restart the daemon after changes;
`set` and `unset` return `restart_required: true` and emit the hint on stderr.
An environment variable still takes precedence over a newly written runtime value.

Sensitive fields display only `set` or `unset`, including credentials nested in
lists. Sensitive input requires `--value-file PATH` or `--value-stdin`, never a
positional value. Trailing CR/LF characters are stripped from file/stdin input.
Vault-managed credentials (API keys, tokens, passwords and database credentials)
remain excluded from runtime state by the existing loader: setters reject them
with a vault/environment hint. No command prints their values. The commercial
license key and master-key name can be written using file/stdin input.

`validate` reports malformed JSON, unknown file keys, type errors, invalid local
model configuration, and relative paths that depend on the current directory.
It exits 1 if any problem is found, including relative paths in defaults. Reports
contain key names and diagnostics, never rejected values. Config files currently
use JSON, including paths ending in `.toml`, matching the existing loader.

Every operation accepts `--json` before or after its subcommand. Stdout contains
one JSON object with `schema_version: 1` and `ok`; diagnostics go to stderr.
Successful reports include `operation`, `config_path`, `runtime_state_path`, and
`restart_required`. Reads add `entry` or `entries`; validation adds `problems`.
Operational/usage errors use `{"schema_version":1,"ok":false,"error":{"code":2,
"message":"..."}}` (code 1 for I/O/configuration failures). Argument parsing errors
follow clap's standard stderr diagnostics. Exit codes: 0 success, 1 operation or
validation failure, 2 invalid usage. These local commands perform no authentication
or network requests and never prompt.

## LLM configuration and unattended setup

`xavier llm` configures and probes the general language model or a separate model
for explanations. Every subcommand accepts `--json` and `--scope general|explain`
(default: `general`). JSON output is one document on stdout; diagnostics and
restart hints go to stderr. Responses include `schema_version: 1` and `ok`.

```sh
xavier llm status --json
xavier llm set --provider local --model llama3.2 --url http://127.0.0.1:11434/v1 --json
xavier llm set --provider openai --model gpt-4.1-nano --api-key-file /run/secrets/openai-key --json
xavier llm set --provider anthropic --model claude-haiku-4-5 --scope explain --api-key-stdin --json < /run/secrets/explain-key
xavier llm list-models --json
xavier llm test --scope explain --json
xavier setup --local --model llama3.2 --yes --json
```

`status` reports the effective provider, model, base URL, each setting's source,
and API key presence (`set` or `unset`, never its value). Sources are `default`,
`config`, `runtime`, `env:NAME`, `vault`, or `unset`. The local Ollama placeholder
is not a configured key. `daemon.reachable` and `daemon.llm_configured` describe
`/health` at the configured Xavier URL; `llm_configured` is null when unavailable.
The daemon health field describes its general LLM, including for explain scope.
CLI configuration and the running daemon can differ until the daemon restarts.

`set` requires `--provider local|openai|openrouter|groq|anthropic` and a nonempty
`--model`. `--url` supplies an HTTP(S) provider API root, normally ending in `/v1`
(or `/openai/v1` for Groq). Credentials, query strings, and fragments are refused
in URLs. Settings go through `XavierSettings::save()` to the runtime state file
(`XAVIER_RUNTIME_STATE_PATH` overrides its location), never the config file.
Restart the daemon after changes. Existing environment variables take precedence
and must be updated or removed in the daemon environment to use saved settings.
Explain settings are independent; until an explain provider is selected,
explanations inherit the general provider.

API keys are accepted only through mutually exclusive `--api-key-file PATH` or
`--api-key-stdin`. They are stored in the existing encrypted provider vault,
never runtime JSON. A vault failure aborts with a hint naming the provider's key
environment variable. Explain keys use a separate vault entry. The existing
`XAVIER_EXPLAIN_API_KEY_FILE` override remains supported. No key argument is
accepted on the command line.

`list-models` returns `models`, an array of IDs from the provider's `/models`
endpoint. Local servers must expose the OpenAI-compatible API (Ollama supports
`/v1/models`). `test` first lists models, then submits a one-token completion
(`/chat/completions` or Anthropic `/messages`). Completion text and remote error
bodies are never printed. `--timeout-secs` bounds each HTTP request (default 10,
range 1–120). These commands contact the provider directly, not the Xavier daemon.

Unattended `setup --local --model ID` writes local runtime settings without
prompts, model downloads, shell changes, or `.env` edits. It is selected by
`--yes`, `--json`, or non-terminal stdin. Missing `--local` or `--model` exits 2
with a usage hint. Install models separately and use `llm test` to check them.
The existing interactive wizard remains available with terminal stdin.

Exit codes: 0 success; 1 operation/protocol failure; 2 invalid or missing input;
3 authentication failure (HTTP 401/403); 4 provider connection failure or timeout.
For JSON operation failures, stdout contains `ok: false` and a safe `error`
message. Argument parsing errors use stderr and exit 2.

## Shared daemon configuration

The CLI reads `XAVIER_ENV_FILE`, or discovers `~/.config/xavier/env`
(`$XDG_CONFIG_HOME/xavier/env` when set), before loading settings. Point both
`xavier http` and CLI invocations at the same contract. For a systemd deployment,
use that file as the service's `EnvironmentFile`; copy any relevant `XAVIER_*`
overrides from drop-ins into the shared file. Xavier does not inspect systemd units.

```sh
XAVIER_ENV_FILE=/etc/xavier/env xavier doctor --json
xavier doctor --format json
```

The contract accepts one `KEY=VALUE` per line, blank lines, full-line `#` comments,
and optional matching single or double quotes around values. Only `XAVIER_*`
keys are loaded. Existing process variables win. An explicitly selected missing or
malformed contract fails startup with exit 2; an absent discovered file is allowed.
Values are literal: no shell
execution, interpolation, variable expansion, or inline comments. Keep credentials
in a protected file; contract values are never printed by the loader.

Precedence: process environment > env contract > runtime state > config defaults
> compiled defaults. To share config defaults as well as environment overrides,
set absolute `XAVIER_CONFIG_PATH` and `XAVIER_RUNTIME_STATE_PATH` values in the
contract. Restart processes after editing the contract; it is not hot-reloaded.

Relative `memory.data_dir` / `XAVIER_DATA_DIR` is relative to the user's home.
Other relative storage paths are relative to the effective data directory, with
an initial `data/` removed for compatibility with existing defaults. For example,
with data directory `/srv/xavier`, both `data/code_graph.db` and `code_graph.db`
resolve to `/srv/xavier/code_graph.db`. Absolute paths remain absolute and empty
optional paths remain empty. This rule also covers memory stores, workspace
storage, embedding cache, enterprise storage, router policy, panel storage, and
Data Commons. No path in this group depends on the invocation directory. Existing
files are not moved; use absolute storage paths to retain a previous location.

## Doctor

```sh
xavier doctor
xavier doctor --json
XAVIER_ENV_FILE=/etc/xavier/env xavier doctor --json
```

Doctor's JSON object has `checks` and `overall`. Each check has `name`, `status`,
`detail`, `hint`, and `timed_out`. The `CLI/daemon config agreement` check compares
resolved data and code-graph database paths against the authenticated
`GET /v1/system/health` response's `config_paths` object (`data_dir`,
`code_graph_db_path`). The record-key file follows the data directory at
`node/record.key`; its contents are never requested.

Agreement is `Ok`; disagreement is `Fail` with a configuration hint. An unreachable
daemon is `Skip`, which does not make the overall report fail. Authentication
failure and unsupported/invalid daemon responses are reported as failures.
Doctor exits 0 for an overall `Ok`/`Warn`, 1 for failed checks, and 3 when the
agreement probe is rejected with HTTP 401/403. JSON is written only to stdout;
logs and setup errors go to stderr. No interactive input is required.

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

## Memory

`xavier memory fts-check` checks the `memory_fts` index for corruption. It exits `0` on success, `1` on corruption, and `2` on operational errors.

`xavier memory fts-rebuild` rebuilds the corrupted `memory_fts` index. It exits `0` on success, and `2` on missing DB or operational errors.
Both commands output JSON if `--json` is provided.
