# Issue: ANOMALY-2 — Strict Literal Substring Filtering in `filter_symbols_by_query` Destroys Fuzzy & Semantic Query Results

**Status:** 🔴 PENDING
**Labels:** jules, bug, high-priority
**Created:** 2026-05-21

---

## 🎯 Objetivo
Modificar o eliminar la sobre-restricción de filtrado en `filter_symbols_by_query` para que las búsquedas semánticas o difusas no resulten erróneamente en `0` resultados.

## 📋 Descripción del Problema
Cuando el cliente realiza búsquedas en el codebase a través de `code_find_handler` o `code_context_handler`, el backend utiliza el motor de base de datos de grafos de código (`state.code_query`). Dicho motor tiene capacidades inteligentes de búsqueda.

Sin embargo, en `src/adapters/inbound/http/handlers/code.rs` se ejecuta la función `filter_symbols_by_query` inmediatamente antes de retornar:
```rust
fn filter_symbols_by_query(symbols: &mut Vec<code_graph::types::Symbol>, query: &str) {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() { return; }

    symbols.retain(|symbol| {
        symbol.name.to_ascii_lowercase().contains(&query)
            || symbol.signature.as_deref().unwrap_or_default().to_ascii_lowercase().contains(&query)
            || symbol.file_path.to_ascii_lowercase().contains(&query)
    });
}
```
### ¿Por qué es un bug grave?
Si un usuario busca `"memory storage"` y el motor de indexación devuelve con éxito un símbolo llamado `struct CacheStore` (debido a relaciones semánticas u ocurrencias textuales cruzadas), la función `filter_symbols_by_query` **eliminará** este resultado de inmediato, ya que el string `"cachestore"` no contiene literalmente la subcadena `"memory storage"`. 

Esto causa que la gran mayoría de las consultas inteligentes devuelvan de forma constante **0 resultados**, inutilizando la funcionalidad de búsqueda inteligente.

## 🔧 Archivos Afectados
*   `src/adapters/inbound/http/handlers/code.rs`

## ✅ Criterios de Aceptación
1.  **Diferenciación de Búsqueda:** Distinguir si la búsqueda requiere coincidencia exacta literal (ej. filtros por tipo o patrón de código) o si es una consulta difusa de texto/semántica libre.
2.  **Relajar o Desactivar en Búsqueda General:**
    *   Si se trata de una búsqueda general a través de `search_code_symbols_with_fallback`, **no se debe aplicar** el filtro estricto literal `.contains()`. Se debe confiar en el ranking del propio motor de base de datos de código.
    *   Si se aplica, permitir un threshold de similitud difusa (fuzzy logic) o solo aplicar el filtrado estricto si el usuario especifica explícitamente un patrón exacto (`payload.pattern` o `payload.kind`).
3.  **Tests de Integración:** Verificar que búsquedas de conceptos lógicos en un codebase dummy indexado (por ejemplo, buscar "store" y recuperar structs con nombres relacionados) no sean filtradas a cero.

## 🔧 Comandos de Verificación
1.  Verificar compilación:
    ```bash
    cargo check --target-dir target_local
    ```
2.  Correr pruebas del módulo de código:
    ```bash
    cargo test --target-dir target_local --lib adapters::inbound::http::handlers::code::tests
    ```
