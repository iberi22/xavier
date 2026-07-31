# NixOS Docker without `docker.service`

On many NixOS desktops, `systemctl start docker` fails because the unit is not
defined until `virtualisation.docker.enable = true` is set in configuration.nix
and the system is rebuilt.

## Preferred: enable the NixOS module

```nix
{
  virtualisation.docker.enable = true;
  users.users.<you>.extraGroups = [ "docker" ];
}
```

Then `sudo nixos-rebuild switch` and use `systemctl start docker` / `docker info`.

## Alternative: dockerd helper (agent privilege path)

When an agent runs with `NoNewPrivs=1` (Cursor/sandbox) and cannot elevate in-process:

1. Use the Hermes skill `~/.hermes/skills/devops/agent-privilege-notify`
2. Prefer `/run/wrappers/bin/sudo` (setuid wrapper), never store `sudo` without setuid
3. The skill’s `start-dockerd.sh` can start `dockerd` and adjust the socket group when
   `docker.service` is absent

Request example:

```bash
bash ~/.hermes/skills/devops/agent-privilege-notify/scripts/agent-priv.sh doctor
bash ~/.hermes/skills/devops/agent-privilege-notify/scripts/agent-priv.sh request \
  --title "Start dockerd" \
  --body "Local CI needs the Docker daemon" \
  --cmd "dockerd-helper" \
  --timeout 180
```

Do not vendor the skill into this repo; keep ops skills under Hermes.

## Socket permissions

After a manual dockerd start, ensure the socket is usable by your user group
(often `docker` or `users`). Prefer the NixOS module so group membership is declarative.
