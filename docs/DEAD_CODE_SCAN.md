# Dead Code Scan Report

**Date:** 2026-04-19
**Tool:** Manual analysis + clippy + grep

## Summary

**Overall: LOW — Mínimo dead code en producción.**

Solo se encontraron **6 instancias de `#[allow(dead_code)]`** en todo el proyecto, y ningún módulo/archivo completo está muerto.

## Hallazgos

### 1. `#[allow(dead_code)]` — state.rs (UI state)

- **Archivo:** `src/adapters/inbound/http/state.rs`
- **Líneas:** 29, 35, 38, 42, 45, 49
- **Content:** 6 campos de struct marcados como `#[allow(dead_code)]`
- **Veredicto:** Son campos de estado de UI del panel que aún no se usan pero se necesitan en el struct. **JUSTIFICADO** — mantener.
- **Recomendación:** Agregar comentario `// reserved for future UI state` o similar.

### 2. `#[allow(dead_code)]` que ya eliminamos en Phase 1

- `src/agents/system3/helpers/date.rs` — `has_temporal_signal()` → ya corregido (se le puso doc + `#[allow]` documentado)
- `src/security/layers/tool_alias.rs` — `needless_range_loop` → ya corregido

### 3. `pub use` en mod.rs — todos justificados

El patrón `pub use` de re-exportación se usa extensivamente (~50+ sitios) pero todos son necesarios:
- **Módulos agnósticos a ruta** (ej: `coordination::message_bus::*` → `coordination::*`)
- **Interfaces de puertos** (`ports::*`)
- **Tipos de dominio expuestos** (`domain::*`)

Ninguno es código muerto. Es una práctica de API design.

### 4. Funciones no usadas detectadas por clippy (36 warnings)

Los 36 warnings de clippy incluyen:
- `unused import` — imports que sobran (muchos ya corregidos en el commit clippy)
- `unused variable` — variables en test/ejemplo
- `method never used` — métodos públicos disponibles para API externa
- `function never used` — funciones helper públicas

**Ninguno requiere `#[allow(dead_code)]`.** Son warnings de estilo, no de código muerto real.

## Recomendaciones

1. ✅ No hay código muerto removible
2. ✅ Los 36 warnings de clippy son de imports/variables no usadas, no de módulos completos
3. ✅ Los `pub use` son parte del diseño de API
4. ➡️ Para los campos de `state.rs`, agregar comentario de intención futura
