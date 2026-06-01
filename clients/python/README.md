# Xavier Python SDK (xavier-py)

Professional-grade Python SDK for interacting with Xavier's high-performance memory API.

## Installation

Desde tu repo local (editable para desarrollo):

```bash
cd clients/python
pip install -e .
```

O desde PyPI cuando esté publicado:

```bash
pip install xavier-py
```

## Quickstart

### Configurar token

```powershell
# Windows PowerShell
$env:XAVIER_TOKEN = "dev-token"
```

```bash
# Linux/Mac
export XAVIER_TOKEN="your-token"
```

> ⚠️ Siempre usa un token real en producción. Sin token el cliente lanza una advertencia.

### Uso sincrónico

```python
from xavier_py import XavierClient

# Conecta a localhost:8006 por defecto
client = XavierClient()

# 1. Ver estado del servidor
stats = client.stats()
print(f"Xavier v{stats.version} — workspace: {stats.workspace_id}")
# → Xavier v0.4.1 — workspace: default

# 2. Agregar un documento a memoria
result = client.add(
    "Xavier soporta búsqueda híbrida semántica + lexical con RRF fusion.",
    path="docs/retrieval",
    metadata={"author": "swal", "type": "technical"}
)
print(f"Agregado: id={result['id']}")
# → Agregado: id=01KT2CTM4Z7DW732G1QFGXNPQ5

# 3. Buscar en memoria
results = client.search("búsqueda híbrida", limit=5)
print(f"Resultados: {results.count}")
for doc in results.results:
    print(f"  [{doc.id[:8]}] {doc.content[:80]}...")

# 4. Eliminar por ID
client.delete(id=result["id"])
```

### Uso asincrónico

```python
import asyncio
from xavier_py import XavierClient

async def main():
    client = XavierClient()

    # Operaciones async
    r = await client.add_async(
        "Xavier implementa retrieval multi-capa con working, episodic y semantic memory.",
        path="docs/memory-layers"
    )
    print(f"Memoria agregada: {r['id']}")

    resp = await client.retrieve_async("memory layers", limit=3)
    print(f"Retrieved {len(resp.results)} results")
    for m in resp.results:
        print(f"  [{m.source_layer}] score={m.score:.3f}: {m.content[:60]}")

    stats = await client.stats_async()
    print(f"Verificación: Xavier v{stats.version} alive")

asyncio.run(main())
```

## API Reference

### `XavierClient(base_url, token)`

| Parámetro | Default | Descripción |
|-----------|---------|-------------|
| `base_url` | `"http://localhost:8006"` | URL del servidor Xavier |
| `token` | `XAVIER_TOKEN` env var | Token de autenticación |

### Métodos sincrónicos

| Método | Endpoint | Descripción |
|--------|----------|-------------|
| `stats()` | `GET /memory/stats` | Estado y versión del servidor |
| `add(content, path?, metadata?, **kwargs)` | `POST /memory/add` | Agregar documento a memoria |
| `search(query, limit=10, filters?)` | `POST /memory/search` | Búsqueda híbrida semántica + lexical |
| `retrieve(query, limit=10, **kwargs)` | `POST /memory/retrieve` | Retrieval multi-capa |
| `delete(id?, path?)` | `POST /memory/delete` | Eliminar por ID o path |

Mismos métodos con sufijo `_async` para versión asincrónica (ej: `stats_async()`, `add_async()`).

### Manejo de errores

```python
from xavier_py import XavierClient

client = XavierClient()

# Error de autenticación
try:
    client.stats()
except Exception as e:
    print(f"Error: {e}")

# Delete sin parámetros
try:
    client.delete()
except ValueError as e:
    print(f"Validación: {e}")
    # → Either 'id' or 'path' must be provided.

# Delete de ID inexistente (devuelve status not_found, no exception)
result = client.delete(id="nonexistent")
print(result["status"])  # → not_found
```

## Ejemplos de uso real

### Pipeline típico

```python
from xavier_py import XavierClient
import time

client = XavierClient()

# 1. Guardar documentos
docs = [
    ("Xavier usa SQLite con FTS5 y vectores embedding para búsqueda híbrida.", "docs/search"),
    ("El retrieval multi-capa combina working, episodic y semantic memory.", "docs/layers"),
    ("RRF fusion rankea resultados combinando scores semánticos y léxicos.", "docs/ranking"),
]
for content, path in docs:
    r = client.add(content, path=path)
    print(f"✓ {path} → id={r['id'][:8]}")

# 2. Buscar
results = client.search("cómo funciona la búsqueda en Xavier", limit=3)
print(f"\nResultados ({results.count} total):")
for doc in results.results:
    print(f"  {doc.content[:70]}...")
```

### Verificar conectividad

```python
from xavier_py import XavierClient

def check_xavier_health(url="http://localhost:8006"):
    """Health check rápido para Xavier."""
    try:
        client = XavierClient(base_url=url)
        s = client.stats()
        return {"status": "ok", "version": s.version}
    except Exception as e:
        return {"status": "error", "detail": str(e)}

print(check_xavier_health())
# → {'status': 'ok', 'version': '0.4.1'}
```

## Features

- **Sync & Async**: Soporte dual con `requests` (sync) y `aiohttp` (async)
- **Type Safety**: Respuestas validadas con Pydantic v2
- **Auto-Auth**: Toma `XAVIER_TOKEN` del environment automáticamente
- **Timeouts**: 30 segundos en todas las requests
- **404 Graceful**: Delete devuelve `not_found` en vez de exception
- **Multi-layer**: Acceso directo al sistema de retrieval híbrido de Xavier

## License

MIT
