# Quick Start Guide

## Installation

### From Source

```bash
git clone https://github.com/iberi22/xavier.git
cd xavier
cargo build --release
./target/release/xavier http
```

### From Binary

Download from GitHub Releases for your platform.

### Docker

```bash
docker run -p 8006:8006 ghcr.io/iberi22/xavier:latest
```

## First Steps

1. **Start the server:**
   ```bash
   xavier http
   ```

2. **Add your first memory:**
   ```bash
   xavier add "Hello Xavier!" "First Memory"
   ```

3. **Search:**
   ```bash
   xavier search "hello"
   ```

4. **Check stats:**
   ```bash
   xavier stats
   ```

## Next Steps

- [CLI Reference](./CLI_REFERENCE.md) - Full CLI documentation
- [API Reference](../api/README.md) - HTTP API details
- [MCP Integration](./MCP_INTEGRATION.md) - Connect to AI clients
