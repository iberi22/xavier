# Xavier TypeScript SDK (@iberi22/xavier)

Official, async-first TypeScript SDK for the Xavier Memory API.

## Installation

```bash
npm install @iberi22/xavier
```

## Quickstart

```typescript
import { XavierClient } from '@iberi22/xavier';

// Auto-resolves XAVIER_TOKEN from process.env
const client = new XavierClient({ baseUrl: 'http://localhost:8080' });

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

## Features

- **Async-First**: Built for modern Node.js and browser environments.
- **Full TypeScript Types**: Complete interfaces for all requests and responses.
- **Auto-Auth**: Automatically uses `XAVIER_TOKEN` from the environment if available.
- **Layered Retrieval**: Full support for Xavier's multi-layer memory architecture.

## License

MIT
