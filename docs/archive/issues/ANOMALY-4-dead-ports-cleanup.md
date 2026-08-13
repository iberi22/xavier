# Issue: ANOMALY-4 — Cleanup Dead Clean Architecture Ports and Service Stubs

**Status:** 🔴 PENDING
**Labels:** jules, refactor, clean-code
**Created:** 2026-05-21

---

## 🎯 Objetivo
Identificar, evaluar y remover del codebase los puertos (`ports/`) y servicios (`app/`) que quedaron como stubs inconclusos (`todo!()`) o código muerto, simplificando la arquitectura y eliminando technical debt antes de la distribución general.

## 📋 Descripción del Problema
El archivo `docs/TODO.md` documenta varios puertos que se crearon como parte de un diseño preliminar de arquitectura limpia, pero que nunca llegaron a integrarse y actualmente actúan como código muerto o stubs con macros `todo!()`:

*   `EmbeddingPort` (stub `todo!()` en inbound/outbound ports)
*   `AgentRuntimePort` (stub `todo!()` en inbound/outbound ports)
*   `StoragePort` (stub `todo!()` en inbound/outbound ports)
*   `PatternDiscoverPort` (stub `todo!()` en inbound/outbound ports)
*   `HealthCheckPort` (overhead innecesario, superado por el health handler de HTTP/Axum)
*   `AgentLifecyclePort` (evaluar simplificación)

Además, en el directorio `src/app/` existen stubs de servicios asociados a estos puertos que no tienen un propósito activo en Xavier 1.0. Esto contamina el grafo de dependencias de Rust e incrementa los tiempos de compilación.

## 🔧 Archivos Afectados
*   `src/ports/inbound/mod.rs`
*   `src/ports/outbound/mod.rs`
*   `src/ports/inbound/*.rs` (archivos individuales de puertos huérfanos)
*   `src/ports/outbound/*.rs`
*   `src/app/*.rs` (servicios stubs en desuso)

## ✅ Criterios de Aceptación
1.  **Auditoría de Referencias:** Analizar qué componentes en `src/` importan o implementan los puertos redundantes listados.
2.  **Eliminación de Puertos Huérfanos:**
    *   Remover las definiciones de `PatternDiscoverPort`, `EmbeddingPort`, `AgentRuntimePort` y `StoragePort` que contengan implementaciones vacías o stubs `todo!()`.
    *   Eliminar el puerto `HealthCheckPort` si todo el flujo de verificación de salud de la API HTTP corre a través del handler nativo Axum.
3.  **Remoción de Archivos Físicos:** Eliminar de forma segura los archivos `.rs` individuales de estos puertos y limpiar sus declaraciones en los respectivos `mod.rs`.
4.  **Verificación de Compilación y Testeo:** Garantizar que Xavier compila sin advertencias de código muerto (`dead_code`) y que las pruebas unitarias pasan en su totalidad.

## 🔧 Comandos de Verificación
1.  Compilar el proyecto completo:
    ```bash
    cargo check --target-dir target_local --all-targets
    ```
2.  Correr los tests unitarios:
    ```bash
    cargo test --target-dir target_local --lib
    ```
