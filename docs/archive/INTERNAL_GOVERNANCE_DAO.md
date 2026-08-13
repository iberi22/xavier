# Xavier Internal Governance — Governance Interna de Xavier

**Versión:** 1.0.0-22-06-2026
**Propósito:** Sistema de gobernanza para mantenimiento, roadmap y mejora continua de Xavier
**NO** incluye economía del mesh (ver WHITEPAPER_SOVEREIGN_MESH.md para tokenomics)

---

## Filosofía

Xavier no es solo código — es un sistema de memoria vivo que evoluciona con cada interacción.
La gobernanza interna permite que **personas** (mantenedores) decidan su dirección mientras
que el sistema mismo aprende de esas decisiones para mejorar sus modelos internos.

**Separación clara:**
- Este documento = Gobernanza del proyecto Xavier (roadmap, mantenimiento, fine-tuning)
- WHITEPAPER_SOVEREIGN_MESH.md = Economía de la red mesh (bonding curves, APY, staking)

---

## 1. Arquitectura General: El Ciclo VIRTUOSO

```
┌─────────────────────────────────────────────────────────┐
│                    XAVIER INTERNAL DAO                    │
│                                                          │
│  ┌──────────┐    ┌──────────┐    ┌──────────────────┐   │
│  │ HUMANOS  │───>│ TAREAS   │───>│ DATA LAKES       │   │
│  │ (Nodos)  │    │ (Issues) │    │ (Memorias crudas)│   │
│  └──────────┘    └──────────┘    └──────────────────┘   │
│       │               │                   │              │
│       │               │                   ▼              │
│       │               │          ┌──────────────────┐   │
│       │               └──────────│ PATRONES          │   │
│       │                          │ (Análisis + TGD)  │   │
│       │                          └──────────────────┘   │
│       │                               │                  │
│       ▼                               ▼                  │
│  ┌──────────┐              ┌──────────────────┐         │
│  │ PAGOS    │              │ FINE-TUNING      │         │
│  │ (Tokens) │              │ + Embeddings      │         │
│  └──────────┘              └──────────────────┘         │
│                                                          │
│  ↺ El feedback loop de cada tarea alimenta el sistema   │
└─────────────────────────────────────────────────────────┘
```

---

## 2. Roles en el Sistema

### 2.1 Tipos de Nodos

| Rol | Descripción | Acceso a datos internos | Pago |
|-----|-------------|------------------------|------|
| **Mantenedor** | Humano que ejecuta tareas de mantenimiento | ✅ Solo lo necesario para su tarea | Tokens XAV |
| **Validador** | Nodo que revisa tareas completadas | ✅ Solo la tarea asignada para revisión | Tokens XAV |
| **Contribuidor de Datos** | Aporta recursos (cómputo, almacenamiento) para el mesh | ❌ No | Tokens XAV (vía tokenomics) |
| **Consumidor** | Usa servicios del mesh | ❌ No | Paga en tokens |
| **Guardián** | Miembro del consejo de gobernanza (elegido) | ✅ Nivel completo según su rol | Salary en tokens |

### 2.2 Ciclo de Vida de un Mantenedor

```
Registro → Aporte inicial (stake) → Nivel de Confianza Inicial
  → Recibe Tareas → Ejecuta → Envía para Validación
  → Validadores Revisan → Feedback + Puntaje
  → Si aprueba: Pago en tokens + Sube Confianza
  → Si rechaza: Feedback + Baja Confianza + Penalidad
  → Datos de la tarea → Data Lake → Patrones → Fine-tuning
```

---

## 3. Sistema de Confianza (Karma / Trust Score)

### 3.1 Cálculo del Trust Score

```
TrustScore = (w_1 * TareasExitosas) + (w_2 * CalidadPromedio) 
           + (w_3 * Antigüedad) + (w_4 * ValidacionesAcertadas)
           - (w_5 * TareasFallidas) - (w_6 * ReportesNegativos)
```

Donde:
- **w_1** = 0.30 (peso de cantidad de tareas completadas)
- **w_2** = 0.25 (peso de calidad del trabajo)
- **w_3** = 0.15 (peso de tiempo en el sistema)
- **w_4** = 0.20 (peso de precision como validador)
- **w_5** = 0.05 (penalidad por tareas fallidas)
- **w_6** = 0.05 (penalidad por reportes negativos de otros)

### 3.2 Niveles de Confianza

| Nivel | TrustScore | Privilegios |
|-------|-----------|-------------|
| 🟢 Novato | 0-250 | Solo tareas simples, 1 validador requerido |
| 🟡 Aprendiz | 251-500 | Tareas medias, 1 validador |
| 🟠 Regular | 501-700 | Tareas complejas, 2 validadores |
| 🔵 Experto | 701-900 | Tareas críticas, puede validar a otros |
| 🟣 Guardian | 901-1000 | Acceso a datos sensibles, voto en consejo |
| 🏆 Legendario | 1000+ | Peso de voto amplificado, acceso total |

### 3.3 TrustScore como Token Soulbound

El TrustScore es **no transferible** (soulbound) y está ligado al perfil del nodo.
No se puede comprar, solo ganar con trabajo.

```
TrustScoreNoTransferible = hash(node_id + trust_score) 
                         = SBT (Soulbound Token)
```

---

## 4. Sistema de Tareas de Mantenimiento

### 4.1 Tipos de Tareas

| Tipo | Descripción | Pago (en XAV) | Validadores Requeridos |
|------|-------------|---------------|----------------------|
| **Bug Fix** | Reparar errores en código Xavier | 50-200 | 2 |
| **Feature** | Implementar nueva funcionalidad | 100-500 | 2-3 |
| **Fine-Tuning** | Entrenar/ajustar modelos internos | 200-1000 | 3 |
| **Data Curation** | Limpiar y etiquetar datos para data lakes | 20-100 | 1 |
| **Embedding Tuning** | Ajustar lógicas de embeddings | 100-400 | 2 |
| **Memory Audit** | Revisar consistencia de memorias | 50-150 | 2 |
| **Security Review** | Auditoría de seguridad | 150-600 | 3 |
| **Documentation** | Actualizar documentación técnica | 30-100 | 1 |
| **Model Evaluation** | Evaluar rendimiento de modelos | 100-300 | 2 |
| **Governance** | Participar en votaciones del DAO | 10-50 | N/A |

### 4.2 Ciclo de Vida de una Tarea

```
┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐
│ CREADA   │──>│ ASIGNADA │──>│ EN CURSO │──>│ REVISIÓN │
│ (Issue)  │   │ (Match)  │   │ (Work)   │   │ (Audit)  │
└──────────┘   └──────────┘   └──────────┘   └──────────┘
                                                  │
                                           ┌──────┴──────┐
                                           ▼             ▼
                                      ┌──────────┐ ┌──────────┐
                                      │ APROBADA │ │ RECHAZADA│
                                      │ + Pago   │ │Feedback  │
                                      └──────────┘ └──────────┘
                                           │
                                           ▼
                                    ┌──────────────┐
                                    │ DATA LAKE    │
                                    │ (Feedback    │
                                    │  loop)       │
                                    └──────────────┘
```

### 4.3 Algoritmo de Asignación (Matchmaking)

Las tareas se asignan automáticamente según:
1. **TrustScore del mantenedor** — debe ser >= nivel requerido
2. **Historial exitoso** en tareas similares
3. **Disponibilidad actual** (no más de N tareas simultáneas)
4. **Especialización** (campos donde ha trabajado antes)
5. **Precio** (si hay múltiples candidatos, el que mejor relación calidad/precio tenga)

```
MatchScore = TrustScore * 0.4 + SimilarityScore * 0.3 
           + AvailabilityFactor * 0.2 + PriceEfficiency * 0.1
```

---

## 5. Validación Entre Pares (Peer Validation)

### 5.1 Proceso

1. Mantenedor completa la tarea y sube evidencia (PR, datos, reporte)
2. Sistema asigna validadores automáticamente (mínimo N según tipo de tarea)
3. Validadores revisan y emiten voto: ✅ Aprobar / ❌ Rechazar / ⏸️ Solicitar cambios
4. Si todos aprueban → tarea completada, pago liberado
5. Si hay rechazo → feedback al mantenedor, oportunidad de corregir
6. Si 2 rechazos seguidos en misma tarea → penalidad + baja de confianza

### 5.2 Validación Cruzada con "Juez Final"

Para tareas críticas donde los validadores no llegan a consenso:
```
Si ValidadoresAprobar = ValidadoresRechazar:
    Se asigna un "Juez Final" (mantenedor con TrustScore > 800)
    Su voto decide
    El juez también recibe pago por su decisión
```

### 5.3 Recompensas para Validadores

Los validadores reciben pago por cada revisión:
- **Pago base:** 10% del valor de la tarea
- **Bono de precisión:** Si su voto coincide con el resultado final, +5%
- **Penalidad:** Si consistentemente vota mal, baja su TrustScore de validador

---

## 6. Token Gating y Acceso a Datos Internos (Data Commons)

### 6.1 Principio

> "Cada nodo solo ve lo que necesita para su trabajo. Nada más."

El mesh tiene **datos internos sensibles** (memorias, decisiones, embeddings, configuraciones).
Ningún nodo puede acceder sin los tokens o claves necesarias.

### 6.2 Niveles de Acceso

| Nivel | Acceso | Requisito |
|-------|--------|-----------|
| **🔒 Público** | Documentación, whitepapers, código abierto | Ninguno |
| **🔒 Nivel 1** | Issues asignadas, datos de tarea específica | Ser mantenedor + tener la tarea asignada |
| **🔒 Nivel 2** | Memorias de sesiones recientes | TrustScore > 500 |
| **🔒 Nivel 3** | Data lakes de entrenamiento | TrustScore > 700 |
| **🔒 Nivel 4** | Embeddings y configuraciones de modelos | TrustScore > 900 o ser Guardián |
| **🔒 Nivel 5** | Claves privadas, wallets, datos críticos | Solo consejo Guardianes, multisig |

### 6.3 Mecanismo de Token Gating

```rust
// Pseudocódigo del sistema de gating
fn check_access(node: &Node, resource: &Resource) -> bool {
    match resource.level {
        0 => true, // Público
        1 => node.has_task_assigned(resource.task_id),
        2 => node.trust_score >= 500,
        3 => node.trust_score >= 700,
        4 => node.trust_score >= 900 || node.is_guardian,
        5 => multisig_approval(node, resource), // Requiere 3/5 Guardianes
    }
}
```

### 6.4 Desbloqueo Temporal

Para tareas que requieren acceso a datos de nivel superior al del mantenedor:
```
TemporaryAccess = {
    node_id: "xyz",
    resource: "data_lake_v3",
    level: 4,
    expires_at: timestamp + 48_hours, // Solo 48h
    task_id: "issue_123",
    signed_by: guardian_wallet
}
```

Esto permite que mantenedores de nivel Regular (TrustScore 501-700) puedan hacer
tareas que requieran datos de Nivel 3, con supervisión de un Guardián.

---

## 7. Data Lakes Internos y Extracción de Patrones

### 7.1 El Ciclo de Aprendizaje

Cada tarea que se completa genera datos que alimentan el sistema:

```
Tarea Completada
    │
    ├──> Código/PR mergeado → Mejora del código base
    ├──> Decisiones registradas → Data Lake de Decisiones
    ├──> Feedback de revisión → Data Lake de Calidad
    ├──> Tiempo/recursos usados → Data Lake de Eficiencia
    └──> Patrones identificados → TGD (Textual Gradient Descent)
```

### 7.2 Data Lakes

| Data Lake | Contenido | Uso |
|-----------|-----------|-----|
| **Decisiones** | Cada voto, cada PR mergeado, cada decisión del DAO | Entrenar modelos de priorización |
| **Calidad** | Ratings de validadores, feedback, errores comunes | Ajustar thresholds de calidad |
| **Eficiencia** | Tiempo por tarea, recursos consumidos, bottlenecks | Optimizar asignación de tareas |
| **Patrones** | Patrones de pensamiento extraídos de discusiones | Fine-tuning de modelos internos |
| **Memorias** | Interacciones, queries, respuestas | Mejorar embeddings y búsqueda |

### 7.3 Extracción Automática de Patrones (TGD + HORMER)

El sistema usa TGD (Textual Gradient Descent) y HORMER para:
1. **Analizar data lakes** periódicamente (consolidación nocturna)
2. **Identificar patrones** de decisiones exitosas vs fallidas
3. **Generar insights** que mejoran las lógicas deterministas de Xavier
4. **Ajustar embeddings** basado en qué términos aparecen juntos en decisiones buenas
5. **Reforzar comportamientos** que llevan a resultados positivos

```
Patrón identificado: "Issues con más de 3 comentarios antes de merge
tienen 40% más probabilidad de tener bugs post-merge"

→ Acción: Sistema ajusta threshold para requerir mínimo 3 comentarios
→ Embedding: "merge rápido" se asocia con "riesgo alto"
→ Fine-tuning: Modelo interno prioriza revisión exhaustiva
```

### 7.4 Fine-Tuning Personal (Per-Model Tuning)

Cada mantenedor tiene un perfil de trabajo. Sus datos permiten:
1. **Fine-tuning del modelo** que usa para sus tareas
2. **Ajuste de embeddings** según su especialización
3. **Recomendaciones personalizadas** de tareas que se le dan bien

---

## 8. Ciclo de Recompensas (Pagos en Tokens)

### 8.1 Mecanismo de Pago

```
Tarea completada → Validadores aprueban → Pago liberado:
    70% → Mantenedor (por ejecutar la tarea)
    20% → Validadores (dividido entre ellos)
    5%  → Treasury (para sostenibilidad del DAO)
    5%  → Quemado (deflación)
```

### 8.2 Bono por Calidad

```
BonusQuality = PagoBase * (0.5 * (TrustScoreMantenedor / 1000) 
                          + 0.5 * (CalificacionValidadores / 10))
```

### 8.3 Penalidad por Fracaso

Si una tarea es rechazada por los validadores:
```
Penalidad = PagoBase * 0.1 * (1 - TrustScore / 1000)
1ra vez: Advertencia + -10 TrustScore
2da vez seguida: -25 TrustScore + -5% pago futuro
3ra vez seguida: Suspensión temporal + revisión del consejo
```

---

## 9. Gobernanza del Roadmap

### 9.1 Tipos de Decisiones

| Tipo | Quórum | Mayoría | Timelock |
|------|--------|---------|----------|
| **Tareas técnicas** (bugs, features) | 3 votos | Simple (>50%) | 24h |
| **Parámetros del DAO** (thresholds, pagos) | 30% del poder de voto | 60% | 72h |
| **Cambios en TrustScore weights** | 40% | 66% | 5 días |
| **Data lake access policies** | 50% | 75% | 7 días |
| **Constitucionales** (cambios en el DAO) | 60% | 80% | 14 días |

### 9.2 Peso de Voto en Decisiones de Roadmap

```
VotingPower = TokensStakeados × (1 + log₂(1 + TrustScore))
```

Esto asegura que mantenedores con alta confianza tengan más peso,
pero no dominen solo por tener más tokens.

### 9.3 Propuestas de Roadmap

Cualquier mantenedor con TrustScore > 300 puede crear una propuesta.
Las propuestas pasan por:
1. **Discusión** (7 días) — comentarios y refinamiento
2. **Votación** (3-14 días según tipo)
3. **Implementación** — asignada vía matchmaking

---

## 10. Implementación Técnica

### 10.1 Módulos Rust

```
src/
├── mesh/
│   ├── governance.rs        ← YA EXISTE (expandir)
│   ├── tokenomics/          ← YA EXISTE (#268)
│   │   ├── economy.rs
│   │   ├── rewards.rs
│   │   ├── wallet.rs
│   │   └── vesting.rs
│   ├── internal_dao/        ← NUEVO
│   │   ├── mod.rs           → Re-exportar
│   │   ├── trust_score.rs   → TrustScore + niveles + SBT
│   │   ├── tasks.rs         → Task lifecycle + matchmaking
│   │   ├── validation.rs    → Peer validation + jueces
│   │   ├── gating.rs        → Token gating + access control
│   │   ├── data_lakes.rs    → Data lakes + pattern extraction
│   │   ├── rewards.rs       → Payment distribution
│   │   └── tests.rs         → Tests de integración
│   └── ...
```

### 10.2 Interfaces EVM (Solidity)

```solidity
contracts/internal/
├── TrustScoreRegistry.sol    // TrustScore soulbound tracking
├── TaskBoard.sol             // Task lifecycle on-chain
├── ValidationOracle.sol      // Peer validation results
├── AccessGate.sol            // Token gating levels
├── InternalGovernance.sol    // Roadmap voting
└── RewardDistributor.sol     // Automatic payment release
```

### 10.3 Integración con Wallets Existentes

El módulo `wallet.rs` ya tiene `InvestmentTier` y `WalletBalance`.
Hay que agregar:
```rust
pub struct MaintainerProfile {
    pub node_id: NodeId,
    pub trust_score: u64,
    pub level: TrustLevel,
    pub completed_tasks: u64,
    pub successful_validations: u64,
    pub specialization: Vec<TaskCategory>,
    pub current_tasks: Vec<String>, // Task IDs
    pub total_earned: u64,
    pub penalty_count: u64,
}
```

---

## 11. Conexión con Tokenomics Existentes (#268)

El sistema de gobernanza interna **usa la infraestructura de tokenomics** pero no es lo mismo:

| Concepto | Tokenomics (#268) | Internal DAO (este doc) |
|----------|-------------------|------------------------|
| Tokens | XAV (transferible, mercado) | TrustScore (soulbound, no transferible) |
| Incentivo | APY, staking, bonding curve | Pagos por tareas, reputación |
| Participación | Cualquiera con tokens | Solo mantenedores aprobados |
| Riesgo | Pérdida de capital | Pérdida de reputación |
| Gobierno | Mesh económico | Roadmap técnico de Xavier |
| Datos | Públicos (precios, liquidez) | Privados (memorias, modelos) |

### 11.1 Flujo de Tokens entre Sistemas

```
Tokenomics (#268)              Internal DAO
    │                              │
    │  Staking ──────────────────> │  Para ser mantenedor, debes
    │                              │  tener XAV en stake (seguridad)
    │                              │
    │  <──────────────────── Pago  │  Tareas completadas pagan XAV
    │                              │  (se mintean nuevos o del treasury)
    │                              │
    │  Rewards ──────────────────> │  Validadores reciben XAV
    │                              │
    │  <──────────────────── Burn  │  5% de cada pago se quema
    │                              │
```

---

## 12. Referencias Externas

Este diseño se inspira en:

| Proyecto | Concepto usado |
|----------|---------------|
| **Bittensor** | Subnets + validadores que evalúan mineros, Yuma Consensus |
| **Ocean Protocol** | Data DAOs, token gating, data farming, recompensas por GPU |
| **Gitcoin** | Bounties + Gitcoin Passport (identidad descentralizada) |
| **Karma (Web3)** | Sistemas de reputación como ledger distribuido |
| **Soulbound Tokens (Vitalik)** | TrustScore no transferible |

---

*Este documento es un diseño vivo. Se actualizará con cada iteración de la implementación.*
