## Descripción
Implementar cron-validator en el pipeline CI para validar expresiones cron y job schedules.

## Contexto
El proyecto usa cron jobs para scheduling. Sin validación automática, expresiones cron inválidas pueden pasar desapercibidas hasta producción.

## Archivos objetivo
- `.github/workflows/` (crear workflow nuevo)
- Usar el skill cron-validator del workspace

## Criterios de Aceptación
- [ ] Workflow CI valida todas las expresiones cron en el repo
- [ ] Falla con error claro si hay expresión inválida
- [ ] Corre en PRs automáticamente
