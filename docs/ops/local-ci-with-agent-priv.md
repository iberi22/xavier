# Local CI when GitHub Actions budget is exhausted

Xavier CI on GitHub Actions may fail immediately when the org has no remaining
Actions minutes. Prefer **local CI** for merge confidence, then merge with
diff review if remote checks stay red.

## Tools

| Tool | Role |
|------|------|
| `swal-ci-container` Hermes skill | Runs the CI matrix in Docker |
| `agent-privilege-notify` Hermes skill | Elevates allowlisted commands under `NoNewPrivs` via `systemd --user` |

## Why agents cannot `sudo` directly

Cursor/sandbox agent processes often have **`NoNewPrivs=1`**. `sudo`/`pkexec` in
that process fail. The privilege skill writes a request, runs a user systemd unit
with `NoNewPrivs=0`, shows an Accept/Deny dialog, then runs allowlisted
`/run/wrappers/bin/sudo -n …`.

## Runbook

```bash
# 1. Diagnose privilege + display
bash ~/.hermes/skills/devops/agent-privilege-notify/scripts/agent-priv.sh doctor

# 2. Ensure Docker/dockerd (NixOS: see docs/ops/nixos-docker.md)
bash ~/.hermes/skills/devops/agent-privilege-notify/scripts/agent-priv.sh request \
  --title "Start Docker (SWAL CI)" \
  --body "Xavier local CI needs Docker" \
  --cmd "systemctl start docker" \
  --timeout 180

# 3. Run CI against this repo
bash ~/.hermes/skills/swal-ci-container/scripts/swal-ci.sh run ~/proyectosSWAL/xavier
```

Host-native fallback (no container) can still run:

```bash
CARGO_TARGET_DIR=target_local cargo check -p xavier
CARGO_TARGET_DIR=target_local cargo test -p xavier --lib
```

## Safety

- Only extend the agent-priv **allowlist** deliberately — each entry is privilege surface
- Never commit tokens; do not paste `XAVIER_TOKEN` into issues/PRs
- Treat toast clicks as dismiss-only; Accept happens in the privilege dialog (rofi)
