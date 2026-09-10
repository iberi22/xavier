# Local Deploy — Xavier (WAVE-20.01)

Operational runbook for rebuilding and redeploying the local Xavier service
binary on this machine. All commands run on the host (NixOS, systemd user service).

## Why this exists

The live binary (`~/.local/bin/xavier-real`) was built WITHOUT the
`local-gllm` cargo feature, so with `XAVIER_EMBEDDER=local` the daemon logs
`built without the local-gllm feature` in a loop and never produces embeddings.
Fix: rebuild WITH the feature and restart. See iberi22/xavier#2020.

## Canonical build (AGENTS.md)

```bash
cargo build --release --features local-gllm   # host deploy
cargo build --release --features ci-safe      # CI only, NOT for host deploy
```

Never `rm -rf target/` (warm cache, metered connection).

## Deploy (automated)

```bash
bash scripts/deploy-local-xavier.sh
```

What it does: backups binary + systemd drop-in, incremental release build,
install to `~/.local/bin/xavier-real`, `systemctl --user restart xavier.service`,
health gate (`GET /health` 200 in <2s, 90s budget), automatic rollback on gate failure.
Logs: `/tmp/xavier-deploy-<stamp>.log`.

## Rollback (manual)

```bash
cp ~/.local/bin/xavier-real.bak-<stamp> ~/.local/bin/xavier-real
cp ~/.config/systemd/user/xavier.service.d/zzz-embeddings-cloud.conf.bak-<stamp> \
   ~/.config/systemd/user/xavier.service.d/zzz-embeddings-cloud.conf
systemctl --user restart xavier.service
```

## Verify

```bash
timeout 5 curl -s -o /dev/null -w "%{http_code} %{time_total}\n" http://127.0.0.1:8006/health
curl -s http://127.0.0.1:8006/memory/stats | head -c 300
~/.local/bin/xavier recall "<unique-wave20-term>"
```

Plan B (if local backend still fails): switch drop-in to
`XAVIER_EMBEDDER=openai-compatible`, `XAVIER_EMBEDDING_URL=http://localhost:11434/v1`,
flavor openai, dims 768 (local Ollama, zero downloads), restart, re-verify.

## Evidence (WAVE-20.01, 2026-09-07)

- Build: `cargo build --release --features local-gllm` exit 0 in 20m39s (warm target/).
- Binary: 88MB with local-gllm (old one 214MB without it). Installed via stop-first (cp over running binary fails ETXTBSY — script handles it).
- Health: `200 OK` (~45-60s warm-up on fresh start, ~5.7s under load; <2s AC not met under daemon load — follow-up in #2022/20.03).
- Stats: `{"status":"ok","version":"0.1.1","workspace_id":"default"}`, no pool errors.
- Roundtrip: add `hermes/wave20/smoke-20260907` → status ok → embedding len=1552 → search `zephyr-qt-77` returns it with real vector.
- gllm errors in log since deploy: 0 (was infinite loop before).
- Memory: service RSS ~3.5-4.3G, still high — daemon bounds pending (#2022, wave 21).
- Lesson: new binary ENFORCES XAVIER_TOKEN (old one accepted empty); use token from apps/xavier/.env.
