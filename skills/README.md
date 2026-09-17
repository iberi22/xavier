# Xavier — Skills personalizados del proyecto

Skills operativos mantenidos junto al código de Xavier. Son la **fuente canónica**:
los demás harnesses los consumen por symlink o copia.

## Convención de instalación

| Harness | Cómo consume | Ruta |
|---|---|---|
| Xavier (repo) | fuente canónica | `apps/xavier/skills/<name>/SKILL.md` |
| Hermes | copia | `~/.hermes/skills/<name>/` |
| Codex CLI | symlink | `~/.codex/skills/<name>` → repo |
| opencode CLI | symlink | `~/.config/opencode/skills/<name>` → repo |
| OpenClaw | copia | `~/.openclaw/skills/<name>/` |

**Regla:** editar SIEMPRE en el repo y re-sincronizar. Nunca editar la copia.

```bash
# Re-sincronizar tras editar un skill en el repo
NAME=maloca-hub
REPO="${XAVIER_REPO_DIR:-$(pwd)}"
cp "$REPO/skills/$NAME/SKILL.md" ~/.hermes/skills/$NAME/SKILL.md
cp "$REPO/skills/$NAME/SKILL.md" ~/.openclaw/skills/$NAME/SKILL.md
rm -f ~/.codex/skills/$NAME && ln -sfn "$REPO/skills/$NAME" ~/.codex/skills/$NAME
rm -f ~/.config/opencode/skills/$NAME && ln -sfn "$REPO/skills/$NAME" ~/.config/opencode/skills/$NAME
```

## Reglas de contenido

- **Sin secretos quemados.** Nunca pegar tokens, API keys, JWT ni valores de `.env` en un
  SKILL.md, ni siquiera redactados parcialmente. Referenciar la *fuente* de la credencial
  (`~/bin/get-xavier-token`, `apps/xavier/.env`) sin imprimir su contenido.
- **Sin comandos destructivos.** Los skills de inspección nunca deben ejecutar
  deploys, `systemctl restart`, migraciones ni escrituras de estado.
- **Distinguir diseño vs. implementación vs. uso real.** Un HTTP 200 no acredita que un
  módulo tenga datos ni que esté en uso.
- **Rutas absolutas** son aceptables (el repo vive en un path estable del host canónico);
  los *valores de configuración* no se queman: se leen de su fuente.

## Índice

### Maloca

| Skill | Propósito |
|---|---|
| `maloca-hub` | Hub SWAL: 6 módulos, 7 principios, capas, endpoints canónicos |
| `maloca-backlog-ops` | Backlog unificado, features.json v2, waves, gaps #2/#4 |
| `maloca-xavier-integration` | Módulo Maloca de Xavier: rutas, WS feed, tests, health |
| `maloca-atlas-tasks` | Atlas Core ↔ Xavier ↔ Maloca, event bus, swal-node |
| `maloca-security` | Bind 0.0.0.0, token Admin plano, guard del túnel, JWT readonly |
| `maloca-cloudflare` | Zonas, Pages, túnel efímero, mitigación KV creep |

### Xavier core

| Skill | Propósito |
|---|---|
| `xavier-cognitive-memory` | Memoria cognitiva vía CLI/HTTP/MCP + Code-Graph |
| `xavier-code-graph-analysis` | Navegación AST, símbolos, call hierarchy, blast-radius |
| `xavier-maintenance-hygiene` | Higiene de repo, purga de cachés, migraciones |
| `xavier-rtk-execution` | Proxy rtk-kernel para comprimir salida de CLI |
| `xavier-wave-verification` | Pipeline GitCore 3.8 de verificación de features |

## Verificación

```bash
# Todos los skills presentes en el repo
ls /home/belal/proyectosSWAL/apps/xavier/skills/

# Consistencia repo ↔ copias
for s in maloca-hub maloca-backlog-ops maloca-xavier-integration \
         maloca-atlas-tasks maloca-security maloca-cloudflare; do
  diff -q "/home/belal/proyectosSWAL/apps/xavier/skills/$s/SKILL.md" \
          "/home/belal/.hermes/skills/$s/SKILL.md" && echo "✓ $s"
done

# Sin secretos
grep -rlE "XAVIER_TOKEN=[A-Za-z0-9]{8}|sk-[A-Za-z0-9]{20}|cfoat_|cfut_" \
  /home/belal/proyectosSWAL/apps/xavier/skills/*/SKILL.md || echo "✓ limpio"
```
