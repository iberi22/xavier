# Xavier Self-Managed Runtime — Análisis (2026-08-08)

> Propósito: que Xavier (motor de memoria SWAL) gestione su propio runtime y monitoree
> el ambiente, leyendo logs del sistema, creando tickets, y mejorando su health check
> y sus capacidades de forma autónoma — como un operador humano haría (caso real abajo).

## 1. Caso de estudio — diagnóstico real del 2026-08-08

Diagnóstico manual (Hermes) que Xavier debería poder hacer solo:

| Hallazgo | Evidencia | Impacto |
|---|---|---|
| Swap thrashing severo | zram 11.3/15.6G usado; si/so ~77MB/s; PSI io full 57%; wa 22-29% | Todo el sistema lento; comandos tardan |
| **xavier-real: 5.9GB viviendo en swap** (75% de su memoria) | VmSwap por proceso | Cada consulta a Xavier paga GBs de swap-in → la lentitud que siente el usuario es CAUSADA por el propio Xavier en swap |
| peerjs (shelf :9000): 100% de un core, 12h, 0 clientes | ps aux + ss | Busy loop quemando CPU (bug) |
| Gateway Hermes: polling Telegram MUERTO desde 04:09 | threads en S, CLOSE-WAIT, 0 conexiones ESTAB, logs mudos | Bot no responde mensajes |
| OpenClaw (node) con polling ACTIVO al mismo servicio | ss: 6 conexiones ESTAB a 149.154.x.x:443 | Posible conflicto de updates |
| IPv6 roto + DNS devuelve AAAA | ip -6 route solo link-local; getent → 2001:... | Timeouts dobles en adaptadores |
| LLM local (Ollama :11434) unreachable | /health → llm.unreachable | health degraded (modo local-degraded) |

Lección: el health check actual de Xavier (`GET /health`) solo mira SU sistema (CPU/RAM/disco/DB/embeddings/mesh). NO mira: swap, PSI, procesos vecinos, servicios del ecosistema, conectividad externa, ni sus propios logs.

## 2. Visión

Xavier debe operar como su propio SRE:

```
┌─────────────────────────────────────────────────────────┐
│  SENSORES (read-only)                                    │
│  · system_health — /health + PSI + swap + top procesos   │
│  · log_reader — ~/.xavier/logs/, journalctl, ~/.hermes/  │
│  · env_scan — servicios systemd, puertos, conectividad   │
└──────────────────────────┬──────────────────────────────┘
                           ▼
┌─────────────────────────────────────────────────────────┐
│  ANÁLISIS (cerebro)                                      │
│  · reglas de anomalía (umbrales PSI/swap/CPU/errores)    │
│  · auto_improvement::gaps extendido → runtime_gaps       │
│  · TGD (Textual Gradient Descent) para recomendar fixes  │
└──────────────────────────┬──────────────────────────────┘
                           ▼
┌─────────────────────────────────────────────────────────┐
│  ACCIÓN                                                 │
│  · ticket_create — issue GitHub (labels) o backlog Maloca│
│  · memory_add kind=incident — indexar hallazgo           │
│  · recommend — acciones concretas para Hermes/BELA       │
└─────────────────────────────────────────────────────────┘
```

## 3. Tools MCP propuestas (src/server/mcp/tools_*)

| Tool | Endpoint/camino | Input → Output |
|---|---|---|
| `sys_health` | `src/health/system_probe.rs` (nuevo) | `{}` → PSI avg10/60/300, swap used, si/so, top5 CPU/RAM, D-state count |
| `log_scan` | `src/health/log_scanner.rs` (nuevo) | `{since_minutes, pattern, sources[]}` → líneas matching + conteo por severidad |
| `env_status` | `src/health/env_probe.rs` (nuevo) | `{services[]}` → systemd active/inactive + RAM/swap por PID + puertos listening |
| `ticket_create` | `src/governance/tickets.rs` (nuevo) | `{title, severity, evidence}` → issue GitHub vía gh CLI o proposal Maloca (store.json) |
| `improve_cycle` (existe parcial) | `src/auto_improvement/cycle.rs` | extender `analyze_gaps` con gaps de runtime |

## 4. Lógica interna — ciclo de automejora extendido

El `AutoImprovementEngine` (cycle.rs) hoy optimiza solo retrieval (benchmark → config overrides).
Extender el loop con "runtime gaps":

1. **Collect**: correr `sys_health` + `log_scan` + `env_status` (cron interno, `XAVIER_CRON_SLEEP_MINUTES` ya existe).
2. **Gap**: comparar contra umbrales → `Gap { kind: Runtime, metric, value, threshold, severity }`.
3. **Ticket**: si severity ≥ warn y no existe ticket abierto equivalente (dedup por title hash) → `ticket_create`.
4. **Recommend**: TGD genera sugerencia (ej. "xavier está 75% en swap → swapoff/swapon + swappiness=10").
5. **Learn**: tras el fix, verificar mejora (PSI antes vs después) → memory_add kind=incident + kind=decision.

## 5. Umbrales iniciales (del caso real)

| Métrica | Warn | Critical | Evidencia |
|---|---|---|---|
| PSI io full avg10 | >30% | >50% | 57% hoy = CRITICAL |
| swap used | >60% | >80% | zram 72% hoy |
| swap-in rate (si) | >20MB/s | >50MB/s | 77MB/s hoy |
| proceso con >4GB VmSwap | warn | — | xavier 5.9GB |
| servicio con polling muerto | sin inbound > 2h | — | gateway Hermes |
| peer/proceso con 100% CPU >10min sin I/O | warn | — | peerjs 12h |
| error_rate en logs > N/min | warn | — | — |

## 6. Integración con Hermes (sensor externo)

Hermes ya hace diagnósticos de ambiente (como este). Protocolo:
- Hermes → `POST /v1/memories` kind=incident con hallazgos estructurados (path `runtime/YYYY-MM-DD/`).
- Xavier los indexa y su ciclo de mejora los cruza con su health interno.
- Regla SWAL: restart de xavier.service lo aplica BELA manualmente; las acciones destructivas (swapoff, kill) requieren aprobación — el ticket_create debe incluir el comando listo + marca "requires approval".

## 7. Plan de fases

| Fase | Alcance | Verificación |
|---|---|---|
| P0 | `sys_health` tool + umbrales PSI/swap (read-only) | MCP tool devuelve datos reales; alerta si PSI>50 |
| P1 | `log_scan` (propios + ~/.hermes/logs) + detección de polling muerto | detecta el caso Telegram de hoy |
| P2 | `ticket_create` → GitHub issue (label `runtime`) con dedup | issue creado sin duplicados |
| P3 | extender `auto_improvement::gaps` con runtime gaps + TGD recommend | ciclo completo en 1 cron |
| P4 | auto-repair seguro (repair.rs): solo acciones reversibles y aprobadas | — |

## 8. Decisiones

- **Sin Jules**: implementación directa (regla F12/SWAL) — workflow Hermes+kimi(design)→implementación local.
- **Read-only primero**: sensores P0-P1 no tocan nada; P4 con aprobación explícita.
- **Dedup de tickets**: hash(title + metric) en store de tickets para no duplicar.
- **Logs**: respetar rotación diaria (~/.xavier/logs/xavier.YYYY-MM-DD); no cargar >1000 líneas por scan.

## 9. Diseño validado (Kimi k3, 2026-08-08)

Diseño de referencia para implementación — módulo `src/self_manage/` (feature-gated `self-manage`):

```
src/self_manage/            # raíz del módulo
├── mod.rs                  # re-exports + registro de tools en MCP router
├── config.rs               # SelfManageConfig: log_paths, journal_unit, ticket_backend,
│                           #   systemd_allowlist, net_probe_targets, scan_window_default,
│                           #   max_read_bytes (1MiB), rate_limits, auto_ticket policy
├── error.rs                # SelfManageError: Io|JournalUnavailable|PermissionDenied|
│                           #   BackendUnreachable|RateLimited|InvalidPattern|DedupeConflict|BackendDisabled
├── audit.rs                # AuditLog append-only de toda acción autónoma
├── rate_limit.rs           # token-bucket por tool (anti-loop)
├── logs/                   # trait LogSource + file_reader.rs (tail con cursor persistido
│                           #   en ~/.xavier/state/scan.cursor) + journal.rs (journalctl -u xavier -o json)
├── env/                    # trait EnvProbe: psi.rs (/proc/pressure/*), memory.rs (/proc/meminfo),
│                           #   proc_top.rs (/proc/*/stat sin shell), systemd.rs (is-active allowlist),
│                           #   net.rs (TCP connect timeout 2s)
├── tickets/                # trait TicketBackend + github.rs (gh CLI/REST) + maloca.rs (backlog)
└── tools/                  # adaptadores MCP: sys_health.rs, log_scan.rs, env_status.rs, ticket_create.rs
```

Contrato de tools:

| MCP tool | Input | Output clave |
|---|---|---|
| `sys_health` | `{verbose, component?}` | overall + components + active_gaps (de analyze_gaps) + last_experiment |
| `log_scan` | `{since?, level_min?, pattern?, source, max_entries≤500}` | entries + truncated + cursor (incremental) + histogram por nivel |
| `env_status` | `{include_processes?, top_n≤20}` | psi (some/full avg10/60/300), swap, load_avg, top_processes, services (allowlist), connectivity, alerts derivados de umbrales |
| `ticket_create` | `{title≤120, body≤8KiB, labels[], severity, fingerprint?}` | id + url + deduplicated + backend |

Reglas de diseño (de Kimi):
1. `self_manage` depende de `health/` y `auto_improvement/` SOLO vía traits (`HealthProvider`, `GapSink`) — sin imports cruzados, test con mocks.
2. `sys_health` consume el handler de `/health` in-process (nunca HTTP loopback a sí mismo).
3. Idempotencia: `log_scan` vía cursor; `ticket_create` vía fingerprint (hash title+severity+componente) consultado antes de crear.
4. Degradación parcial = respuesta exitosa con campos Option/alerts (journald caído no tumba log_scan si files funcionan). Fallos de backend = error de tool (reintentable).
5. Timeouts duros 2-5s por probe — un /proc bloqueado nunca cuelga el MCP server.
6. Anti-loop: rate limit estricto (ej. 3 tickets/hora), `auto_ticket` default `Disabled`, suppress-list de errores originados en `tickets/`.
7. Seguridad: redacción regex de secrets (API keys, Bearer) antes de devolver entries; `journalctl`/`systemctl` con argv array (nunca `sh -c`); unit names contra allowlist; regex sin backtracking; `top_n` capado + cmdlines truncados.
8. Cron loop (`XAVIER_CRON_SLEEP_MINUTES`) gana hook: `env_status` + `log_scan` periódicos → `GapSource::Environment` en auto_improvement.

Riesgos mitigados: loop autónomo, fuga de secretos en logs, command injection, exfiltración de contexto host, costo/ruido de scans (cursor + histograma), acceso journald (grupo systemd-journal, degradar a files-only con warning), feature gate para builds embebidos.

## 10. Siguientes pasos

1. P0: `sys_health` + umbrales PSI/swap (read-only) → verificación: tool devuelve datos reales.
2. P1: `log_scan` + detección de polling muerto (caso Telegram 2026-08-08).
3. P2: `ticket_create` → GitHub issue (label `runtime`) con dedup.
4. P3: runtime gaps en auto_improvement + TGD recommend.
5. P4: auto-repair reversible con aprobación (repair.rs).

## 11. Investigación web (2026-08-08): técnicas, scripts y programas

Investigación para mejorar reacción del sistema + limpieza de RAM/swap. Hallazgos:

### 11.1 zram vs zswap (decisión clave para este host)

| Fuente | Hallazgo |
|---|---|
| linuxblog.io (2025, "zswap IS better than zram") | Con NVMe rápido, si el uso de swap supera regularmente 20-30% de RAM, **zswap** (cache comprimido en RAM + evict a disco) gana a zram: degradación suave, menos escrituras NVMe. Config: `zswap.enabled=1 zswap.compressor=lzo zswap.max_pool_percent=25` |
| laggner.info ("Renaissance of zram") | RAM 16-32GB: zram **25-33%** (no 50%); compresión `zstd` (ratio) o `lz4` (CPU); **prioridad zram > disco** |
| tonybtw.com (NixOS) | zram comprime ~2:1; `memoryPercent=50` en 31GB = ~8GB útiles; priority=100 obligatorio |
| PR nixpkgs #351002 | zramSwap module: sysctls recomendados para "desktops less swappy, more snappy"; **no combinar zswap con zram** (redundante) |

**Diagnóstico del caso real 2026-08-08:** zram 15.6G (50% de RAM) lleno al 72% → thrashing si/so 77MB/s. El zram era DEMASIADO GRANDE para el compresor y el kernel vertía a disco NVMe.

**Recomendación para swal-desktop (31GB, NVMe 990 EVO):**
```nix
# Opción A (recomendada): zram pequeño + swappiness alto (CachyOS-style)
zramSwap = {
  enable = true;
  memoryPercent = 25;          # ~8G de 31G (antes 50% = 15.6G → thrashing)
  priority = 100;
  algorithm = "zstd";
};
boot.kernel.sysctl = {
  "vm.swappiness" = "100";     # zram es barato: usarlo agresivamente, NUNCA disco
  "vm.page-cluster" = "0";     # sin readahead de swap (no aplica a zram)
  "vm.vfs_cache_pressure" = "50";  # retener VFS cache (CachyOS default)
};

# Opción B: migrar a zswap (alternativa válida con NVMe)
# boot.kernelParams = ["zswap.enabled=1" "zswap.compressor=zstd"
#                      "zswap.zpool=zsmalloc" "zswap.max_pool_percent=25"
#                      "zswap.shrinker_enabled=1"];
# boot.initrd.kernelModules = ["zsmalloc"];
# + swap de disco existente (8.8G) y SIN zramSwap
```
Los cambios en /etc/nixos los aplica BELA manualmente (guard SWAL) con `sudo nixos-rebuild switch --flake .#swal`.

### 11.2 OOM prevention: earlyoom vs systemd-oomd

| Programa | Mecanismo | Ventaja | Desventaja |
|---|---|---|---|
| **earlyoom** | % de RAM/swap, poll | Reacciona ANTES (evita freezes de 30-60s con browsers comiendo 20GB swap) | Estimaciones propias, menos granulares |
| **systemd-oomd** | PSI + cgroups v2 | Precisión del kernel; `ManagedOOMMemoryPressureLimit=50%`, `DurationSec=20s` | Reacciona más lento en desktops |
| kernel OOM | — | — | Tarde: sistema ya inutilizable |

**Recomendación:** earlyoom para desktop (config `/etc/default/earlyoom`: `EARLYOOM_ARGS="-r 600 -m 6 -s 80"` aprox). En NixOS: `services.earlyoom.enable = true;` + `services.earlyoom.extraArgs = "-r 600 -m 5 -s 80"`. Alternativa: `services.systemd-oomd.enable` con slice `user@.service` ManagedOOM*.

### 11.3 Guardian / self-healing (referencia para el diseño de Xavier)

| Fuente | Patrón |
|---|---|
| arXiv 2604.03933 (Guardian Agent, 2026) | SRE autónomo: correlaciona dmesg, NVMe SMART, NIC stats, térmicas + incident memory; 300 ciclos de reparación autónoma; predice fallos horas antes |
| Zylos Research (2026) | Ciclo self-healing de 5 etapas: anomaly detection → RCA → remediation → verification; heartbeat + watchdog combinados |
| osModa | Watchdog Rust: exit monitoring + heartbeat + health endpoint + resource monitoring (leaks, CPU runaway) |
| Kernel docs (PSI) | **/proc/pressure/* soporta triggers con epoll()/select()** — eventos nativos en vez de polling |

**Implicación para Xavier:** el módulo `self_manage` puede usar **PSI triggers con epoll** (kernel nativo, sin polling) + `sysinfo`/`procfs` crates para el monitoreo — implementación limpia en Rust. El diseño MAPE-K (Monitor-Analyze-Plan-Execute-Knowledge) es el marco del guardian.

## 12. Xavier como GUARDIÁN del dispositivo (nodo de persistencia mesh)

Rol: Xavier protege el nodo donde corre para ser un **nodo de persistencia confiable del mesh SWAL** — un nodo que se cae por thrashing/OOM no puede persistir datos del mesh. El guardian garantiza disponibilidad:

```
MAPE-K loop (implementado en self_manage + auto_improvement):
M (Monitor)   sys_health + log_scan + env_status — PSI triggers epoll (nativo), sysinfo/procfs
A (Analyze)   umbrales (tabla §5) + incident memory (memorias kind=incident) + TGD
P (Plan)      ticket_create (issue GitHub/Maloca) + recommend (acción concreta reversible)
E (Execute)   auto-repair SOLO acciones reversibles/aprobadas: SIGSTOP→CONT, restart de
              servicios, swapoff/swapon — jamás kill -9 ni borrados (guard SWAL)
K (Knowledge) memory_add kind=incident/decision tras cada ciclo; el patrón se reutiliza
              (predicción: "este patrón PSI ya se vio el 2026-08-08 → runbook X")
```

Reglas del guardian:
1. **Proteger primero la persistencia**: si la RAM < umbral, el guardian prioriza el proceso xavier-real (mlock para vec-store hot pages — ver Clavis `zeroize+mlock`) y recomienda pausar cargas pesadas ajenas (SIGSTOP, nunca kill).
2. **Nunca acciones destructivas sin aprobación**: todo `Execute` es reversible o requiere OK de BELA (guard SWAL). El guardian PLANEA y RECOMIENDA; BELA/Hermes ejecuta lo destructivo.
3. **Anti-loop**: rate limit 3 tickets/h, suppress-list de errores propios, backoff.
4. **Nodo mesh**: al detectar degradación crítica, el guardian anuncia en el mesh (`agent_heartbeat` con status degraded) para que los peers no dependan de este nodo para persistencia (failover proactivo).

## 13. Estado de la sesión 2026-08-08 (ejecutado)

- Gateway Hermes: polling Telegram restaurado (Connected, ESTABLED, sesiones stale prunadas).
- OpenClaw: **detenido y deshabilitado** (openclaw-gateway.service) — dejó de competir por updates de Telegram; puerto 18789 libre.
- peerjs (shelf :9000): eliminado (busy loop 100% CPU 12h, 0 clientes).
- Swap: zram vaciado 11.3G→0, xavier traído a RAM (5.9G→786M VmSwap), drop_caches, zram restaurado 16G prio 100.
- Resultado: si/so 77MB/s→~2-4MB/s; RAM 18G disponibles.
- Pendiente: aplicar config NixOS (§11.1) para el fix permanente del zram.


