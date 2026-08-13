# Issue: ANOMALY-3 — Outdated Pending Task in `TODO.md` for Resolved Data Race in `get_last_sync_result`

**Status:** 🔴 PENDING
**Labels:** jules, documentation, low-priority
**Created:** 2026-05-21

---

## 🎯 Objetivo
Auditar `docs/TODO.md` y marcar como completada la tarea crítica de solucionar la data race en `get_last_sync_result()`, ya que el codebase actual fue refactorizado correctamente para mitigar este problema.

## 📋 Descripción del Problema
En `docs/TODO.md`, bajo la sección de **Antes de v1.0 — BLOCKERS**, aparece la siguiente tarea de prioridad crítica marcada como **PENDIENTE**:
```markdown
| 1 | Fix data race en `get_last_sync_result()` — leer todos los campos bajo un lock | 🔴 CRITICAL | **PENDIENTE** |
```

Sin embargo, al auditar `src/tasks/session_sync_task.rs`, se observa que ya se implementó un lock unificado sobre un struct estático `LAST_CHECK_RESULT` de tipo `StdRwLock<SyncCheckResult>`:
```rust
pub(crate) static LAST_CHECK_RESULT: Lazy<StdRwLock<SyncCheckResult>> =
    Lazy::new(|| StdRwLock::new(SyncCheckResult::default()));

pub fn get_last_sync_result() -> SyncCheckResult {
    LAST_CHECK_RESULT
        .read()
        .map(|r| r.clone())
        .unwrap_or_default()
}
```
Esto garantiza que todos los campos del resultado de sincronización se lean de manera atómica bajo un único lock de lectura, eliminando por completo cualquier data race al consultar el estado de la sincronización desde los endpoints REST. La documentación en `TODO.md` y los release criteria están desactualizados y deben reflejar esta corrección.

## 🔧 Archivos Afectados
*   `docs/TODO.md`

## ✅ Criterios de Aceptación
1.  **Actualizar el Status en TODO.md:** Cambiar el estado de la tarea 1 de `**PENDIENTE**` a `✅ **COMPLETADO**` (o moverla a la sección de completados).
2.  **Actualizar Release Criteria:** Marcar la casilla en "v1.0 Release Criteria":
    ```markdown
    - [x] Data race fix commiteado
    ```
3.  **Verificación Visual:** Asegurarse de que no queden referencias confusas que sugieran que este problema de concurrencia sigue activo en el motor de Xavier.

## 🔧 Comandos de Verificación
1.  Verificar que el archivo `docs/TODO.md` guarde el formato Markdown correcto tras los cambios.
