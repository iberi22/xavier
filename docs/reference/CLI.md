# CLI reference

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
