# Parallel Execution Architecture — Xavier Ola 8+

## Problema

Hoy: 1 issue a la vez con label `jules`. Jules procesa secuencialmente porque:
- Todas las issues modifican `.gitcore/features.json` → conflicto garantizado
- No hay mapa de ownership de archivos automatizado
- El merge order es manual

## Diagnóstico: ¿Qué archivos comparten las features?

Del Feature Reality Scan + análisis de `implemented_in` de features.json:

| Archivo | Features que lo tocan | ¿Batchable? |
|---------|----------------------|-------------|
| `.gitcore/features.json` | **TODAS** (25/25) | ❌ No — reconciliación final |
| `src/lib.rs` | ~8 features (registro de módulos) | ⚠️ Secuencial |
| `Cargo.toml` | ~5 features (dependencias) | ⚠️ Secuencial |
| `src/memory/store.rs` | 4 features (storage backend) | ⚠️ Secuencial |
| `src/cli/commands/` | 3 features (CLI) | ✅ Paralelo si distinto archivo |
| `src/ports/` | 6 features (interfaces) | ✅ Paralelo (puertos independientes) |
| `src/search/` | 2 features (hybrid search) | ❌ Misma área |
| `src/security/` | 3 features | ✅ Paralelo si distinto submódulo |
| `src/memory/` | 7 features | ⚠️ Depende del submódulo |
| `docs/` | 2 features | ✅ Paralelo total |
| `panel-ui/` | 1 feature | ✅ Paralelo total |
| `code-graph/` | 1 feature | ✅ Paralelo total |

## Estrategia: Parallel Execution Framework

### Principio 1: features.json no se toca hasta el final

**Cada issue NO modifica features.json.** En su lugar:
1. El issue declara su progreso esperado en el body: `feat-xyz → 100%`
2. Al mergear el PR, se actualiza un archivo temporal: `.gitcore/ola8-progress.json`
3. Al final de la ola, una issue de **reconciliación** unifica TODO en features.json

Esto elimina el 100% de los conflictos de features.json.

### Principio 2: File Ownership Matrix Automatizada

```bash
# Script que analiza qué archivos toca cada feature
# Basado en implemented_in del features.json + code-graph AST
xavier verify ownership --output json
# → mapa de: feature_id → [files], con marcas de conflicto
```

Salida:
```json
{
  "feat-hybrid-search": {
    "files": ["src/search/", "src/retrieval/", "src/embedding/"],
    "conflicts_with": ["feat-context-regeneration"],
    "parallel_safe": true
  }
}
```

### Principio 3: Batch Assignment por File Island

Los issues se agrupan en **waves paralelas** basadas en file islands disjuntos:

```
Wave A (paralelo puro):
├── feat-hybrid-search     → src/search/
├── feat-telegram-bot      → src/telegram/       (distinto módulo)
├── feat-documentation-site → docs/site/          (distinto módulo)
└── feat-security-hygiene  → src/security/        (distinto módulo)

Wave B (paralelo puro):
├── feat-unified-storage   → src/memory/sqlite_vec_store/
├── feat-mcp-server        → src/server/mcp/      (distinto módulo)
└── feat-agent-cli-commands → src/cli/handlers/   (distinto módulo)
```

### Principio 4: Contract-First con Ports (donde aplica)

Donde hexagonal YA tiene un port trait definido, multiples features pueden implementar adapters en paralelo:

```rust
// Puerto ya definido en src/ports/inbound/ — no cambia
pub trait MemoryQueryPort {
    async fn search(&self, query: &Query) -> Result<Vec<Memory>>;
}

// Feature A implementa un adapter → archivo propio
// Feature B implementa OTRO adapter → archivo SEPARADO
// No hay conflicto porque los adapters son archivos distintos
```

**Pero solo donde el port existe.** No crear ports nuevos solo por paralelismo — eso fue el error de la hexagonal pura que generaba conectores innecesarios.

### Principio 5: Feature Flags para Safe Parallel Merge

```rust
// Cargo.toml — cada feature es un flag
[features]
default = []
feat-hybrid-search = []
feat-telegram-bot = []
feat-mesh-network = []

// Código — gated compilation
#[cfg(feature = "feat-hybrid-search")]
mod hybrid_search_impl;
```

Cada PR mergea su feature flag apagado (`default = []`). No rompe build. Se activan cuando todas las features de la wave están mergeadas.

## Problema Real: La Hexagonal Pura Creó Sobreingeniería

El análisis de arquitectura (sesión previa) reveló:

| Problema | Impacto |
|----------|---------|
| 11 port traits en inbound, 5 en outbound | Muchos para lo que realmente se usa |
| Domain module importa de infrastructure | Violación de DIP |
| AppState con code_graph raw (no port) | "Se moverá a ports en fase futura" — sigue sin moverse |
| MemoryStore trait con 18 métodos + bail!() default | Monolito, no desacoplado |

**Lo que funcionó:** Los ports donde HABÍA múltiples implementaciones (security con PromptGuard + Anticipator, memory con SQLite + Postgres).

**Lo que NO funcionó:** Crear ports para cosas con UNA sola implementación (HealthCheckPort, TimeMetricsPort, SchemaInit). Eso es sobreingeniería.

### Regla práctica para ports:

| Situación | Decisión |
|-----------|----------|
| 2+ implementaciones reales (o planificadas) | ✅ Port trait |
| 1 implementación y no cambiará | ❌ No crear port — llamar directo |
| 1 implementación pero cambiará en 6 meses | ⚠️ Crear port, pero sin adapter layer |
| Feature experimental | ❌ No crear port — refactorizar después |

## Implementación: Parallel Dispatch Tool

```bash
# Nuevo comando
xavier wave create --ola 8 \
  --issues "#847,#848,#849" \
  --parallel safe \
  --reconcile features.json

# Valida:
# 1. File ownership matrix → no conflicts
# 2. Cada issue tiene archivo(s) exclusivo(s)
# 3. features.json NO está en la lista de archivos
# 4. Asigna labels simultáneamente

# Uso
xavier wave dispatch --ola 8 --wave wave-1
# → Aplica label `jules` a TODOS los issues del wave simultáneamente
```

## Flujo Completo

```
Fase 1: DISEÑO (humano + agente de planificación)
├── Crear EPIC
├── Crear issues con file ownership explícito
├── NO incluir features.json en los archivos a modificar
└── Asignar wave y orden

Fase 2: DISPATCH PARALELO (automático)
├── `xavier wave dispatch --ola 8 --wave wave-1`
├── Valida file ownership matrix → sin conflictos
├── Aplica label a N issues simultáneamente
└── Jules los toma en paralelo

Fase 3: INTEGRACIÓN (automático + humano)
├── PRs llegan en orden arbitrario
├── Cada PR solo toca sus archivos → merge automático
├── Sin conflicts de features.json
└── CI verifica que nada se rompe

Fase 4: RECONCILIACIÓN (final)
├── `xavier wave reconcile --ola 8 --wave wave-1`
├── Unifica progreso de todas las features en features.json
├── Corre feature scan para medir mejora real
└── Cierra EPIC
```

## Ejemplo: Ola 8 Wave 1 Paralelo

Con el diseño actual, estos issues de Ola 8 son **paralelizables INMEDIATAMENTE** porque tocan archivos distintos:

| Issue | Archivos | Paralelo con |
|-------|----------|-------------|
| #847 Features Validation Sweep | `.gitcore/features.json` solo | ❌ Es la reconciliación |
| #848 feat-auto-improvement | `src/improvement/` | ✅ #849 |
| #849 feat-documentation-site | `docs/site/` | ✅ #848, #850 |
| #850 feat-local-first | `src/embedding/`, config | ✅ #848, #849 |

**Pero #847 no es paralelizable con nadie porque toca features.json.**  
Solución: convertir #847 en issue de reconciliación (Fase 4) y crear issues independientes para cada feature que NO toquen features.json.
