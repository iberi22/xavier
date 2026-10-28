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
