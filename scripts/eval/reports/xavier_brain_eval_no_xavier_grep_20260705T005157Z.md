# Xavier Brain Eval — locomo_xavier_subagent_v1

- Fecha: 20260705T005157Z
- Modo: no_xavier_grep
- Documentos sembrados: 0 (fallos: 0)
- Casos evaluados: 12
- Latencia promedio: 0.0 ms
- Pass rate global: 1.0

## Resultados por caso

| Caso | Categoria | hit@1 | hit@3 | hit@5 | mrr | lat(ms) | pass |
|------|-----------|-------|-------|-------|-----|---------|------|
| single-hop-system3 | single_hop | 0 | 1 | 1 | 0.5 | 0.0 | ✅ |
| single-hop-schema | single_hop | 1 | 1 | 1 | 1.0 | 0.0 | ✅ |
| single-hop-rrf | single_hop | 1 | 1 | 1 | 1.0 | 0.0 | ✅ |
| single-hop-iroh | single_hop | 1 | 1 | 1 | 1.0 | 0.0 | ✅ |
| multi-hop-rrf-and-schema | multi_hop | - | - | - | 1.0 | 0.0 | ✅ |
| temporal-most-recent-xavier | temporal | 1 | 1 | 1 | 1.0 | 0.0 | ✅ |
| temporal-content-ops-latest | temporal | 1 | 1 | 1 | 1.0 | 0.0 | ✅ |
| multilingual-es | multilingual | 1 | 1 | 1 | 1.0 | 0.0 | ✅ |
| tenancy-isolate-xavier | tenancy | leak:0/0/0 | - | - | - | 0.0 | ✅ |
| tenancy-isolate-content-ops | tenancy | leak:0/0/0 | - | - | - | 0.0 | ✅ |
| agent-filter-content | tenancy | 1 | 1 | 1 | 1.0 | 0.0 | ✅ |
| infra-migration | single_hop | 1 | 1 | 1 | 1.0 | 0.0 | ✅ |

## Resumen por categoria

| Categoria | n | hit@1 | hit@3 | hit@5 | mrr | pass_rate | avg_lat(ms) |
|-----------|---|-------|-------|-------|-----|-----------|-------------|
| single_hop | 5 | 0.8 | 1.0 | 1.0 | 0.9 | 1.0 | 0.0 |
| multi_hop | 1 | - | - | - | 1.0 | 1.0 | 0.0 |
| temporal | 2 | 1.0 | 1.0 | 1.0 | 1.0 | 1.0 | 0.0 |
| multilingual | 1 | 1.0 | 1.0 | 1.0 | 1.0 | 1.0 | 0.0 |
| tenancy | 3 | 1.0 | 1.0 | 1.0 | 1.0 | 1.0 | 0.0 |
