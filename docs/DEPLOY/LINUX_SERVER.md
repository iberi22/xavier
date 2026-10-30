# Xavier on a Linux server (headless)

The server build is the `xavier` binary compiled with `--no-default-features --features headless`.
It serves HTTP, MCP and the CLI. It has no TUI and no panel routes, and it needs no display or D-Bus.

## Choose a channel

| Channel | Command | Installs |
|---|---|---|
| Installer | `curl -fsSL https://raw.githubusercontent.com/iberi22/xavier/main/packaging/linux/install.sh \| sh` | Binary, `xavier.service` and a token |
| Debian/Ubuntu | `apt install ./xavier-server_<ver>_<arch>.deb` | `/usr/bin/xavier`, unit and token (not started) |
| Fedora/RHEL | `dnf install ./xavier-server-<ver>.<arch>.rpm` | The same as the `.deb` |
| Docker | `docker run -d --hostname xavier -v xavier-data:/data -e XAVIER_TOKEN=… -p 127.0.0.1:8006:8006 ghcr.io/iberi22/xavier:server-latest` | A container whose state lives in `/data` |
| Nix | `nix run github:iberi22/xavier#xavier-server -- http --no-ui` | A package; for a service, use the NixOS module `services.xavier` |
| Tarball | `xavier-server-<ver>-<target>.tar.gz` from the release page | Run `install.sh --archive <file>` from inside it |

Release targets are `x86_64-unknown-linux-gnu` and `aarch64-unknown-linux-gnu`. Both are built on Ubuntu 22.04 and need glibc 2.34 or newer (Debian 12, Ubuntu 22.04, RHEL/Alma 9 and later). A static musl build is not available yet: `sqlite-vec` does not compile against musl.

## install.sh

```sh
sh install.sh                       # latest release; system service when run as root
sh install.sh --version v0.3.0      # pin a release
sh install.sh --user                # ~/.local/bin + systemd --user unit, no root
sh install.sh --archive xavier-server-….tar.gz   # offline; needs the .sha256 next to it
sh install.sh --no-start            # install without enabling the service
```

The installer always verifies the archive against its `.sha256` and aborts on a mismatch. It never overwrites an existing env file, so a token survives reinstalls. It also leaves alone a `xavier.service` that it did not create, unless you pass `--force`.

## Layout (system install)

| Path | Content |
|---|---|
| `/usr/local/bin/xavier` (`/usr/bin/xavier` from packages) | The binary |
| `/etc/xavier/xavier.env` (0640, root:xavier) | `XAVIER_TOKEN`, bind address and embeddings |
| `/var/lib/xavier` | `HOME` of the `xavier` user. Holds `.xavier/master.key` and `data/` |
| `xavier.service` | `http --no-ui`, user `xavier`, `ProtectSystem=strict`, writes only `/var/lib/xavier` |

```sh
systemctl enable --now xavier
curl -fsS http://127.0.0.1:8006/ready
```

## Keys and backups

`master.key` lives in `$HOME/.xavier/`, which on a system install is `/var/lib/xavier/.xavier/`.

Without a desktop keyring, the key file is wrapped with a key derived from the host name and the machine id. Moving the data to another host, or changing the host name, makes the key unreadable. Xavier then refuses to start against the existing data. Back up `/var/lib/xavier` as a whole, and use `xavier recovery` to move a node.

In Docker, keep both the key and the data in the `/data` volume, and set a fixed `--hostname`. The image already sets `HOME=/data/home` and `XAVIER_DATA_DIR=/data/data`.

## Embeddings

The server runs without a local model. Point `XAVIER_EMBEDDING_URL` at an Ollama or OpenAI-compatible endpoint; `docs/LOCAL_EMBEDDINGS.md` lists the providers.

## Verification

The `Server Packages` workflow (`.github/workflows/server-packages.yml`) checks every channel:
- **Binary:** builds the binary for each target and fails if the TUI or Tauri crates are linked in. It starts the server and checks `/health` and `/ready`, then stops it with SIGTERM.
- **Packages and installer:** installs the `.deb` on Debian, the `.rpm` on Fedora, and runs `install.sh` on Debian and Ubuntu.
- **Docker:** recreates the container on the same volume and checks that the same master key loads.
- **Nix:** builds the Nix package and evaluates the NixOS module.

## Headless mode limits

In headless mode, the server supports a limited set of tools and routes.

**Supported Tools:**
- `code_scan`
- `code_find`
- `code_stats`
- `code_context`

Unsupported tools return a JSON 501 contract like:
```json
{
  "error": "Tool 'xyz' is not implemented in headless mode",
  "code": 501,
  "reason": "unsupported_in_headless",
  "supported_tools": ["code_scan", "code_find", "code_stats", "code_context"]
}
```

**Unsupported Routes:**
Agent management routes are not supported in headless mode. These include:
- `GET /v1/agents`
- `POST /v1/agents/spawn`
Calling these routes will return a 501 Not Implemented response with the reason `unsupported_in_headless` and a hint to `run the full daemon (xavier serve)`.
