# Xavier Docker Deployment

> Rewritten 2026-09-24 as part of GH #2545 (docs accuracy pass). The previous version of this
> file described a Windows path (`E:\scripts-python\xavier`) from an unrelated old setup, an
> obsolete `docker/` Dockerfile, and a "push to Docker Hub" plan that was never what CI
> actually does. This version reflects what is in the repo and in CI today.

## TL;DR

- **Build context / Dockerfile:** the repo-root [`Dockerfile`](../../Dockerfile) (multi-stage:
  `panel-ui` frontend build → Rust release build → minimal `debian:bookworm-slim` runtime,
  non-root `xavier` user). There is no other maintained Dockerfile for the main server — the
  old `docker/Dockerfile` (pinned to `rust:1.77-slim`, missing the `codegraph-types/`,
  `crates/`, `vendor/maloca-core/` path dependencies and the panel-ui build stage — it would
  not even compile) was removed; nothing in CI or any `docker-compose*.yml` referenced it.
- **Build time:** budget **~20 minutes** for a cold build (no BuildKit cache): the `panel-ui`
  pnpm install + Vite build is a few minutes, the Rust `cargo build --release` compiling the
  full dependency graph (tree-sitter grammars, `libsql`, `rusqlite` w/ bundled SQLite, etc.) is
  the bulk of it. Incremental rebuilds reuse `--mount=type=cache` layers for `cargo` registry +
  `target/` and are much faster unless `Cargo.lock` or most of `src/` changed.
- **Registry: GHCR, not Docker Hub.** The only image this project's CI publishes is
  `ghcr.io/iberi22/xavier`, pushed by the `docker-publish` job in
  [`.github/workflows/release.yml`](../../.github/workflows/release.yml) — **and only when a
  `v*` git tag is pushed** (`if: startsWith(github.ref, 'refs/tags/v')`). There is no
  `docker-build.yml` workflow (older docs referenced one that does not exist) and no Docker Hub
  publish step anywhere in this repo's CI. If you need to confirm whether a given version was
  actually published, check
  <https://github.com/iberi22/xavier/pkgs/container/xavier> directly — do not assume `latest`
  is fresh without checking; publication is tied to tagging a release, not to every merge to
  `main`. The pipeline is also young and has broken repeatedly very recently — see
  `CHANGELOG.md` entries for #2523/#2525/#2527/#2529/#2531 (all within 2026-09-23/24: Inno
  Setup flag, GHCR build-flag, builder MSRV, and `LICENSE` `COPY`/`include_str!` fixes) — so a
  given tag's `latest` may lag a fix by one release if you're debugging something that looks
  like a stale image.
- **Local dev/build:** always build locally (`docker build -t xavier:local .` or `docker
  compose up -d --build`) unless you have confirmed a matching tag was published to GHCR.

## Building locally

```bash
git clone https://github.com/iberi22/xavier.git
cd xavier
docker build -t xavier:local .          # ~20 min cold, faster with BuildKit cache
# or, with extra optional features:
docker build --build-arg FEATURES="local-gllm,cli-interactive,panel-ui" -t xavier:local .
```

`scripts/build-docker.sh` wraps the same build with `DOCKER_BUILDKIT=1` and `--platform
linux/amd64`; it builds from the repo root now too (it used to point at the removed
`docker/Dockerfile`).

## Running

```bash
docker run -d --name xavier \
  -p 8006:8006 \
  -e XAVIER_TOKEN=your-secure-token \
  -e XAVIER_JWT_SECRET=$(openssl rand -hex 32) \
  -e XAVIER_STATE_DIR=/data \
  -v xavier-state:/data \
  xavier:local
```

`XAVIER_STATE_DIR=/data` matters: the image runs as the non-root `xavier` user (see
`Dockerfile`), and `/data` is the directory it creates and `chown`s for state — set it
explicitly and mount a volume there, or `.xavier/auth.db` and everything else lives only in
the container's ephemeral home directory and is lost on `docker rm`.

Health check: `curl -fsS http://localhost:8006/ready` (the `HEALTHCHECK` in the image checks
`/ready`, not `/health` — `/health` returns `200` even when the store isn't ready yet).

## docker-compose

The repo-root [`docker-compose.yml`](../../docker-compose.yml) and
[`docker-compose.dev.yml`](../../docker-compose.dev.yml) both build from the root `Dockerfile`
(`context: .`) and are the maintained entry points:

```bash
cp .env.example .env   # set XAVIER_TOKEN
docker compose up -d
```

`docker/` also holds compose variants for local Ollama (`docker-compose.local.yml`),
embeddings (`docker-compose.embeddings.yml`), benchmarks
(`docker-compose.benchmarks.yml`), and an enterprise/Cortex profile
(`docker-compose.cortex-enterprise.yml`) — all of those also build from the repo-root
`Dockerfile` (`context: ..`, `dockerfile: Dockerfile`), not a Dockerfile inside `docker/`.

## Pulling the published image (once a version tag has actually been released)

```bash
docker pull ghcr.io/iberi22/xavier:latest    # or a specific vX.Y.Z tag
docker run -d -p 8006:8006 -e XAVIER_TOKEN=... -e XAVIER_STATE_DIR=/data -v xavier-state:/data \
  ghcr.io/iberi22/xavier:latest
```

See [`DEPLOYMENT.md`](./DEPLOYMENT.md) for the full environment-variable reference, systemd
service, and upgrade procedure.
