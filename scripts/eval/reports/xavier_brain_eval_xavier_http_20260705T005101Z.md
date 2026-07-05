# Xavier Brain Eval — locomo_xavier_subagent_v1

- Fecha: 20260705T005101Z
- Modo: xavier_http
- Documentos sembrados: 0 (fallos: 0)
- Casos evaluados: 12
- Latencia promedio: 2958.7 ms
- Pass rate global: 0.917

## Resultados por caso

| Caso | Categoria | hit@1 | hit@3 | hit@5 | mrr | lat(ms) | pass |
|------|-----------|-------|-------|-------|-----|---------|------|
| single-hop-system3 | single_hop | 1 | 1 | 1 | 1.0 | 2223.2 | ✅ |
| single-hop-schema | single_hop | 1 | 1 | 1 | 1.0 | 2336.6 | ✅ |
| single-hop-rrf | single_hop | 1 | 1 | 1 | 1.0 | 2354.2 | ✅ |
| single-hop-iroh | single_hop | 1 | 1 | 1 | 1.0 | 2381.5 | ✅ |
| multi-hop-rrf-and-schema | multi_hop | - | - | - | 1.0 | 2213.5 | ✅ |
| temporal-most-recent-xavier | temporal | 1 | 1 | 1 | 1.0 | 2435.3 | ✅ |
| temporal-content-ops-latest | temporal | 1 | 1 | 1 | 1.0 | 2366.0 | ✅ |
| multilingual-es | multilingual | 1 | 1 | 1 | 1.0 | 2558.4 | ✅ |
| tenancy-isolate-xavier | tenancy | leak:0/0/0 | - | - | - | 4988.6 | ✅ |
| tenancy-isolate-content-ops | tenancy | leak:0/0/0 | - | - | - | 4559.2 | ✅ |
| agent-filter-content | tenancy | 0 | 0 | 0 | 0.0 | 4777.7 | ❌ |
| infra-migration | single_hop | 1 | 1 | 1 | 1.0 | 2309.9 | ✅ |

## Resumen por categoria

| Categoria | n | hit@1 | hit@3 | hit@5 | mrr | pass_rate | avg_lat(ms) |
|-----------|---|-------|-------|-------|-----|-----------|-------------|
| single_hop | 5 | 1.0 | 1.0 | 1.0 | 1.0 | 1.0 | 2321.08 |
| multi_hop | 1 | - | - | - | 1.0 | 1.0 | 2213.5 |
| temporal | 2 | 1.0 | 1.0 | 1.0 | 1.0 | 1.0 | 2400.65 |
| multilingual | 1 | 1.0 | 1.0 | 1.0 | 1.0 | 1.0 | 2558.4 |
| tenancy | 3 | 0.0 | 0.0 | 0.0 | 0.0 | 0.667 | 4775.167 |
