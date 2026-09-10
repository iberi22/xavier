# ADR-NNNN — <Título corto de la decisión>

| Campo | Valor |
|-------|--------|
| **ID** | ADR-NNNN |
| **Estado** | Propuesto / Aceptado / Rechazado / Reemplazado por ADR-XXXX |
| **Fecha** | YYYY-MM-DD |
| **Autores** | |
| **Relacionados** | |

## Contexto

Qué problema existe, qué restricciones aplican (citar dirección vigente: GOAL, decisiones bloqueadas).

## Opciones que compiten

| Opción | Descripción | Coste/complejidad esperada |
|--------|-------------|----------------------------|
| A | | |
| B | | |

> Mínimo 2 opciones reales. Una ADR con una sola opción no es una decisión, es una descripción.

## Simulación (OBLIGATORIO)

> Ninguna decisión se acepta sin simulación multi-escenario. Ver skill `swal-adr-simulation`.

- **Motor:** `~/proyectosSWAL/periferia/swal-sim/adr_sim.py`
- **Comando:** `python3 adr_sim.py --model <model_id> --runs 5000 --seed 42 --outdir reports`
- **Runs / seed:** 5000 / 42
- **Resultado:** winner `<opcion>` (composite `X.XXX`) · primary-only winner `<opcion>`
- **¿Coinciden?** Sí/No — si NO, explicar por qué se decide con el composite y no con una sola métrica
- **Sensitivity:** ¿el ganador flipa al quitar un escenario? (bull/base/bear/worst) → decisión robusta: Sí/No
- **Reporte:** `periferia/swal-sim/reports/ADR-NNNN-<model>.md` (+ `.json`)

## Supuestos y su anclaje

| Parámetro | Valor usado | Fuente |
|-----------|-------------|--------|
| | | `docs/...:line` o **ASSUMPTION** |

> Un parámetro sin fuente es un supuesto. Etiquetarlo como tal es obligatorio.

## Decisión

Qué se decide, en una frase. Qué opción gana y con qué margen.

## Consecuencias

- **Positivas:**
- **Negativas / coste:**
- **Qué invalidaría esta decisión** (qué dato la revierte):

## Verificación posterior

Cómo se comprobará en la práctica que la decisión fue correcta (métrica observable + fecha de revisión).
