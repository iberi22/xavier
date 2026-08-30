# Xavier User Manual & Feature Documentation

Welcome to the official **Xavier User Manual**. Xavier is a high-performance, decentralized cognitive memory runtime and enterprise mesh network built in Rust for autonomous AI agents and collaborative engineering swarms.

---

## 📚 Manual Modules

1. **[Getting Started & Installation](GETTING_STARTED.md)**
   - Binary installation, cargo crates, Docker containers, systemd service setup, and initial CLI configuration.
2. **[Cognitive Memory & Hybrid Search](COGNITIVE_MEMORY.md)**
   - Multi-tiered memory architecture (Working, Epistemic, Episodic, Procedural), `sqlite-vec` embeddings, Reciprocal Rank Fusion (RRF), and belief graph reasoning.
3. **[Enterprise Mesh & Decentralized Sync](ENTERPRISE_MESH.md)**
   - P2P synchronization, ed25519 keypair identity, RBAC access clearance, wallet-gating, read-once ephemeral passes, and Tor/ICE NAT traversal.
4. **[Model Context Protocol (MCP) Integration](MCP_INTEGRATION.md)**
   - Connecting Claude Desktop, Cursor, OpenCode, Hermes, VSCode, and custom agent runtimes to Xavier's contextual memory stream.
5. **[AST Code Graph & Semantic Navigation](CODE_GRAPH.md)**
   - Zero-overhead symbol indexing, call-graph traversal, implementation mapping, and cross-repo dependency intelligence.
6. **[HTTP REST & WebSocket API Reference](API_REFERENCE.md)**
   - Exhaustive specification of all `/v1/*` endpoints, JSON schemas, headers, authentication tokens, and event streams.

---

## ⚡ Quick Reference

```bash
# Start Xavier daemon in HTTP + MCP mode
xavier http --port 8006

# Query cognitive memory via CLI
xavier memory search --query "mesh sync protocol" --limit 5

# Inspect node status and peer mesh
xavier mesh status
```
