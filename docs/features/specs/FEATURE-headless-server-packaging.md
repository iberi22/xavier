# FEATURE-headless-server-packaging

## Overview
Xavier is a server core as well as a desktop app. This feature adds a headless build profile and the channels to install it on Linux servers, each verified by CI. A channel is verified by installing it in a clean environment and starting the server.

## Requirements Reference
- **REQ-087**: Headless server distribution
- Related: **REQ-010** (MCP & HTTP server)

## Scope
- **`headless` feature:** `self-manage` + `pageindex`; `default` includes it. Built with `--no-default-features --features headless`.
- **`packaging/linux/`:**
  - `install.sh` (system and `--user` modes, SHA-256 verification, idempotent token);
  - `setup-host.sh` (shared by deb/rpm/install.sh);
  - the system and user systemd units and `xavier.env.example`;
  - `make-tarball.sh` and `smoke-test.sh`.
- **`.deb` / `.rpm`:** `cargo-deb` / `cargo-generate-rpm` metadata named `xavier-server`, built from the CI binary (`--no-build`).
- **`Dockerfile.headless`:** `headless` features, configurable jobs, `HOME=/data/home`, `XAVIER_DATA_DIR=/data/data`, `VOLUME /data`.
- **`flake.nix`:** `packages.<system>.xavier-server`, `nixosModules.xavier` (`services.xavier`), and a `module-eval` check that does not build the package.
- **`.github/workflows/server-packages.yml`:** runs on PRs that touch these paths, and from `release.yml` on tags. On tags it publishes to GHCR.

## Verification
- **Build matrix:** gnu x86_64 and aarch64 on Ubuntu 22.04 (glibc 2.35), and an experimental musl x86_64. For each target, CI checks the dependency graph, builds the binary and runs `smoke-test.sh`. The smoke test checks `/health`, `/ready`, no `/panel`, and exit within 30 s of SIGTERM.
- **Packages:** installs the `.deb` on `debian:bookworm` and the `.rpm` on `fedora:latest`. It checks the user, the env file mode and the token, then runs the smoke test.
- **install.sh:** runs on `debian:bookworm` and `ubuntu:24.04` covering install, reinstall (the token is kept), a tampered archive (refused) and the smoke test.
- **Docker:** starts, stops and recreates the container on the same volume and hostname. The master key KCV must match, and the container must not be killed with 137.
- **Nix:** `nix flake check --no-build`, the module evaluation and a full `nix build`, then the smoke test.
- **Unit tests:** `packaging_contract` runs the real `install.sh` against a throwaway archive, plus checks on the unit and the features.

## Out of Scope
- An explicit keystore mode for servers (`LoadCredential`/KMS) in place of the host-derived fallback. That is a separate feature.
- Multi-arch container images (only amd64 is published), Homebrew and winget.
- Splitting the crate (ADR-005).
