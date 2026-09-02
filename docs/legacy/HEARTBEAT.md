# HEARTBEAT.md — Checklist de Heartbeat para el Agente

> Qué verificar al iniciar cada sesión de trabajo en Xavier.
> What to check when starting a new work session on Xavier.

---

## 🟢 Heartbeat Checklist (Inicio de Sesión)

### 1. Contexto / Context
- [ ] Leer `SOUL.md` — Recordar quién soy (CEO de SWAL)
- [ ] Leer `USER.md` — Recordar quién es BELA
- [ ] Leer `AGENTS.md` — Protocolo de subagentes
- [ ] Leer `CLAUDECODE_TASK.md` o `TASK.md` — Tarea activa
- [ ] Leer `MEMORY.md` — Decisiones pasadas y lecciones

### 2. Servidor Xavier / Xavier Server
- [ ] Verificar que Xavier corre: `curl http://localhost:8006/health`
- [ ] Si no corre, iniciar: `./start-xavier-rag.ps1` o `cargo run -- http 8006`
- [ ] Verificar token: `$env:XAVIER_TOKEN` está configurado
- [ ] Buscar contexto relevante en Xavier: `POST /memory/search`
- [ ] Verificar `cargo check` pasa sin errores

### 3. Repositorio / Repository
- [ ] `git status` — Sin cambios sin commit
- [ ] `git pull` — main actualizado
- [ ] Revisar issues abiertos (label `jules` o `bug`)
- [ ] Revisar PRs pendientes

### 4. Entorno / Environment
- [ ] Docker containers necesarios corriendo (xavier, pgheart, etc.)
- [ ] `cargo check` passes
- [ ] Tests principales pasan: `cargo test --lib`

### 5. Tarea / Task
- [ ] Estado actual de la tarea
- [ ] Dependencias bloqueantes
- [ ] ¿Qué hay que lograr en esta sesión?

---

## 📋 Al Finalizar la Sesión / End of Session

- [ ] Persistir resultados/decisiones en Xavier (`POST /memory/add`)
- [ ] Actualizar `TASK.md` con progreso
- [ ] Actualizar `MEMORY.md` si hay nuevas decisiones
- [ ] Hacer commit con mensaje descriptivo (`feat(scope): ...`)
- [ ] Push a origin
- [ ] Si hay bug o feature request → crear GitHub Issue

---

## 🚨 Señales de Alerta / Red Flags

| Señal | Acción |
|-------|--------|
| `cargo check` falla | Arreglar antes de continuar |
| Xavier no responde en :8006 | Verificar proceso / Docker |
| Tests existentes fallan | No tocar código hasta que pasen |
| Documentación inconsistente con código | Actualizar docs o crear issue |
| Token no configurado | Revisar `.env` o vault Clavis |

---

_Última actualización: 2026-07-09_
