# Xavier TypeScript SDK (@iberi22/xavier)

Official, async-first TypeScript SDK for the Xavier Memory API.

## Instalación

```bash
npm install @iberi22/xavier
```

Requiere Node.js >= 18 (usa `fetch` nativo y `AbortController`).

## Quickstart

### Configurar token

```bash
export XAVIER_TOKEN="dev-token"
```

### Uso básico

```typescript
import { XavierClient } from '@iberi22/xavier';

// Conecta a localhost:8006 por defecto
const client = new XavierClient();

async function main() {
  // 1. Ver estado del servidor
  const stats = await client.stats();
  console.log(`Xavier v${stats.version} — workspace: ${stats.workspace_id}`);
  // → Xavier v0.4.1 — workspace: default

  // 2. Agregar un documento a memoria
  const added = await client.add({
    content: 'Xavier soporta búsqueda híbrida semántica + lexical con RRF fusion.',
    path: 'docs/retrieval',
    metadata: { author: 'swal', type: 'technical' }
  });
  console.log(`Agregado: id=${added.id}`);
  // → Agregado: id=01KT2CTM4Z7DW732G1QFGXNPQ5

  // 3. Buscar en memoria
  const results = await client.search('búsqueda híbrida', 5);
  console.log(`Resultados: ${results.count}`);
  for (const doc of results.results) {
    console.log(`  [${doc.id.slice(0, 8)}] ${doc.content.slice(0, 80)}`);
  }

  // 4. Eliminar por ID
  await client.delete({ id: added.id });
}

main().catch(console.error);
```

## API Reference

### `new XavierClient(options?)`

```typescript
interface ClientOptions {
  baseUrl?: string;    // default: "http://localhost:8006"
  token?: string;      // default: process.env.XAVIER_TOKEN
  timeoutMs?: number;  // default: 30000 (30s)
}
```

### Métodos

| Método | Endpoint | Descripción |
|--------|----------|-------------|
| `stats()` | `GET /memory/stats` | Estado y versión del servidor |
| `add(payload)` | `POST /memory/add` | Agregar documento a memoria |
| `search(query, limit?, filters?)` | `POST /memory/search` | Búsqueda híbrida semántica + lexical |
| `retrieve(query, limit?, options?)` | `POST /memory/retrieve` | Retrieval multi-capa |
| `delete({id?, path?})` | `POST /memory/delete` | Eliminar por ID o path |

### Manejo de errores

```typescript
import { XavierClient } from '@iberi22/xavier';

const client = new XavierClient({ token: 'bad-token' });

// Error de autenticación (throw)
try {
  await client.stats();
} catch (err) {
  console.error(err.message); // → Xavier error: 401 Unauthorized
}

// Delete sin parámetros (throw)
try {
  await client.delete({});
} catch (err) {
  console.error(err.message);
  // → Xavier error: Either id or path must be provided for delete.
}

// Delete de ID inexistente (devuelve status, no throw)
const result = await client.delete({ id: 'nonexistent' });
console.log(result.status); // → not_found
```

## Timeout configuration

Por defecto las requests tienen un timeout de 30 segundos usando `AbortController`.
Puedes cambiarlo:

```typescript
const client = new XavierClient({ timeoutMs: 5000 }); // 5 segundos
```

Si el servidor no responde dentro del tiempo, la request se cancela con error.

## Ejemplos de uso real

### Pipeline típico

```typescript
import { XavierClient } from '@iberi22/xavier';

const client = new XavierClient();

async function pipeline() {
  // Guardar documentos
  const docs = [
    { content: 'Xavier usa SQLite con FTS5 y vectores embedding.', path: 'docs/search' },
    { content: 'El retrieval multi-capa combina working, episodic y semantic.', path: 'docs/layers' },
    { content: 'RRF fusion rankea resultados combinando scores.', path: 'docs/ranking' },
  ];
  for (const doc of docs) {
    const r = await client.add(doc);
    console.log(`✓ ${doc.path} → id=${r.id.slice(0, 8)}`);
  }

  // Buscar
  const results = await client.search('cómo funciona la búsqueda', 3);
  console.log(`\nResultados (${results.count} total):`);
  for (const doc of results.results) {
    console.log(`  ${doc.content.slice(0, 70)}...`);
  }
}

pipeline().catch(console.error);
```

### Health check

```typescript
import { XavierClient } from '@iberi22/xavier';

async function checkHealth(url = 'http://localhost:8006') {
  try {
    const client = new XavierClient({ baseUrl: url });
    const s = await client.stats();
    return { status: 'ok', version: s.version };
  } catch (err) {
    return { status: 'error', detail: (err as Error).message };
  }
}

checkHealth().then(console.log);
// → { status: 'ok', version: '0.4.1' }
```

## Features

- **Async-First**: `fetch` nativo + `AbortController` para timeouts — sin dependencias HTTP
- **Full TypeScript Types**: Interfaces completas para requests y responses
- **Auto-Auth**: Toma `XAVIER_TOKEN` de `process.env` automáticamente
- **Timeouts**: AbortController con 30s default, configurable
- **Delete gracefully**: 404 devuelve JSON de error, no exception
- **Layered Retrieval**: Acceso directo al retrieval multi-capa de Xavier
- **Node >=18**: Usa APIs modernas del runtime

## License

MIT
