## Descripción
Agregar tests para los módulos críticos sin cobertura. Ver `docs/TEST_COVERAGE_GAPS.md` para el análisis completo.

## Módulos prioritarios

| Módulo | Cobertura actual | Prioridad |
|--------|-----------------|-----------|
| Agentes (sistema1, provider, runtime) | ~27% | 🔴 Crítica |
| `src/cli/` | ~30% | 🔴 Crítica |
| Coordinación (message_bus, agent_registry) | ~40% | 🟠 Media |
| Contexto (builder, manager, orchestrator) | ~35% | 🟠 Media |
| Seguridad (layers, scanner) | ~86% | 🟢 Mejora |

## Lo que ya existe
- `src/security/` tiene buena cobertura (86%) — usar como referencia
- `src/memory/` tiene tests de integración existentes

## Notas
- No modificar código de producción existente
- Solo agregar tests
- Usar `#[cfg(test)] mod tests {}` blocks dentro de cada archivo
- Priorizar tests de integración para mega-módulos
