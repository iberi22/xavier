# Xavier Python SDK (xavier-py)

Professional-grade Python SDK for interacting with Xavier's high-performance memory API.

## Installation

```bash
pip install xavier-py
```

## Quickstart

### Synchronous Usage

```python
from xavier_py import XavierClient

# Auto-resolves XAVIER_TOKEN from environment
client = XavierClient(base_url="http://localhost:8080")

# Add a memory
client.add("Xavier is a high-performance memory engine.", path="docs/intro")

# Search
results = client.search("What is Xavier?", limit=5)
for doc in results.results:
    print(f"[{doc.path}] {doc.content}")
```

### Asynchronous Usage

```python
import asyncio
from xavier_py import XavierClient

async def main():
    client = XavierClient()

    # Add memory asynchronously
    await client.add_async("Episodic memory stores session history.", path="docs/episodic")

    # Retrieve across layers
    response = await client.retrieve_async("Tell me about episodic memory")
    print(f"Found {len(response.results)} results across {response.layers_used.total_results} total items.")

asyncio.run(main())
```

## Features

- **Sync & Async**: Built-in support for both `requests` and `aiohttp`.
- **Type Safety**: Fully validated responses using Pydantic models.
- **Auto-Auth**: Automatically picks up `XAVIER_TOKEN` from the environment.
- **Multi-layer Retrieval**: Easy access to Xavier's hybrid retrieval system.

## License

MIT
