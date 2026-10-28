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
