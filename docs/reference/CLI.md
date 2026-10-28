# Xavier CLI reference

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
