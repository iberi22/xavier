# Xavier — Deployment Docs Index

> This file used to contain a generic "Architecture Decision Records" template unrelated to
> deployment (a stale copy — Xavier's real ADRs live in [`docs/adr/`](../adr/) and already
> follow that format). Replaced with an actual index for this folder as part of the GH #2545
> docs-accuracy pass.

| Doc | Covers |
|---|---|
| [`DEPLOYMENT.md`](./DEPLOYMENT.md) | Full deployment guide: required env vars, Docker run/compose, Linux systemd service, Windows Scheduled Task, health checks, CI/CD pipeline overview, upgrade procedure. |
| [`DOCKER_DEPLOY.md`](./DOCKER_DEPLOY.md) | Docker-specific detail: which Dockerfile is authoritative, real build time (~20 min cold), the GHCR-only / tag-gated publish story, and how `docker-compose*.yml` (root and `docker/`) wire together. |
| [`SERVICE.md`](./SERVICE.md) | systemd service unit reference. |
| [`CLOUD_RUN.md`](./CLOUD_RUN.md) | Google Cloud Run deployment notes. |

## Quick facts (see `DOCKER_DEPLOY.md` for detail)

- **Registry:** `ghcr.io/iberi22/xavier` only. No Docker Hub publish exists in this repo's CI.
- **Publish trigger:** pushing a `v*` git tag (`docker-publish` job in `release.yml`) — not
  every merge to `main`.
- **Dockerfile:** the repo-root [`Dockerfile`](../../Dockerfile). `docker/` holds
  docker-compose variants (Ollama, embeddings, benchmarks, enterprise) that all build from
  that same root Dockerfile, plus a few auxiliary Dockerfiles for other services — it does not
  hold a second maintained Dockerfile for the main `xavier` server.
