# Xavier TypeScript SDK (@iberi22/xavier)

Official, async-first TypeScript SDK for the Xavier Memory API.

## Installation

```bash
npm install @iberi22/xavier
```

## Quickstart

```typescript
import { XavierClient } from '@iberi22/xavier';

// Auto-resolves XAVIER_TOKEN from process.env (default port: 8006)
const client = new XavierClient();
// Or specify custom URL:
// const client = new XavierClient({ baseUrl: 'http://localhost:8006' });

async function main() {
  // Add a memory
  await client.add({
    content: 'Xavier implements multi-layer RRF fusion for high-precision retrieval.',
    path: 'tech/retrieval'
  });

  // Search
  const results = await client.search('How does Xavier retrieve?', 5);
  results.results.forEach(doc => {
    console.log(`[${doc.path}] ${doc.content}`);
  });
}

main().catch(console.error);
```

## Configuration

### Token

Set the `XAVIER_TOKEN` environment variable before using the client:

```bash
export XAVIER_TOKEN=your-xavier-token
```

> ⚠️ **Security**: Always set `XAVIER_TOKEN` in production. The client will log
> a warning if no token is provided.

### Default URL

The client defaults to `http://localhost:8006`, Xavier's standard port.
Override via `ClientOptions.baseUrl` for remote or custom deployments.

## Features

- **Async-First**: Built for modern Node.js and browser environments.
- **Full TypeScript Types**: Complete interfaces for all requests and responses.
- **Auto-Auth**: Automatically uses `XAVIER_TOKEN` from the environment if available.
- **Layered Retrieval**: Full support for Xavier's multi-layer memory architecture.
- **Type Safety**: `add()` returns `AddMemoryResponse` instead of `Promise<any>`.

## License

MIT
