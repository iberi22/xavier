# Xavier CLI

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
