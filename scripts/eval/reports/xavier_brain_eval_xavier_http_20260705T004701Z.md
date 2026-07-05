# Xavier Brain Eval — locomo_xavier_subagent_v1

- Fecha: 20260705T004701Z
- Modo: xavier_http
- Documentos sembrados: 16 (fallos: 0)
- Casos evaluados: 12
- Latencia promedio: 4835.9 ms
- Pass rate global: 0.167

## Resultados por caso

| Caso | Categoria | hit@1 | hit@3 | hit@5 | mrr | lat(ms) | pass |
|------|-----------|-------|-------|-------|-----|---------|------|
| single-hop-system3 | single_hop | 0 | 0 | 0 | 0.0 | 5287.2 | ❌ |
| single-hop-schema | single_hop | 0 | 0 | 0 | 0.0 | 4819.3 | ❌ |
| single-hop-rrf | single_hop | 0 | 0 | 0 | 0.0 | 5462.9 | ❌ |
| single-hop-iroh | single_hop | 0 | 0 | 0 | 0.0 | 4713.7 | ❌ |
| multi-hop-rrf-and-schema | multi_hop | - | - | - | 0.0 | 4756.7 | ❌ |
| temporal-most-recent-xavier | temporal | 0 | 0 | 0 | 0.0 | 4923.6 | ❌ |
| temporal-content-ops-latest | temporal | 0 | 0 | 0 | 0.0 | 4597.2 | ❌ |
| multilingual-es | multilingual | 0 | 0 | 0 | 0.0 | 4667.7 | ❌ |
| tenancy-isolate-xavier | tenancy | leak:0/0/0 | - | - | - | 4456.6 | ✅ |
| tenancy-isolate-content-ops | tenancy | leak:0/0/0 | - | - | - | 4600.7 | ✅ |
| agent-filter-content | tenancy | 0 | 0 | 0 | 0.0 | 4775.3 | ❌ |
| infra-migration | single_hop | 0 | 0 | 0 | 0.0 | 4969.9 | ❌ |

## Resumen por categoria

| Categoria | n | hit@1 | hit@3 | hit@5 | mrr | pass_rate | avg_lat(ms) |
|-----------|---|-------|-------|-------|-----|-----------|-------------|
| single_hop | 5 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 5050.6 |
| multi_hop | 1 | - | - | - | 0.0 | 0.0 | 4756.7 |
| temporal | 2 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 4760.4 |
| multilingual | 1 | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 4667.7 |
| tenancy | 3 | 0.0 | 0.0 | 0.0 | 0.0 | 0.667 | 4610.867 |
