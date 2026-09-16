# FEATURE: Clasificación por identidad (clearance atado a la identidad + planes internos del laboratorio)

**Status:** F1 ✅ · F2.1 ✅ · F2.2 ✅ · F3 pendiente · F4 pendiente | **Tier:** núcleo (seguridad/gobernanza) | **Owner:** Belal + Hermes
**Última actualización:** 2026-09-15

## Problema

El laboratorio administra el 10% en segmentos internos (administración, investigación, otros) que son
**planes internos, no del DAO**, y deben permanecer ocultos (top-secret) con permiso de lectura por nodo
hasta que el consejo tenga nodos expertos verificados suficientes.

Hoy el andamiaje existe (`ClearanceLevel`, `AclManager`, `GroupManager`, `render_for_clearance`,
auditoría), pero **no es una frontera de seguridad**:

1. El middleware `clearance_middleware` **no estaba aplicado** en ningún router.
2. El nivel en `/v1/documents/{id}` lo declaraba **el cliente** por header (`X-Clearance`) ⇒ cualquiera
   podía leer material top-secret enviando `X-Clearance: TOP_SECRET`.
3. La búsqueda de memoria no intersecta el nivel del solicitante.
4. El default de las entradas sin nivel es `TopSecret` (invirtiendo el sentido de la política).

## Diseño (fases)

### Fase 1 — Nivel derivado de la identidad ✅ (implementado en esta ola)

- `resolve_requester_clearance(headers, claims)` devuelve **primero** el nivel derivado de las `Claims`
  autenticadas (`role_clearance`: Admin→TopSecret, User→Confidential, Readonly→Internal).
- El header `X-Clearance` **se ignora** salvo que el operador active `XAVIER_CLEARANCE_TRUST_HEADER=1`
  (interno/loopback/tests) → `trusts_clearance_header()`.
- Nuevo middleware `clearance_session_middleware`: publica `ClearanceLevel` + `ClearanceEnforcer` en las
  extensiones de la request. Se aplica **después** de `auth_middleware` en `protected_routes` y
  `large_body_routes` (`src/cli/server.rs`).
- `/v1/documents/{id}` (`f12_routes::get_document_handler`) usa el nivel derivado de la identidad.

**Criterios de aceptación (F1):**

| # | Criterio | Verificación |
|---|---|---|
| F1.1 | El header solo no otorga nivel | `cargo test --release --lib --features ci-safe clearance` → `test_header_ignored_without_optin` |
| F1.2 | Las claims mandan sobre el header | `test_claims_take_precedence_over_header` |
| F1.3 | El middleware publica el nivel derivado | `test_session_middleware_publishes_identity_level` |
| F1.4 | Documento: Readonly ve REDACTED, Admin ve completo | `server::f12_routes::tests::test_get_document_clearance_redaction_by_identity` |
| F1.5 | Documento: header solo no revela | `test_clearance_header_alone_does_not_grant_access` |

### Fase 2 — Enforzar en las lecturas (pendiente)

#### F2.1 — Política de niveles y parseo direccional ✅ (implementado)

- **Default corregido:** material sin nivel declarado ⇒ `default_clearance()` → **`Internal`**
  (antes `TopSecret`, que invertía la política y habría ocultado todo al activar el filtro).
  Override: `XAVIER_DEFAULT_CLEARANCE` (valor desconocido ⇒ `TopSecret`).
- **Parseo direccional** (un solo matcher, dos políticas):
  `parse_requester_level()` — desconocido ⇒ `Unclassified` (nunca otorga);
  `parse_required_level()` — desconocido ⇒ `TopSecret` (nunca degrada).
  Usados en el middleware y en la política de config; `ClearanceLevel::from(&str)` (fail-open) queda
  fuera del camino de decisión.
- **Criterios de aceptación:** `requester_parser_never_grants_on_unknown`,
  `required_parser_fails_closed_on_unknown`, `default_clearance_is_internal_unless_overridden`,
  `resolve_metadata_defaults_clearance_to_internal`.

#### F2.2 — Techo de lectura en búsqueda/lectura ✅ (implementado)

**Helpers (`src/security/clearance.rs`):**

| Función | Semántica |
|---|---|
| `readable_levels(requester)` | niveles que el solicitante puede leer (≤ su nivel) |
| `level_from_metadata(metadata)` | nivel declarado de una entrada; **fail-closed** (typo ⇒ `TopSecret`) |
| `intersect_clearance_filter(pedido, requester)` | recorta el filtro del cliente a su techo; nunca eleva |
| `split_by_clearance(requester, items, level_of)` | devuelve `(visibles, ocultos)`; los ocultos solo se cuentan |

**Rutas enforzadas:**

| Ruta | Handler vivo | Enforcement |
|---|---|---|
| `POST /memory/search` | `cli::handlers::memory::search_handler` | techo + intersección del filtro + `hidden_by_clearance` |
| `POST /v1/memories/search` | `server::v1_api::v1_memories_search` | techo sobre los documentos devueltos |
| `GET /v1/documents/{id}` | `server::f12_routes::get_document_handler` | redacción por nivel (F1) |

**Semántica elegida (y por qué):**

- **Búsqueda ⇒ exclusión**: el material por encima del techo no se lista. Devolverlo redactado
  revelaría igualmente la existencia y la estructura de los planes internos. El conteo
  `hidden_by_clearance` permite auditar sin revelar cuáles.
- **Lectura directa por id ⇒ redacción**: el id ya actúa como capacidad; se devuelve el documento
  con el contenido redactado (`[REDACTED: requires SECRET]`).
- **Sin identidad** (ruta sin el middleware) el techo es `default_clearance()` (`Internal`), nunca un
  nivel elevado y coherente con el default de ingesta. En modo abierto las `Claims` se insertan con
  rol `User` (⇒ `Confidential`), así que el material normal se lee igual que antes.

**Criterios de aceptación (F2.2):** `test_readable_levels_capped_by_requester`,
`test_intersect_filter_never_elevates`, `test_level_from_metadata_fail_closed`,
`test_split_by_clearance_hides_above_ceiling` (`cargo test --package xavier --lib --features ci-safe -- clearance`).

**Nota de implementación:** la ruta viva `/memory/search` la sirve `src/cli/handlers/memory.rs`
(no el módulo de adaptadores, que no está registrado en ningún router). El enforcement se aplicó ahí y
también en `src/adapters/inbound/http/handlers/memory.rs` + `v1_api.rs` por defensa en profundidad.

**Pendiente de F2.2 (migración):** informar cuántas entradas cambian de política por el default nuevo
(`TopSecret` → `Internal`). No se reescribe contenido: el nivel efectivo se resuelve en lectura.

### Fase 3 — Segmentos internos del laboratorio

#### F3.1 — Techo por segmento (nivel del grupo) ✅ (implementado)

Los grupos ya existían con persistencia (`security/groups.rs::GroupRegistry`, archivo
`data/security/groups.json`) y con auditoría propia. Lo que faltaba era que **la pertenencia
significara algo al leer**:

- `InfoGroup.clearance` — nivel que el grupo otorga a sus miembros con permiso de lectura.
  `#[serde(default = "default_clearance")]`: un grupo sin nivel explícito **no eleva** a nadie.
- `GroupRegistry::member_ceiling(member_ids)` — máximo de los niveles de los grupos donde el
  miembro tiene `acl.read`; `None` si no pertenece a ninguno. Un grupo sin permiso de lectura no
  eleva aunque la persona sea miembro.
- `shared_member_ceiling()` — registro compartido del camino de lectura, con **recarga por
  mtime+tamaño**: una búsqueda no paga un parseo por request y los cambios escritos por
  `/v1/f12/groups` se ven en la siguiente lectura.
- `resolve_effective_clearance()` (middleware) — **techo efectivo = rol ∪ segmentos**, aplicado
  en `clearance_session_middleware` y `clearance_middleware`. Sin `Claims` **no** se evalúa
  membresía: un anónimo nunca sube de nivel.
- `LAB_SEGMENTS` + `ensure_lab_segments()` — los cuatro segmentos del laboratorio
  (`seg-admin` TopSecret, `seg-research` Secret, `seg-ops` Confidential, `seg-legal` Secret) se
  crean **sin miembros**: la elevación se asigna, no se hereda.
- `POST /v1/f12/groups` acepta `clearance` opcional.

**Criterios (F3.1):** `test_member_ceiling_requires_read_membership`,
`test_member_ceiling_is_max_over_segments`, `test_group_without_clearance_does_not_elevate`,
`test_ensure_lab_segments_is_idempotent_and_empty`, `test_shared_ceiling_reads_file_and_reloads_on_change`,
`test_effective_clearance_is_union_and_never_lowers`, `test_effective_clearance_anonymous_never_lifts`.

#### F3.2 — Config de ruta, auditoría y namespace de segmento ✅ (implementado)

**(a) Exigencia de nivel por ruta — configuración del servidor, no del cliente**
(`src/security/route_policy.rs`): fuente por prioridad `XAVIER_REQUIRED_CLEARANCE_ROUTES` (JSON inline)
> `data/security/route_clearance.json` (recargado por mtime) > reglas por defecto del binario.
Gana el **prefijo más largo**; un nivel ilegible en config ⇒ `TopSecret` (un error de config
**cierra**, nunca abre). Enforcement en `enforce_route_policy()`, llamado desde
`clearance_session_middleware`: 403 + denegación auditada.

> **Hueco real detectado aquí y CERRADO en la fase siguiente (F4-export):** los endpoints de
> exportación (`/memory/export*`, `/v1/memory/export-markdown`) devolvían memoria en bloque **sin
> aplicar el nivel de cada entrada**. La política de ruta los exige a `INTERNAL` (bloquea al
> anónimo) y ahora además aplican el techo del solicitante con
> `split_by_clearance` + auditoría (`action: "export"`, con `hidden_by_clearance`). El contrato de
> `/memory/export` se mantiene (array de documentos) para no romper a los consumidores; el conteo de
> lo oculto viaja en la auditoría, no en el cuerpo.

**(b) Auditoría de lecturas clasificadas** (`src/security/clearance_audit.rs`): append-only JSONL en
`data/security/clearance_audit.jsonl` (override `XAVIER_CLEARANCE_AUDIT_PATH`), con
`timestamp, subject, role, requester_level, route, action, query_hash, visible, hidden_by_clearance, allowed`.
**La query no se guarda cruda** (solo sha256 corto) y el sujeto es el `sub`/email del token.
Concedidas y denegadas (`route_denied`). Un fallo de escritura se reporta por log y **no** tumba la
lectura. Nota: existe además `security::audit::AuditLogger` (SQLite, `log_check`) para chequeos de
permiso; unificar ambos sumideros en una sola superficie de consulta es deuda declarada.

**(c) Namespace de segmento = piso de nivel** (`groups::segment_level_from_path` +
`resolve_metadata`): material escrito en `segments/<seg-id>/...` no puede quedar por debajo del nivel
de ese segmento **aunque la metadata diga otra cosa**. El namespace es piso, no techo: declarar un
nivel más alto se respeta.

**Criterios (F3.2):** `test_longest_prefix_wins`, `test_unknown_level_in_config_closes`,
`test_disabled_rule_is_ignored`, `test_default_rules_cover_exports`,
`test_route_policy_denies_below_required`, `test_query_hash_is_stable_and_hides_text`,
`test_append_writes_jsonl_and_can_be_read_back`, `test_subject_and_role_from_claims`,
`test_record_uses_env_path_when_set`, `test_segment_level_from_path`,
`resolve_metadata_applies_segment_namespace_floor`.

**Falta de F3:** espacio de documentos *dedicado* al 10 % (hoy es convención de namespace, no un
store aparte).

### Fase 4 — E2E y operación (pendiente)

- Test E2E con dos identidades (nodo A normales vs nodo B del segmento) contra el servidor real.
- Guía operativa: cómo se escribe un documento segmentado y cómo se asigna nivel por grupo.
- Entrada del ledger a `stable` solo con pipeline verde (`scripts/verify-pipeline.sh`).

## Archivos tocados (Fase 1)

| Archivo | Cambio |
|---|---|
| `src/adapters/inbound/http/middleware/clearance.rs` | nivel derivado de identidad, opt-in de header, `clearance_session_middleware`, 4 tests |
| `src/adapters/inbound/http/middleware/mod.rs` | re-export de los nuevos símbolos |
| `src/server/f12_routes.rs` | `get_document_handler` usa el nivel de la identidad; test reescrito + test de no-confianza del header |
| `src/cli/server.rs` | cableado del middleware en `protected_routes` y `large_body_routes` |

## No objetivos

- No se implementa KYC ni identidad externa: el nivel sale del rol ya autenticado (token raíz, lease,
  sesión efímera o API token).
- No se cambia el contrato de las rutas públicas (`/health`, `/v1/mesh/public/nodes`).

## Deuda detectada (para F3, no se toca en F1)

- `clearance_middleware` (el viejo) exige el nivel por **header del cliente** (`X-Required-Clearance`):
  la exigencia de nivel debe ser **configuración de la ruta**, no una declaración del solicitante. F1 usa
  `clearance_session_middleware` (nivel derivado de identidad) y deja ese chequeo como está para no
  ampliar el alcance; en F3 se reemplaza por exigencia por ruta.
- `AclManager`/`GroupManager` no están respaldados por persistencia en el camino HTTP (viven en memoria
  en `f12_routes::F12State`); F3 debe decidir el store.

