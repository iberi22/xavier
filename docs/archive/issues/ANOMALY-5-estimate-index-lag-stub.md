# Issue: ANOMALY-5 — Replace Hardcoded / Stubbed `estimate_index_lag()` with Dynamic Calculations

**Status:** 🔴 PENDING
**Labels:** jules, feature, medium-priority
**Created:** 2026-05-21

---

## 🎯 Objetivo
Reemplazar cualquier cálculo estático, mock o simplificado de retraso de indexación (`estimate_index_lag()`) por un algoritmo dinámico y real que calcule la diferencia temporal exacta entre los eventos registrados y su registro físico en el almacén de memoria de Xavier.

## 📋 Descripción del Problema
En `docs/TODO.md`, se reporta que el cálculo del retraso del índice es un stub o aproximación estática (por ejemplo, devolviendo de forma constante ~5 minutos o estimaciones simplificadas):
```markdown
| `estimate_index_lag()` real (no stub ~5min) | 🟡 MEDIUM | PENDIENTE |
```

En `src/tasks/session_sync_task.rs`, se observa una lógica que estima el lag leyendo registros del almacén de memoria (`MemoryKind::Session`):
```rust
async fn estimate_index_lag(&self) -> u64 {
    if let Some(ref storage) = self.memory_store {
        // ...
        if let Some((event_ts, indexed_ts)) = records
            .iter()
            .filter_map(|record| {
                session_event_timestamp_ms(&record.content)
                    .map(|ts| (ts, record.updated_at.timestamp_millis()))
            })
            .max_by_key(|(event_ts, _)| *event_ts)
        {
            return indexed_ts.saturating_sub(event_ts).max(0) as u64;
        }
    }
    0
}
```
### Problema
Esta lógica depende enteramente de que exista un `self.memory_store` cargado en `SessionSyncTask`. Si el store es `None` (como ocurre en implementaciones desacopladas de red), el método retorna por defecto `0`. Además, en escenarios reales de base de datos distribuidas o SQLite local, las marcas de tiempo de los eventos de sesión a veces se estiman estáticamente en 5 minutos en lugar de sincronizarse con precisión milimétrica en tiempo real. Se requiere una solución robusta que estime de forma confiable el desfase real de indexación bajo cualquier escenario de almacenamiento.

## 🔧 Archivos Afectados
*   `src/tasks/session_sync_task.rs`

## ✅ Criterios de Aceptación
1.  **Garantizar fallback confiable:** Implementar una estrategia de fallback robusta cuando `self.memory_store` no está configurado (por ejemplo, consultando el endpoint `/memory/stats` o una cola intermedia).
2.  **Cálculo preciso del desfase:** Usar marcas de tiempo Unix de milisegundos de alta precisión para comparar el instante en el que ocurre el evento versus cuando se escribe en SQLite (`updated_at`).
3.  **Alertas funcionales:** Integrar este valor con el sistema de alertas de `SessionSyncTask` (si el lag supera los 30 segundos, levantar estado de `"alert"` de manera dinámica).
4.  **Pruebas unitarias de desincronización:** Diseñar un test unitario donde se añadan registros con marcas de tiempo artificialmente retrasadas y verificar que `estimate_index_lag()` calcula exactamente el retraso esperado en milisegundos.

## 🔧 Comandos de Verificación
1.  Verificar que el proyecto compile sin advertencias:
    ```bash
    cargo check --target-dir target_local
    ```
2.  Ejecutar las pruebas asociadas al cron de sincronización:
    ```bash
    cargo test --target-dir target_local --lib tasks::session_sync_task::tests
    ```
