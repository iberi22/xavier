# Xavier Memory System

This file documents the long-term memory architecture and indexing strategy for Xavier.

## Hierarchical Memory
Xavier uses a hierarchical memory structure:
- **L0 (Ephemeral)**: Current session context.
- **L1 (Short-term)**: Recent interactions and tool outputs.
- **L2 (Long-term)**: Durable knowledge and architectural decisions.

## Indexing
Memory is indexed using:
- **BM25**: For lexical search.
- **SQLite-Vec**: For semantic vector search.
- **Belief Graph**: For deterministic relationship mapping.

## Maintenance
- **Precompact**: Runs before reaching token limits to summarize context.
- **Periodic Indexing**: Consolidates ephemeral memories into the durable layer.
