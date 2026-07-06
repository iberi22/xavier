# Xavier Brain Eval — locomo_xavier_subagent_v1

- Fecha: 20260705T015903Z
- Modo: xavier_http
- Documentos sembrados: 0 (fallos: 0)
- Casos evaluados: 12
- Latencia promedio: 2893.6 ms
- Pass rate global: 0.917

## Resultados por caso

| Caso | Categoria | hit@1 | hit@3 | hit@5 | mrr | lat(ms) | pass |
|------|-----------|-------|-------|-------|-----|---------|------|
| single-hop-system3 | single_hop | 1 | 1 | 1 | 1.0 | 2345.5 | ✅ |
| single-hop-schema | single_hop | 1 | 1 | 1 | 1.0 | 2325.0 | ✅ |
| single-hop-rrf | single_hop | 1 | 1 | 1 | 1.0 | 2311.3 | ✅ |
| single-hop-iroh | single_hop | 1 | 1 | 1 | 1.0 | 2425.7 | ✅ |
| multi-hop-rrf-and-schema | multi_hop | - | - | - | 1.0 | 2202.6 | ✅ |
| temporal-most-recent-xavier | temporal | 1 | 1 | 1 | 1.0 | 2327.4 | ✅ |
| temporal-content-ops-latest | temporal | 1 | 1 | 1 | 1.0 | 2324.1 | ✅ |
| multilingual-es | multilingual | 1 | 1 | 1 | 1.0 | 2499.1 | ✅ |
| tenancy-isolate-xavier | tenancy | leak:0/0/0 | - | - | - | 4723.7 | ✅ |
| tenancy-isolate-content-ops | tenancy | leak:0/0/0 | - | - | - | 4621.6 | ✅ |
| agent-filter-content | tenancy | 0 | 0 | 0 | 0.0 | 4344.7 | ❌ |
| infra-migration | single_hop | 1 | 1 | 1 | 1.0 | 2272.4 | ✅ |

## Resumen por categoria

| Categoria | n | hit@1 | hit@3 | hit@5 | mrr | pass_rate | avg_lat(ms) |
|-----------|---|-------|-------|-------|-----|-----------|-------------|
| single_hop | 5 | 1.0 | 1.0 | 1.0 | 1.0 | 1.0 | 2335.98 |
| multi_hop | 1 | - | - | - | 1.0 | 1.0 | 2202.6 |
| temporal | 2 | 1.0 | 1.0 | 1.0 | 1.0 | 1.0 | 2325.75 |
| multilingual | 1 | 1.0 | 1.0 | 1.0 | 1.0 | 1.0 | 2499.1 |
| tenancy | 3 | 0.0 | 0.0 | 0.0 | 0.0 | 0.667 | 4563.333 |
