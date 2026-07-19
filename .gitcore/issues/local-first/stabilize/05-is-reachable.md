## 🎯 Contexto

Para que Xavier sea 100% local (LLM + embeddings vía Ollama), la función que decide si un proveedor está listo **miente**: `ModelProviderConfig::is_configured()` en `src/agents/provider/config.rs:491` devuelve `true` para modo `Local` mientras la `base_url` no sea vacía — **incluso si Ollama no está corriendo**. Eso hace que el sistema crea tener un proveedor cuando no puede generar texto, y los errores llegan tarde y crípticos.

Un intento anterior de Jules falló con `unexpected error`. Este issue es para reimplementarlo limpio.

## 📋 Problema

Hoy (`src/agents/provider/config.rs:491`):
- `Local` → `true` si `base_url` no está vacía. Nunca comprueba que el endpoint responda.
- Resultado: el chat del panel intenta usar el proveedor local, falla con error de conexión, el usuario ve "No pude contactar el modelo de IA".

## ✅ Criterio de aceptación

1. Crear un método `is_reachable()` en `ModelProviderConfig` (o en `ModelProviderClient`) que haga un `GET {base_url}/models` (Ollama expone `/v1/models` compatible OpenAI) con un **timeout corto (≤2s)** y cachee el resultado en memoria por ~15s.
2. **No romper `is_configured()` existente** (sync, barato): que siga validando campos. Añadir `is_reachable()` como chequeo adicional de red.
3. Distinguir tres estados: `Configured & Reachable`, `Configured & Unreachable` (Ollama caído), `NotConfigured`.
4. En modo `Local`, el `api_key` por defecto `"ollama"` debe aceptarse sin exigir clave real.
5. Tests unitarios: `is_configured()` sigue true para local con URL no vacía; `is_reachable()` devuelve `Err`/`false` ante un puerto cerrado y `Ok`/`true` ante un mock HTTP (usar `mockito`).
6. **No modificar** `ProxyUseCase` ni el handler del panel (eso es otro issue — sólo tocas `src/agents/provider/config.rs` y opcionalmente `client.rs`).

## 🔧 Alcance de archivos (estricto)
- `src/agents/provider/config.rs` (principal)
- `src/agents/provider/types.rs` (añadir tipo de estado si hace falta)
- Tests nuevos.

## 🧪 Cómo verificar
```bash
cargo test -p xavier provider::config::is_reachable
cargo clippy -p xavier -- -D warnings
```

## 📎 Referencias
- Issue original LOCAL1-01.
