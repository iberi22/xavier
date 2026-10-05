# Xavier — Plan de puesta a punto (2026-10-05)

Owner: Hermes, a partir del estado real verificado. Sustituye a los checkpoints
`projects/xavier/handoff/2026-10-02-sprint` y `audit/xavier-estado-cierre-2026-10-04`
(recuperados de memoria de sesiones anteriores: b39bdcea, 8811ce50, orquestador 2026-10-04).

## 0. Estado real (verificado, no recordado)

| Hecho | Valor |
|---|---|
| `main` == `origin/main` | `2c6dcc76` |
| Worktree principal | rama `skills/sync-drift-2026-10-04` @ `a19eb338` (18 ahead / 32 behind main — rama muerta, su contenido ya está en main) |
| Daemon | `xavier 0.2.17` vivo en `:8006`, `GET /health` = 200 |
| Health | `mode=local-healthy`, `status=degraded`, causa única: `host:disk` (86.3%) |
| Disco | 907G total, 737G usado, 124G libre; `/tmp` = 80G |
| PRs | #2820 y #2822 MERGED. 1 PR real abierto: #2824 (codegraph por repo) |
| `sprint/xavier-2026-10` | **82 commits sin integrar en main** |
| Ramas WIP | `sprint/aux-encryption` @704ac922, `sprint/test-hygiene` @ed30cdb0 — ambas SIN verificar |

Lo que YA se corrigió y está en main (ya no es trabajo pendiente): el gate de alcance
`scripts/check-staged-scope.sh` + su enganche en `.husky/pre-commit` (líneas 172-173),
el fix de dos señales falsas en `health_check`, y el diseño de BDs por proyecto
(`docs/design/xav-repo-multidb-design.md`).

## 1. El hueco real

`main` **no tiene `src/spacios`**. Todo el núcleo de Espacios vive solo en la rama sprint:
llaves por espacio, almacenes de memoria por espacio, tokens `xsp_`, enlaces de un sentido,
cifrado fail-closed, privacidad de Maloca, mx-hygiene y la regla `AGENTS.md` de que los
agentes remotos nunca compilan Xavier. El daemon en producción corre sin nada de eso.

Dos ramas WIP quedaron a medias porque se agotó el cupo del agente anterior. Los mensajes
de commit lo dicen literalmente: "SIN verificar ni revisar".

## 2. Fases

### F0 — Plan y goal (este documento)  ✅
Objetivo: dejar por escrito el estado real y el orden, para que otro agente pueda
continuar aunque se corte la energía.

### F1 — Reviews de las 2 ramas WIP (2 agentes en paralelo, sólo lectura)
- Agente A: review criptográfico de `sprint/aux-encryption` (cifra conversaciones,
  creencias, notificaciones, snapshot de memoria de trabajo y bus de eventos).
- Agente B: review de higiene de tests de `sprint/test-hygiene` (tests que escribían en
  el almacén real `~/.xavier`, 200+ artefactos hubo que ponirlos en cuarentena).
Ninguno integra nada. Salida: informe con veredicto MERGE / MERGE-AFTER-FIXES / REJECT.

### F2 — Integrar el sprint (82 commits)
Worktree aislada: `~/wt/xav-int-1005`, rama `integrate/espacios-1005`, base `origin/main`.
Nunca se toca el árbol principal (regla: con subagentes escribiendo, `git pull --rebase`
siempre falla).

Conflictos: exactamente 2, ya resueltos a mano:
- `src/storage/multi_db.rs` — dos resolvers de ruta incompatibles (`db_path` de main
  trataba `root` como data dir y hacía `root/db/`, `path_for` del sprint lo trata como
  dir de BD). Se conserva `path_for` y se elimina `db_path`: con los dos, `create` y
  `delete` habrían resuelto ficheros distintos en producción (`data/db/db/*.sqlite`).
  Se conservan **las dos** suites de tests (higiene de main + registro/trayectoria del sprint).
- `src/cli/http_setup.rs` — dos puertas de auth. Se **unifican por la unión**, no por
  "gana una": el sprint protects con una lista de prefijos que cubría `/maloca/support`
  pero **no** el alias `/v1/maloca/support`, que sí está montado por `v1_maloca_router`.
  El alias se añadió a `MALOCA_PROTECTED_READ_PREFIXES`; la condición se simplificó a
  una sola tabla. Aquí la parte blanda habría sido un agujero de seguridad.

### F3 — Verificar
`cargo fmt --all -- --check`, `cargo check --lib --features ci-safe`,
`cargo test --package xavier --lib --features ci-safe -- --test-threads=1`.
Prohibido `rm -rf target` y `CARGO_TARGET_DIR/target-dir` (rompe el build-dir; así
`swal-agent-target` llegó a 63G). CI es el verificador de verdad.

### F4 — Integrar las 2 WIP (sólo tras los reviews de F1)
Con los veredictos encima. Cifrado de almacenes auxiliares es lo más delicado: entra con
revisión cripto explícita, no "a ojo".

### F5 — Ledger
`.gitcore/features.json` dice `feat-encryption-at-rest: stable`. Es falso: los almacenes
auxiliares siguen sin cifrar. Bajar a `beta`. El ledger miente y eso ya pasó dos veces
con `health_check`.

### F6 — Push a main
Sólo con OK explícito de Belal. Nada de deploy, ni rotación de llaves, ni borrar la
carpeta vieja de Drive.

## 3. Lo que necesita a Belal (no lo decido yo)

1. **Disco al 86%** es la única razón de que `/health` reporte `degraded`. `/tmp` ocupa 80G.
   Ya estaba en la mesa: build viejo de Xavier (45G) y `/tmp` (76→80G). No borro nada
   sin su OK.
2. **Push a main** de la integración de F2/F4.
3. Si `feat-encryption-at-rest` debe decir `beta` o `stable` — el código dice beta.

## 4. Reglas que no se rompen

- Nunca `rm -rf target`, nunca `CARGO_TARGET_DIR/target-dir`.
- Los tests usan tempdir; escribir en el repo o en `~/.xavier` es regresión (ya pasó dos veces).
- Nada de push a main, deploy, rotación de llaves ni borrado del backup viejo sin OK.
- CI verifica; los agentes remotos no compilan Xavier aquí (mató el 68% de las sesiones de Jules).
- Todo informe en inglés, en `.md`, con evidencia de comando y exit code.