# Guía operativa — clasificación por identidad y segmentos internos (clearance)

Feature `feat-classified-clearance` · F1 + F2.1 + F2.2 + F3.1 + F3.2 implementadas.
Rama `feat/classified-clearance-f1` · ledger `.gitcore/features.json` (95 %, falta F4-E2E con servidor vivo + PR).

## 1. Modelo en una tabla

| Concepto | Quién lo decide | Regla |
|---|---|---|
| **Nivel del rol** | la identidad (JWT/lease/sesión/token) | Admin→TopSecret · User→Confidential · Readonly→Internal |
| **Techo efectivo** | rol **∪** segmentos | máximo entre el rol y los segmentos donde el sujeto tiene `acl.read` |
| **Piso por namespace** | la ruta del documento | `segments/<seg-id>/...` no puede quedar por debajo del nivel del segmento |
| **Nivel exigido por ruta** | configuración del servidor | `route_clearance.json` / `XAVIER_REQUIRED_CLEARANCE_ROUTES` |
| **Header `X-Clearance`** | — | **ignorado** salvo `XAVIER_CLEARANCE_TRUST_HEADER=1` (interno/tests) |

Niveles: `UNCLASSIFIED < INTERNAL < RESTRICTED < CONFIDENTIAL < SECRET < TOPSECRET`.
Material sin nivel declarado ⇒ `INTERNAL` (override: `XAVIER_DEFAULT_CLEARANCE`).
Un nivel **ilegible** en la metadata de un documento ⇒ `TOPSECRET` (un typo cierra, no abre).

## 2. Segmentos del laboratorio (planes del 10 %)

| id | nivel | uso |
|---|---|---|
| `seg-admin` | TopSecret | administración del laboratorio |
| `seg-research` | Secret | investigación |
| `seg-ops` | Confidential | operaciones |
| `seg-legal` | Secret | legal y cumplimiento |

Nacen **sin miembros**: la elevación se asigna, no se hereda.

```bash
# crear (idempotente) — el nivel es opcional; sin él el grupo no eleva a nadie
curl -sS -X POST http://localhost:8006/v1/f12/groups \
  -H "X-Xavier-Token: $XAVIER_TOKEN" -H 'content-type: application/json' \
  -d '{"id":"seg-research","name":"Investigación","clearance":"SECRET"}'

# asignar a un nodo/persona (member_id = sub del token o email)
curl -sS -X POST http://localhost:8006/v1/f12/groups/join \
  -H "X-Xavier-Token: $XAVIER_TOKEN" -H 'content-type: application/json' \
  -d '{"group_id":"seg-research","member_id":"nodo-7"}'
```

Un grupo **sin** `acl.read` no eleva aunque el sujeto figure como miembro.

## 3. Escribir material clasificado

Dos caminos, y conviene el primero:

1. **Por namespace** (no depende de que quien escribe declare bien el nivel):
   `segments/seg-research/2026-plan-interno` ⇒ queda en `Secret` automáticamente.
2. **Por metadata**: `{"clearance": "SECRET"}` en la metadata del documento.

Declarar un nivel **más alto** siempre se respeta; declarar uno más bajo en un namespace de
segmento **no** baja el piso.

## 4. Leer: qué ve cada identidad

La búsqueda (`POST /memory/search`) devuelve solo entradas que el solicitante puede leer y reporta
`hidden_by_clearance` (cuántas quedaron fuera, sin decir cuáles):

```bash
curl -sS -X POST http://localhost:8006/memory/search \
  -H "X-Xavier-Token: $XAVIER_TOKEN" -H 'content-type: application/json' \
  -d '{"query":"plan interno laboratorio","limit":10}' | jq '{count, hidden_by_clearance}'
```

Semántica: **búsqueda ⇒ exclusión** (no revela existencia) · **lectura por id ⇒ redacción**
(`[REDACTED: requires SECRET]`), porque el id ya actúa como capacidad.

## 5. Exigir nivel en una ruta (config del servidor)

Copiar `docs/features/route_clearance.example.json` a `<workspace>/data/security/route_clearance.json`
y editar. Prioridad: `XAVIER_REQUIRED_CLEARANCE_ROUTES` (JSON inline) > archivo > reglas por defecto.
Gana el prefijo más largo. Quien no llega recibe `403` y la denegación queda auditada.

Reglas por defecto del binario: `/memory/export*` y `/v1/memory/export-markdown` exigen `INTERNAL`
(esos endpoints devuelven memoria en bloque **sin** filtrar por nivel; ver deuda).

## 6. Auditoría

Archivo append-only JSONL: `<workspace>/data/security/clearance_audit.jsonl`
(override `XAVIER_CLEARANCE_AUDIT_PATH`). Una línea por decisión:

```json
{"timestamp":"2026-09-15T21:40:12Z","subject":"nodo-readonly","role":"Readonly",
 "requester_level":"INTERNAL","route":"/memory/search","action":"search",
 "query_hash":"9f2c…","visible":3,"hidden_by_clearance":2,"allowed":true}
```

- **La query no se guarda cruda** (solo sha256 corto): se puede correlacionar sin almacenar contenido.
- `action: "route_denied"` = rechazo por política de ruta.
- Un fallo de escritura se reporta por log y **no** tumba la lectura.

```bash
# ¿quién fue rechazado hoy?
jq -c 'select(.allowed|not) | {subject, route, requester_level}' \
  data/security/clearance_audit.jsonl | sort | uniq -c | sort -rn
```

## 7. Lo que todavía NO hace (deuda declarada)

1. **Exports sin filtro por nivel** (P1): `/memory/export*` devuelve memoria en bloque sin aplicar el
   nivel de cada entrada. Mitigado exigiendo `INTERNAL` por ruta; el arreglo de fondo es filtrar el
   export por el nivel del solicitante.
2. **Dos sumideros de auditoría**: este JSONL y `security::audit::AuditLogger` (SQLite). Unificarlos
   en una sola superficie de consulta queda pendiente.
3. **Espacio de documentos dedicado al 10 %**: hoy es convención de namespace (`segments/<seg>/…`),
   no un store separado.
