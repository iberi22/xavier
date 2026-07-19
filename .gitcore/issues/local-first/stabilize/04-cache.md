## 🎯 Contexto

`src/embedding/cache.rs` (473 líneas) es el cache de embeddings. En modo 100% local, los embeddings vía Ollama son gratis pero **no instantáneos** (latencia de inferencia). Un cache efectivo reduce llamadas repetidas y acelera indexación de memoria y code-graph.

Un intento anterior (PR #534) **falló**: contenía archivos basura sueltos en la raíz (`cache_part1.rs`, `mod_part1.rs`). Este issue es para reimplementarlo **correctamente, tocando solo `src/embedding/cache.rs`**.

## 📋 Problema

- Revisar `src/embedding/cache.rs`: ¿es in-memory? ¿persiste? ¿Invalida al cambiar de modelo de embedding (dimensión distinta)?
- Para 100% local, el cache debe estar **keyado por modelo de embedding** (no solo por texto): si cambias de `embeddinggemma` a otro modelo, los vectores cacheados (dimensión distinta) NO deben servirse.

## ✅ Criterio de aceptación

1. Modificar **solo** `src/embedding/cache.rs` (y opcionalmente `src/embedding/mod.rs` para exponer la API). 
2. Garantizar que el cache está keyado por `{model, text_hash}` → vector. Así cambiar de modelo invalida automáticamente.
3. Añadir persistencia opcional a disco (tabla SQLite `embedding_cache` o archivo mmap) activable vía env `XAVIER_EMBEDDING_CACHE_PERSIST=1`.
4. Métricas: hits/misses logueados a nivel `debug`.
5. Tests unitarios: hit tras insert, miss al cambiar modelo, evicción por capacidad.
6. **No cambiar la API pública** del trait `Embedder` — el cache debe ser transparente (wrapper).

## 🚫 Fuera de scope (NO hacer)
- NO crear archivos `*_part1.rs`, `*_part2.rs`, `mod_part*.rs` sueltos en la raíz del repo (fallo de #534).
- NO tocar `xavier-core/`, `Cargo.toml` workspace members.

## 🔧 Alcance de archivos (estricto)
- `src/embedding/cache.rs` (único archivo principal)
- `src/embedding/mod.rs` (solo si necesitas exponer la API del cache)

## 🧪 Cómo verificar
```bash
cargo test -p xavier embedding::cache
```

## 📎 Referencias
- PR #534 (cerrado por malformado) — referencia del error a evitar.
- Issue original LOCAL1-11.
