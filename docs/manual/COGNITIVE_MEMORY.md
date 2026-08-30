# Cognitive Memory & Hybrid Search Engine

Xavier implements a biological-inspired multi-tiered memory architecture designed to optimize latency, recall accuracy, and token efficiency for LLM agent swarms.

---

## 1. Memory Tiers

```
┌─────────────────────────────────────────────────────────────────┐
│                     AGENT PROMPT CONTEXT                        │
└───────────────────────────────▲─────────────────────────────────┘
                                │
   ┌────────────────────────────┼─────────────────────────────┐
   │                            │                             │
┌──┴───────────────┐ ┌──────────┴──────────────┐ ┌─────────────┴──────────┐
│  WORKING MEMORY  │ │    EPISTEMIC BELIEFS    │ │    EPISODIC MEMORY     │
│ (Active Context) │ │ (Confidence Graph/Facts)│ │ (Historical Trajectory)│
│  Bounded In-RAM  │ │     Entities & Edges    │ │  sqlite-vec Embeddings │
└──────────────────┘ └─────────────────────────┘ └────────────────────────┘
```

1. **Working Memory**: In-memory bounded cache preserving the immediate interaction window without database round-trip overhead.
2. **Epistemic Belief Graph**: Structured knowledge graph capturing facts, entities, claims, and agent confidence scores (`0.0` to `1.0`).
3. **Episodic Memory**: Persistent vector store indexed via `sqlite-vec` for semantic search across past sessions.
4. **Procedural Memory**: Reusable tool execution patterns, navigation policies, and reinforcement learning weights (HORMER GRPO).

---

## 2. Hybrid Search & Reciprocal Rank Fusion (RRF)

Xavier combines sparse keyword matching (BM25 / full-text SQLite indices) with dense vector embeddings using Reciprocal Rank Fusion:

$$RRF(d) = \sum_{m \in M} \frac{1}{k + r_m(d)}$$

Where $k = 60$, ensuring high semantic recall even when exact keyword terms differ.

### Memory Query Example
```bash
curl -X POST http://localhost:8006/v1/memories/search \
  -H "Authorization: Bearer $XAVIER_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "query": "how is the p2p transport encrypted?",
    "limit": 5,
    "mode": "hybrid"
  }'
```
