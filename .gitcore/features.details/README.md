# Feature Details — Xavier

Cada feature tiene su detalle en `features.details/{feature_id}.json` siguiendo el estándar GitCore.

## Formato

```json
{
  "feature": "feat-hybrid-search",
  "name": "Hybrid Search Engine",
  "category": "search",
  "status": "stable",
  "progress_pct": 100,
  "score": 89.0,
  "steps": {
    "architecture_clean_layers": { "passed": true, "weight": 25, "score": 25 },
    "tests_defined":            { "passed": true, "weight": 30, "score": 30 },
    "implementation_complete":  { "passed": true, "weight": 20, "score": 20 },
    "documentation_updated":    { "passed": true, "weight": 10, "score": 10 },
    "ci_cd_workflow":           { "passed": true, "weight": 5,  "score": 5  },
    "xavier_memory_integration":{ "passed": true, "weight": 5,  "score": 5  },
    "feature_type_specific":    { "passed": true, "weight": 5,  "score": 5  },
    "sandbox_rules":            { "passed": true, "weight": 2,  "score": 2  },
    "cross_agent":              { "passed": true, "weight": 1,  "score": 1  },
    "planning_task":            { "passed": true, "weight": 1,  "score": 1  },
    "issue_tracked":            { "passed": false,"weight": 1,  "score": 0  }
  }
}
```

## Archivos

- `features-detailed.json` → Índice maestro con todas las 22 features
- `features.details/{id}.json` → Detalle individual por feature (score, steps, weights)
- `.gitcore/scripts/validate-feature-{id}.sh` → Scripts de validación

## Validation Scripts

```bash
# Validar un feature específico
bash .gitcore/scripts/validate-feature-feat-hybrid-search.sh

# Validar todos
for f in .gitcore/scripts/validate-*.sh; do bash "$f"; done
```
