[local-first][embeddings-local][feat] Reimplementar sondeo Ollama en `EmbedderConfig::auto()` (scope: SOLO src/embedding/mod.rs)

## 🎯 Contexto

El PR #532 intentaba hacer esto pero estaba **malformado**: su diff masivo borraba accidentalmente todo `xavier-core/`. Se revirtió (#535). El cambio **útil** de #532 —hacer que `auto()` sondee Ollama local y use `embeddinggemma` como default inteligente— se perdió y hay que reimplementarlo, esta vez **con scope estricto a un solo archivo**.

## 📋 Estado actual

`src/embedding/mod.rs::auto()` (línea 187) requiere señales explícitas (`XAVIER_EMBEDDING_PROVIDER_MODE` o `XAVIER_EMBEDDER`). Si no hay señal y no hay claves cloud → cae a `Noop` (embeddings desactivados). Una instalación limpia sin config no tiene embeddings.

## ✅ Criterio de aceptación

1. Modificar **solo** `src/embedding/mod.rs`. No tocar `xavier-core/`, `Cargo.toml` members, ni otros crates.
2. Cambiar `auto()` para que cuando no haya señal cloud ni local explícita, **sondee Ollama** (`GET http://localhost:11434/v1/models`, timeout ≤2s): si responde → `local_only()` con `embeddinggemma`; si no → `Noop` + `warn!`.
3. Orden de preferencia en `auto()`:
   - Explicit `XAVIER_EMBEDDING_PROVIDER_MODE` → respeta.
   - Else if claves cloud presentes → cloud.
   - Else if Ollama local reachable → **local (embeddinggemma)** ← nuevo default inteligente.
   - Else → `Noop` + alerta.
4. Log claro: `"Embeddings backend: local-ollama(embeddinggemma) | cloud-openai | disabled(noop)"`.
5. Si `embeddinggemma` no está en la lista de modelos de Ollama, `warn!`: `"Modelo embeddinggemma no encontrado. Ejecuta: ollama pull embeddinggemma"`.
6. Tests unitarios de `auto()` con mocks de env vars.

## 🚫 Fuera de scope (NO hacer)
- No tocar `xavier-core/` (issue separado de estabilización).
- No tocar `Cargo.toml` workspace members.
- No crear archivos `*_part1.rs` sueltos (fallo de #534).

## 🔧 Alcance de archivos (estricto)
- `src/embedding/mod.rs` (único archivo a modificar)

## 🧪 Cómo verificar
```bash
ollama pull embeddinggemma
unset XAVIER_EMBEDDING_PROVIDER_MODE
cargo run -- serve
# log: "Embeddings backend: local-ollama(embeddinggemma)"
cargo test -p xavier embedding::auto
```

## 📎 Referencias
- PR #532 (malformado, revertido) — referencia de la intención original.
- Issue #512 (original LOCAL1-04).
