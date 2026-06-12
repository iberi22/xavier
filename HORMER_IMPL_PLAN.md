# HORMER Implementation Plan for Xavier

> Based on HORMER (Hierarchical Memory Navigation for Efficient Agents) by Duke University + Snowflake AI Research, 2026

## Score de alineación actual: ~40%

| Feature | Score | Status |
|---------|-------|--------|
| Multi-layer memory | 85% | ✅ Ya implementado |
| Entity/Knowledge Graph | 70% | ✅ Ya implementado |
| Consolidation/Decay | 75% | ✅ Ya implementado |
| Hybrid Search | 65% | ✅ Ya implementado |
| **Hierarchical Directories** | **0%** | ❌ **Issue #20** |
| **Navigation Policy** | **0%** | ❌ **Issue #22** |
| **Textual Gradient Descent** | **5%** | ❌ **Issue #23** |
| **GRPO Simplified RL** | **0%** | ❌ **Issue #24** |
| **Nav Commands (API+CLI)** | **0%** | ❌ **Issue #25** |
| **Nav-aware Consolidation** | **0%** | ❌ **Issue #26** |

## Issues y PRs

| Issue | Descripción | PR | Status |
|-------|-------------|----|--------|
| #20 | F1 - Jerarquía dinámica de directorios | — | 🔵 Abierto (jules) |
| #22 | F2 - Política de navegación con scoring | — | 🔵 Abierto (jules) |
| #23 | F3 - Textual Gradient Descent | — | 🔵 Abierto (jules) |
| #24 | F4 - GRPO simplificado | — | 🔵 Abierto (jules) |
| #25 | F5 - Comandos shell (API+CLI) | — | 🔵 Abierto (jules) |
| #26 | F6 - Consolidación navigation-aware | — | 🔵 Abierto (jules) |

## Pipeline cíclico

```powershell
# Jules crea PRs automáticamente desde los issues con label jules
# Pipeline de integración detecta PRs listos, revisa y mergea
# Revisión profunda cada ~3-5 ciclos

# Ejecutar pipeline manual:
powershell -File C:\Users\belal\.openclaw\skills\jules-integration\scripts\integrate.ps1 -Repo iberi22/xavier -Merge

# Ver estado de PRs:
powershell -File C:\Users\belal\.openclaw\skills\jules-integration\scripts\check-prs.ps1 -Repo iberi22/xavier
```

## Orden recomendado
1. F1 (#20) → Directorios (sin esto nada funciona)
2. F2 (#22) → Navigation Policy (usa directorios)
3. F5 (#25) → Comandos shell (expone directorios + policy)
4. F6 (#26) → Consolidación nav-aware (usa directorios)
5. F3 (#23) → Gradient Descent (independiente de los de arriba)
6. F4 (#24) → GRPO simplified (usa policy + gradient)
