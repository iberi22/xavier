# RULES.md — Reglas de Codificación y Convenciones

> Reglas del proyecto Xavier para mantener código limpio, consistente y mantenible.
> Coding conventions and project rules for clean, consistent, maintainable code.

---

## 🏛️ Reglas Generales / General Rules

### 1. Simplicity First (Karpathy Rule #2)
- Mínimo código que resuelva el problema. Nada especulativo.
- Si puedes escribir 50 líneas en vez de 200, hazlo.
- No agregues abstracciones que no necesitas hoy.

### 2. Surgical Changes (Karpathy Rule #3)
- Solo toca lo que debes cambiar. No "mejores" código ajeno sin razón.
- Si creas código huérfano, elimínalo.
- Un PR debe hacer una cosa y hacerla bien.

### 3. Think Before Coding (Karpathy Rule #1)
- No asumas. Si hay confusión, surface tradeoffs.
- Busca en Xavier ANTES de implementar — probablemente alguien ya lo resolvió.
- Para tareas multi-step: escribe plan `[Step] → verify: [check]`

### 4. Memory First
- Siempre buscar en Xavier (`POST /memory/search`) antes de decisiones complejas.
- Siempre persistir resultados en Xavier (`POST /memory/add`) después de completar.
- Actualizar `MEMORY.md` cuando haya decisiones arquitectónicas nuevas.

---

## 🦀 Reglas de Rust / Rust Rules

### Código
- Sigue las convenciones de Rust 2021 edition
- Usa `thiserror` para tipos de error personalizados
- Usa `tracing` para logging (no `log`, no `println!`)
- Mantén funciones < 50 líneas donde sea posible
- Prefiere `impl Trait` sobre `Box<dyn Trait>` cuando sea posible
- Usa `clippy` — todas las advertencias deben resolverse

### Tokio + Rayon (Golden Rule)
```
🚫 NUNCA llamar .par_iter() de Rayon dentro de un worker de Tokio
✅ SIEMPRE usar tokio::task::spawn_blocking para Rayon
```
Esto bloquea el event loop y cuelga webhooks, I/O, y todo lo demás.

### Testing
- Tests unitarios junto al código (`#[cfg(test)] mod tests`)
- Tests de integración en `tests/`
- `cargo test --lib` debe pasar antes de cualquier commit
- Nombrar tests descriptivamente: `test_<module>_<what_it_tests>`
- Usar `#[should_panic]` solo cuando sea semánticamente correcto

### Commits
```
formato: <type>(<scope>): <description> (closes #<issue>)
tipos: feat | fix | refactor | test | docs | chore | style
ejemplo: feat(mcp): add code_graph_explore tool (closes #441)
```

---

## 📁 Estructura del Proyecto

```
src/
├── main.rs              # Entry point
├── lib.rs               # Library root
├── memory/              # Memory engine (core)
│   ├── mod.rs
│   ├── entity_graph/    # Belief graph
│   ├── hormer/          # Hierarchical navigation
│   └── openclaw_*.rs    # OpenClaw scanner/indexer
├── server/              # HTTP + MCP servers
│   ├── http/            # Axum routes
│   └── mcp/             # MCP tools
├── mesh/                # P2P sync + governance
├── security/            # Crypto, auth, license
├── cli/                 # CLI commands
├── telegram/            # Telegram bot
└── observability/       # Health, notifications
```

### Reglas de Módulos
- Un archivo = un módulo (excepto módulos grandes con submódulos)
- `mod.rs` solo re-exporta — la lógica va en archivos nombrados
- Usar `pub use` para API pública clara
- Separar concerns: dominio ≠ infraestructura

---

## 📝 Documentación

### Reglas
- Documentar `pub` items con doc comments (`///` o `//!`)
- README.md describe el qué y por qué, no el cómo
- `docs/` para documentación extensa
- Mantener docs sincronizadas con código (el alignment audit es real)
- Actualizar `CHANGELOG.md` en cada release

#### R-DOC: Regla de Documentación (GitCore Protocol)
> **Todo cambio de código debe ir acompañado de la actualización de documentos asociados.**

1. **Al implementar una feature:** actualizar `.gitcore/features.json` (progreso, tests, estado)
2. **Al cambiar arquitectura:** actualizar `.gitcore/ARCHITECTURE.md` y `docs/devlog/`
3. **Al cambiar API:** actualizar `docs/API.md` y `docs/MCP_CONTRACT.md`
4. **Al completar tarea:** actualizar `.gitcore/planning/TASK.md` (progreso, hitos, deuda)
5. **Al hacer release:** actualizar `CHANGELOG.md` y `.gitcore/STATE.md`
6. **Cross-reference check:** verificar que los enlaces en PLANNING.md, SRC.md y AGENTS.md sigan válidos

Incumplir R-DOC genera deuda técnica documental que debe pagarse en el siguiente sprint.

### Archivos que SIEMPRE deben estar actualizados
| Archivo | Qué contiene | Quién lo mantiene |
|---------|-------------|-------------------|
| `MEMORY.md` | Decisiones, lecciones | Todo agente |
| `TASK.md` | Progreso de tarea activa | Agente actual |
| `.gitcore/features.json` | Estado de features | Después de cada feature |
| `CHANGELOG.md` | Release notes | En cada release |

---

## 🤖 Reglas para Agentes / Agent Rules

1. **Siempre leer `AGENTS.md`, `SOUL.md`, `USER.md` al inicio**
2. **Siempre buscar en Xavier antes de trabajar** (`mem_search`)
3. **Siempre persistir después de completar** (`create_memory`)
4. **Commits atómicos con referencias a issues**
5. **No crear archivos temporales en la raíz** — usar `temp/` o `scripts/subagents/reports/`
6. **Actualizar documentación afectada por cambios**
7. **Si no estás seguro, PREGUNTA** — no asumas

---

## 🔒 Seguridad

- Nunca committear `.env`, tokens, API keys
- Secrets van en vault Clavis o env vars, nunca en código
- Endpoints HTTP requieren `X-Xavier-Token` (excepto `/health`)
- Reportar vulnerabilidades vía `SECURITY.md`
- Usar `cargo audit` periódicamente

---

## ❌ Anti-patrones / Anti-patterns

| Anti-patrón | Alternativa |
|-------------|-------------|
| Funciones de 200+ líneas | Dividir en funciones pequeñas |
| Archivos `utils.rs` / `helpers.rs` | Nombrar por dominio: `search.rs`, `storage.rs` |
| `unwrap()` en producción | `?` o `.context()` con thiserror |
| Comentarios que explican el qué | El código debe ser auto-documentado |
| Feature flags sin documentación | Documentar en `docs/` o `features.json` |
| Código muerto comentado | Eliminarlo (git log lo recupera) |

---

_Última actualización: 2026-07-09_
