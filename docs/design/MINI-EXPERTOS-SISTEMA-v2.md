# Mini-expertos v2 — diseño del sistema completo

Estado: PROPUESTA (2026-10-02). Sustituye la parte operativa de
`docs/ARCHITECTURE/SLM_MICRO_EXPERTS_DATALAKE.md`; la visión sigue en
`docs/design/F12-PRESERVACION-MINI-EXPERTOS.md`.

## 1. Objetivo

Cada usuario de Xavier entrena modelos pequeños (0.5B–3B, GGUF Q4) expertos en
1–2 tareas a partir de **sus** memorias, su introspección y su operación. Deben
cargar en segundos en VRAM modesta (8 GB) y responder rápido. El ciclo lo
orquestan agentes (no determinista: eligen dominio, revisan muestras, deciden si
promover) o un job nocturno; las **puertas** (consentimiento, anonimización,
evaluación) sí son deterministas y nadie las salta.

Principios:
- Por usuario y local-first. Los datos de clasificación P4 nunca salen del nodo.
- Un experto solo se promueve si gana al modelo base en su eval fijo.
- Xavier es memoria: no tiene bots propios. Expone datos, jobs y expertos; los
  agentes del usuario deciden.
- Un solo tipo para cada concepto (hoy hay 3 gates, 2 `ComputeProvider`, 2 sets
  de rutas `/v1/training`).

## 2. Estado real (auditoría 2026-10-02)

| Etapa | Hoy | Problema |
|---|---|---|
| Fuentes | `TrainingExporter` lee solo telemetría Data Commons | no lee memorias, trayectorias ni votos HC |
| Introspección | `humanchallenge/introspection.rs` + rutas `/v1/maloca/introspection/*` | store en RAM (`server.rs:1763` pasa `None`); guía = plantillas, no LLM; no produce datos de entrenamiento |
| Curación | 3 gates sueltos (cola JSON, `TrainingReadinessGate`, `NightTrainer::CurationGate`) | ninguno alimenta al exportador real |
| Anonimización | `PrivacyPipeline` P0–P4, Laplace fijo | k-anonimato no existe; `anonymization_audit.json` no se genera |
| Entrenamiento | scripts | `mini-expert-train.sh` es mock (GGUF vacío); notebook no corre headless y su `--outtype q4_k_m` falla |
| Registro | `MiniExpertRegistry` JSON | sin versiones ni métricas |
| Servir/enrutar | `ollama_models.rs` real; `mini-expert serve` stub; `/v1/agents/mini-experts*` solo en router de tests | no hay ruta en producción ni selección automática |
| Feedback | — | no existe |
| UI | `IntrospectionTab.tsx` existe pero no se renderiza y su contrato no coincide | no hay pantallas de datasets/expertos/jobs |
| Maloca | core HC + componentes Svelte sin fetch | modelo de challenge distinto al de Xavier |

## 3. Arquitectura

```
 memorias(workspace) ─┐
 trayectorias agente ─┤                ┌─ P4: solo backend local
 introspección ───────┼─► Curación ─► Anonimización ─► Bundle ─► Job ─┤
 votos HumanChallenge ┘   (1 gate)     (auditoría)     (hash)        └─ P3: backend remoto
                                                                         │
             feedback (invocaciones, correcciones) ◄── Router ◄── Registro ◄── Eval gate ◄─┘
```

### 3.1 Fuentes (`src/data_commons/training.rs`)
`TrainingSource` trait con implementaciones: `MemorySource` (por workspace y
`kind`, vía el store real), `TrajectorySource` (sesiones de agentes importadas),
`IntrospectionSource` (insights de sesiones completadas), `ChallengeSource`
(pares pregunta/respuesta con voto aceptado). Telemetría queda como una fuente
más, no la única.

### 3.2 Introspección → datos
- Persistir: `server.rs:1763` recibe un `HumanChallengeStore` en archivo y se
  arranca `HcCronBridge`.
- `POST /v1/maloca/introspection/{id}/complete` escribe filas en
  `curation_votes` con `training_eligible` según consentimiento del usuario.
- La guía puede usar el LLM del usuario (proveedor configurado) con las
  plantillas actuales como fallback; queda documentado cuál se usó.

### 3.3 Curación única
Un `TrainingReadinessGate` (el de `humanchallenge/curation_gate.rs`) absorbe la
cola JSON y el gate de `night_trainer.rs`. Entradas: votos aceptados, ratio de
hechos verificados, mínimo de ejemplos por dominio. Salida: `ReadyDomain { domain,
n_examples, clearance_max }`.

### 3.4 Anonimización
- P4 → el bundle se marca `local_only`; ningún backend remoto lo acepta.
- P3 → scrub + Laplace (epsilon configurable) + generación obligatoria de
  `anonymization_audit.json`. k-anonimato: implementar o borrar la afirmación del
  doc (decisión: implementar k≥5 sobre cuasi-identificadores de metadatos).

### 3.5 Jobs (`/v1/training/jobs`)
Tabla SQLite `training_jobs` (id, user, domain, bundle_hash, backend, base_model,
status, logs_uri, artifact_path, metrics, created/updated). Rutas: create,
get, list, cancel, logs. Un solo trait:

```rust
trait ComputeBackend {
    fn accepts(&self, bundle: &BundleManifest) -> bool; // P4 => solo Local
    async fn submit(&self, job: &TrainingJob) -> Result<RemoteRef>;
    async fn poll(&self, r: &RemoteRef) -> Result<JobStatus>;
    async fn fetch_artifacts(&self, r: &RemoteRef, dst: &Path) -> Result<Artifacts>;
    async fn cancel(&self, r: &RemoteRef) -> Result<()>;
}
```

Backends (el elegido por defecto es decisión del dueño, ver §6):
- `Local` — PEFT/LoRA en CPU o GPU AMD (ROCm) para modelos ≤0.5B; siempre
  disponible, lento.
- `Kaggle` — CLI oficial (`kaggle kernels push/status/output`), GPU T4 gratuita
  con cuota semanal; headless y dentro de términos de uso.
- `ColabEnterprise` — `gcloud colab executions create` + GCS; de pago.
- `ManualNotebook` — genera el notebook y espera que el humano suba el GGUF
  (fallback del Colab gratuito, que no tiene API headless).

Credenciales de backends en Clavis, nunca en config.

### 3.6 Entrenamiento (notebook/script único)
`scripts/training/train_expert.py` (reemplaza los 3 actuales): lee
`train.jsonl`, QLoRA/LoRA con PEFT+TRL actual, merge, `convert_hf_to_gguf.py
--outtype f16` y luego `llama-quantize … Q4_K_M`. Sin `files.upload/download`,
sin datasets sintéticos ni GGUF falsos: si algo falla, el job falla.

### 3.7 Evaluación y promoción
`scripts/eval/expert_eval.py`: mismo eval split para base y candidato; métricas
de tarea (exactitud/F1 o juez por rúbrica) + latencia + VRAM. Promueve si
`score_candidato ≥ score_base + margen` y latencia dentro de presupuesto.

### 3.8 Registro
Migrar `MiniExpertRegistry` a SQLite (`mini_experts`: name, version, domain,
base_model, bundle_hash, gguf_path, metrics, status candidate|active|retired,
clearance). Rollback = activar la versión anterior.

### 3.9 Servir y enrutar
- `ollama create` desde GGUF + Modelfile generado; `keep_alive` corto para
  expertos fríos y uno caliente (LRU) para respetar 8 GB.
- Montar `/v1/agents/mini-experts*` en el servidor de producción.
- Router: embedding de la consulta (nomic-embed ya instalado) contra el
  centroide del dominio de cada experto; si no supera umbral → modelo general.
- MCP tool `ask_expert(domain?, prompt)` para que los agentes lo usen.

### 3.10 Feedback
Cada invocación se registra (experto, versión, consulta hash, respuesta,
latencia). Correcciones o valoraciones del usuario/agente → eventos HC →
vuelven a curación.

### 3.11 Orquestación
- **Agente** (no determinista): skill `xavier-trainer` que usa solo la API:
  pregunta al gate qué dominios están listos, inspecciona muestras, lanza el
  job, lee la eval y decide promover o descartar. Cualquier agente del usuario
  puede ejecutarla.
- **Nocturno**: `NightTrainer` arranca en las tareas de fondo del servidor con
  ventana horaria y lock; solo crea jobs cuando el gate pasa y el backend
  elegido tiene cuota. Mismo camino de API que el agente.

### 3.12 Maloca y UI
- Maloca queda como UI (vía `swal-maloca-client` + token). La fuente de verdad
  de HumanChallenge es Xavier; maloca-core se adapta a sus tipos.
- panel-ui: renderizar `IntrospectionTab` con el contrato
  `/introspection/start|turn|complete`; nuevas pestañas Datasets, Curación,
  Expertos y Jobs. Unificar prefijo `/v1` (hoy conviven `/api/v1` y `/v1`).

## 4. Plan por olas (islas de archivos)

| WP | Contenido | Isla |
|---|---|---|
| MX-01 | Persistir HC + introspección → `curation_votes` | `src/humanchallenge/**`, `src/server/maloca/*`, línea de wiring en `server.rs` |
| MX-02 | Gate único + borrar duplicados | `src/humanchallenge/curation_gate.rs`, `src/curation/**`, `src/scheduler/night_trainer.rs` |
| MX-03 | Fuentes + exportador + auditoría de anonimización | `src/data_commons/{training,privacy}.rs`, `src/server/training_routes.rs` |
| MX-04 | Jobs API + `ComputeBackend` (Local + 1 remoto) | nuevo `src/training/**`, `src/enterprise/compute.rs` (absorber) |
| MX-05 | Script único de entrenamiento + eval | `scripts/training/**`, `scripts/eval/**` |
| MX-06 | Registro SQLite + serve + router + MCP tool | `src/agents/{mini_experts,provider_router}.rs`, `src/cli/handlers/mini_experts.rs`, MCP tools |
| MX-07 | UI: introspección + 4 pestañas | `panel-ui/src/maloca/**`, `panel-ui/src/components/training/**` |
| MX-08 | Skill `xavier-trainer` + NightTrainer en server | `skills/xavier-trainer/`, `src/scheduler/**` |

Olas: A = MX-01, MX-03, MX-05, MX-07 (islas disjuntas); B = MX-02, MX-04,
MX-06; C = MX-08 + prueba extremo a extremo con un experto real.

## 5. Primer experto (prueba de vida)

Un dominio con datos abundantes y eval objetivo (p. ej. clasificar el `kind` de
una memoria, o responder sobre el código de Xavier). 200–500 pares revisados,
base Qwen2.5-0.5B, comparación base vs experto en eval fijo.

## 6. Decisiones del dueño (2026-10-02)

1. Backends: **`Local` + `ManualNotebook`**. Sin Kaggle ni Colab Enterprise por
   ahora (quedan como implementaciones futuras del mismo trait). `Local` usa la
   GPU AMD de 8 GB (ROCm) para modelos ≤0.5B; los mayores generan un notebook
   que el humano corre en Colab gratuito y devuelve el GGUF por
   `POST /v1/training/jobs/{id}/artifacts`.
2. Primer experto: **código de Xavier** (preguntas sobre el repo; fuentes:
   code graph + memorias de sesiones de desarrollo).
3. Datos P3 pueden salir del nodo **solo tras anonimización con auditoría
   aprobada**. P4 nunca.
