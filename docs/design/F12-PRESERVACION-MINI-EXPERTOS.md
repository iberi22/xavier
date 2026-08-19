# Xavier — Preservación de Información Real + Mini-Expertos Personales (Visión Fundacional)

> **Estado: DOCUMENTO FUNDACIONAL — define el enfoque; features y olas derivadas**
> Fecha: 2026-08-08 · Autor: Hermes orquestador + visión de BELA · Versión: 1.0

---

## 1. La Misión

**Xavier es un intento de la humanidad por preservar TODA la información del
mundo, para que el futuro se construya con información REAL regenerada — no
generada.**

- Los humanos son los **curadores** de la información, usando Xavier y todo el
  ecosistema SWAL.
- Cada persona puede crear **modelos personalizados** usando SUS propios datos
  internos: modelos privados, personales, entrenados con su propia información.
- Los humanos no son reemplazados por la IA — son sus curadores y propietarios.

## 2. Arquitectura de información (3 capas)

### Capa 1 — Repos PÚBLICOS (como GitHub público)

- Los repos SWAL que decidamos publicar se comparten a toda la red.
- Cualquier nodo puede consultar el CodeGraph + RAG del repo público.
- Ejemplo: xavier, gestalt, maloca, edge-mesh (los cores) son públicos.

### Capa 2 — Red INTERNA SWAL (red de servicio)

- Información de TRABAJO compartida SOLO con la red interna SWAL:
  - Benchmarks, logs, feedbacks, telemetría de funcionamiento
  - Detalles de desarrollo (waves, decisiones, análisis)
  - Memorias y lógicas internas con las que Xavier trabaja
  - **NADA personal** — solo funcionamiento y mejora de Xavier
- Esto permite que la red entera mejore Xavier: las memorias, las lógicas y
  los datos de operación se comparten entre nodos de servicio.

### Capa 3 — Meshes PRIVADAS personales

- Cada usuario tiene su mesh privada entre SUS dispositivos (misma billetera
  de claves).
- Información personal, datos propios, modelos privados — nunca sale de la mesh.
- Permisos rigurosos: grupos de información con control granular.

## 3. Clasificación de documentos (modelo gobierno)

Xavier implementa **clasificación por niveles de sensibilidad**, como los
documentos clasificados del gobierno:

| Nivel | Nombre | Uso | Quién puede ver |
|-------|--------|-----|-----------------|
| 0 | `UNCLASSIFIED` | Repos públicos, docs, skills | Toda la red |
| 1 | `INTERNAL` | Red interna SWAL: benchmarks, logs, feedbacks | Nodos de servicio |
| 2 | `RESTRICTED` | Memorias internas de Xavier, lógicas, decisiones | Nodos autorizados |
| 3 | `CONFIDENTIAL` | Datos de trabajo sensibles, planes | Grupo específico |
| 4 | `SECRET` | Datos personales, modelos privados | Mesh privada del dueño |
| 5 | `TOPSECRET` | Claves, wallets, material crítico | SOLO el nodo dueño |

**Censura por partes** — como los documentos del gobierno, un documento puede
ser clasificado con secciones REDACTED (censuradas) para audiencias parciales:

```
Documento: "Análisis del IrohTransport"
  Sección 1 (pública): arquitectura general → UNCLASSIFIED
  Sección 2 (interna): benchmark de rendimiento → INTERNAL
  Sección 3 (secreta): vulnerabilidad conocida → SECRET
```

Implementación: el documento se divide en segmentos, cada uno con su nivel.
El servidor sirve la versión REDACTADA (sin secciones secretas) a quien no
tiene clearance.

## 4. Grupos de información y permisos rigurosos

- **Grupos**: colecciones de información con una política de permisos.
- Ejemplo: `grupo: core-xavier-dev` (los devs del core), `grupo: nodos-servicio`
  (benchmarks), `grupo: familia-bela` (mesh privada).
- **Permisos**: cada grupo define quién puede: leer / escribir / auditar.
- **Enforcement**: TODAS las lecturas pasan por el check de clearance —
  no hay bypass. El servidor redacta lo que el solicitante no puede ver.
- Auditoría: cada acceso queda registrado (quién, qué, cuándo, con qué clearance).

## 5. Pipeline de Mini-Expertos personales (entrenamiento local)

### 5.1 Fuente: los propios datos de Xavier

Xavier YA tiene `TrainingExporter` (`src/data_commons/training.rs`):
- Genera `TrainingBundle` con `train_split` + `eval_split`
- **Auditoría de consentimiento**: excluye records sin consentimiento o revocados
- CLI: `xavier data-commons training-bundle` (vía `cli/commands/data_commons.rs`)

### 5.2 Puertos de servicio de datos para entrenamiento

Xavier expone endpoints para servir datasets de entrenamiento:

```
GET /v1/training/datasets                  → lista de datasets disponibles
GET /v1/training/datasets/{id}             → manifiesto del dataset (size, split)
GET /v1/training/datasets/{id}/train       → train split (JSONL)
GET /v1/training/datasets/{id}/eval        → eval split (JSONL)
POST /v1/training/bundles                  → generar un bundle nuevo (seed, eval_ratio)
```

Cada dataset lleva: `clearance` (nivel mínimo), `consent` (registros con
consentimiento), `segment` (idioma/dominio), `schema_version`.

### 5.3 Entrenamiento de mini-expertos

**Objetivo**: mini-expertos que se ejecutan ON-DEMAND y cargan rápido.

- Modelos pequeños (1-3B params) con SOLO el idioma del usuario, o inglés +
  el idioma del usuario.
- Segmento por dominio: cada mini-experto se instruye con un segmento
  específico (ej: "experto en IrohTransport de xavier", "experto en el
  marketplace de gara-g").
- Como hacen los expertos humanos: se especializan en SU segmento.

**Herramientas de entrenamiento disponibles en el sistema:**
- `agy` v1.1.8 — CLI de Google (cuenta logueada) — puede acceder a Colab/Vertex
- **`colab` CLI oficial (googlecolab/google-colab-cli, Apache-2.0)** — el
  pipeline CANÓNICO para mini-expertos. Comandos clave:

| Comando | Uso en el pipeline |
|---------|-------------------|
| `colab run --gpu T4 train.py` | VM efímera con GPU: provisiona → ejecuta → descarga → destruye (1 comando) |
| `colab new --gpu L4/H100` | VM persistente para entrenamientos largos |
| `colab upload LOCAL REMOTE` | Subir el dataset JSONL de Xavier |
| `colab download REMOTE LOCAL` | Bajar el modelo resultante (GGUF/safetensors) |
| `colab install -r req.txt` | Instalar deps con uv (ultra-rápido) |
| `colab auth` | Autenticar GCP (BigQuery/GCS) en la VM |
| `colab log -o out.jsonl` | Exportar logs de sesión como JSONL |
| `colab pay` | Gestionar compute units |

- Soporta CPU, GPU (T4, L4, G4, H100, A100) y TPU (v5e1, v6e1).
- Keep-alive automático evita que la VM idle se termine.
- El ejemplo oficial "Accelerator Training with Checkpoint Retrieval" es el
  flujo exacto: provisionar GPU → correr train → recuperar pesos → destruir.
- Ollama local para servir los mini-expertos resultantes (GGUF).

**Flujo (canónico con colab CLI):**
```
1. Xavier exporta el dataset (TrainingExporter → /v1/training/* → JSONL)
2. colab upload dataset.jsonl xavier-dataset.jsonl
3. colab run --gpu T4 -- train_lora.py --dataset xavier-dataset.jsonl
   (el script: carga base 1-3B, LoRA, solo idioma del usuario, guarda GGUF)
4. colab download /content/model.gguf model.gguf
5. El modelo GGUF se sirve localmente con Ollama/llama.cpp
6. El mini-experto responde consultas sobre SU segmento — con datos REALES
```

**Nota**: el CLI se instala con `uv tool install colab-cli` (o pip) — ver
github.com/googlecolab/google-colab-cli. Config en ~/.config/colab-cli/.
Solo Linux/macOS (soportado en este sistema).

### 5.4 Mini-expertos on-demand

- Cada mini-experto = un modelo GGUF pequeño + su metadata (segmento, idioma,
  dataset fuente, fecha, clearance).
- Carga rápida: modelos < 2GB, arrancan en segundos.
- El router de xavier (`ProviderRouter`) puede incluir mini-expertos locales
  como endpoints (`ProviderKind::Local` ya soporta lista de endpoints).

## 6. Integración con el mesh (F9 v2)

La visión se integra con el documento F9-MESH-SWAL-PUBLICO-PRIVADO.md:

| Capa | Mesh | Contenido | Clearance |
|------|------|-----------|-----------|
| Pública | Directorio público SWAL | Repos públicos + CodeGraph | UNCLASSIFIED |
| Interna | Red de servicio | Benchmarks, logs, feedbacks | INTERNAL/RESTRICTED |
| Privada | Mesh de billetera | Datos personales, modelos privados | SECRET/TOPSECRET |

- El árbol público (`/mesh/public/tree`) ahora incluye el clearance de cada rama.
- El RAG público (`/mesh/public/rag`) redacta secciones según el clearance
  del solicitante.
- Los nodos de servicio comparten telemetría de funcionamiento (para mejorar
  xavier) pero NUNCA datos personales.

## 7. Features propuestas (para features.json de xavier)

| Feature ID | Nombre | Descripción | Prioridad |
|-----------|--------|-------------|-----------|
| `feat-clearance-levels` | Clasificación por niveles | Niveles UNCLASSIFIED→TOPSECRET + redacción por segmento | ALTA |
| `feat-groups-permissions` | Grupos con permisos | Grupos de info + ACL rigurosa + auditoría | ALTA |
| `feat-training-datasets-api` | API de datasets | /v1/training/* para servir datos de entrenamiento | ALTA |
| `feat-mini-experts` | Mini-expertos personales | Pipeline: dataset → Colab → GGUF → Ollama local | MEDIA |
| `feat-mesh-service-network` | Red de servicio SWAL | Compartir telemetría/benchmarks/feedbacks entre nodos de servicio | MEDIA |
| `feat-mesh-private-wallet` | Mesh privada por billetera | Nodos de la misma billetera, sync de memoria+modelos | MEDIA |
| `feat-content-redaction` | Censura por partes | Documentos con secciones REDACTED según clearance | ALTA |
| `feat-human-curation` | Curaduría humana | UI/API para que humanos revisen, aprueben, clasifiquen información | MEDIA |

## 8. No-goals

- NO entrenar modelos grandes (solo mini-expertos on-demand 1-3B)
- NO compartir datos personales en la red (siempre mesh privada)
- NO automatizar la curaduría sin humano (el humano aprueba)
- NO reemplazar el juicio humano — Xavier preserva, el humano cura

## 9. Referencias

- `F9-MESH-SWAL-PUBLICO-PRIVADO.md` — el mesh (mismo directorio)
- `src/data_commons/training.rs` — TrainingExporter existente
- `src/data_commons/mesh_bridge.rs` — MeshCommonsBridge existente
- `docs/benchmark/DATA-MARKETPLACE.md` — F12 marketplace (CP-ABE futuro)
- `src/cli/commands/data_commons.rs` — CLI existente
