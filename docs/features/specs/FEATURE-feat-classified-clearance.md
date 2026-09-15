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

### Fase 3 — Segmentos internos del laboratorio (pendiente)

- Grupos por segmento (`admin`, `research`, `ops`, `legal`) con `GroupManager` + `user_max_clearance`.
- Espacio de documentos internos (top-secret) separado del material del DAO.
- Auditoría de lecturas clasificadas (concedidas y denegadas) sobre `enterprise/audit.rs`.

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

