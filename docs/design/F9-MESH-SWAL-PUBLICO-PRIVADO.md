# Xavier Mesh — Red SWAL Pública + Meshes Privadas (F9 v2)

> **Estado: DISEÑO — requiere aprobación antes de implementar** (protocolo cores críticos)
> Fecha: 2026-08-08 · Actualizado: 2026-08-14 (olas M6/M7 — node provisioning) · Autor: Hermes orquestador · Versión: 0.2 (propuesta)

---

## 1. Visión

Xavier es el nodo MADRE de SWAL. Desde él:

1. **Red SWAL pública** — compartimos TODAS las memorias de desarrollo y código
   (CodeGraph + RAG) con los nodos de la red. Cualquier nodo SWAL puede
   descubrirnos públicamente, consultar nuestro árbol de información y hacer
   RAG inteligente sobre nuestro codebase.
2. **Meshes privadas** — además de SWAL, cada usuario puede crear meshes
   privadas entre SUS dispositivos (nodos anclados a una MISMA billetera de
   claves), para administrar información privada sin exponerla a la red.
3. **Abstracción del codebase** — Xavier abstrae los repos con CodeGraph,
   guarda SNAPSHOTS y ofrece búsqueda inteligente de fragmentos exactos
   (línea, función, clase, if) → los issues/planes se completan con el
   cambio EXACTO → los ejecutores (agentes, CLIs) ahorran tokens al no
   reescribir fragmentos enteros.

## 2. Estado actual (verificado 2026-08-08)

| Componente | Estado | Detalle |
|-----------|--------|---------|
| IrohTransport QUIC | ✅ implementado (wave-9) | NAT traversal, wire-compatible con HTTP |
| HTTP mesh transport | ✅ 100% | handshake/manifest/chunks/session |
| ACL | ✅ 90% | — |
| libp2p | ❌ 10% | legacy no compila |
| onchain_gov | ❌ 0% | — |
| **0 peers activos** | ⚠️ | nadie conectado aún |
| CodeGraph multi-proyecto | ✅ 100% | FTS5, symbols, edges, snapshots |
| Snippet write-through | ✅ 100% | símbolos → memorias code_snippets |
| Edge-hive (nodo) | ✅ existe | identity/discovery/tunnel/brain/db/wasm/mcp |
| Edge-mesh (P2P CRDT) | ✅ canónico | WebRTC, Yjs, c49ad5b |

## 3. Arquitectura propuesta

### 3.1 Roles de nodo

```
┌─────────────────────────────────────────────────────────┐
│  NODO MADRE (Xavier en PC principal)                    │
│  • Publica: CodeGraph de repos SWAL + memorias dev      │
│  • RAG público: /mesh/public/rag?q=...                  │
│  • Árbol de información: /mesh/public/tree              │
│  • Snapshot manager: guarda + sirve snapshots           │
├─────────────────────────────────────────────────────────┤
│  NODOS SWAL (edge-hive en VPS/Android/Pi)               │
│  • Se descubren vía discovery público                   │
│  • Consumen RAG del nodo madre                          │
│  • Pueden publicar su propio contenido                  │
├─────────────────────────────────────────────────────────┤
│  MESH PRIVADA (dispositivos del mismo usuario)          │
│  • Anclada a la MISMA billetera de claves (Ed25519)     │
│  • Descubrimiento privado: solo nodos con la clave      │
│  • Sincronizan memoria/código entre dispositivos        │
└─────────────────────────────────────────────────────────┘
```

### 3.2 Identidad y billetera de claves

- **NodeIdentity** (ya existe en xavier y edge-hive): Ed25519 keypair.
- **Billetera**: el nodo madre tiene una billetera de claves (Clavis/KeyLendingEngine
  ya existe en xavier). Los nodos de la MISMA billetera forman la mesh privada.
- **Verificación**: handshake con firma Ed25519 (ya implementado en ACL).

### 3.3 Descubrimiento público (SWAL)

- Publicar la identidad pública (PeerInfo) en un **directorio público SWAL**:
  - Endpoint HTTP en el nodo madre: `GET /mesh/public/nodes` → lista de nodos.
  - Cada nodo se registra (heartbeat) con: node_id, capabilities, manifest de lo que publica.
- **Iroh relay** para NAT traversal (ya implementado en IrohTransport).

### 3.4 Árbol de información pública

Cada nodo publica un **manifiesto en árbol** de lo que ofrece:

```
GET /mesh/public/tree
{
  "node_id": "...",
  "tree": {
    "repos": {
      "xavier": {"snapshot": "2026-08-08", "symbols": 21500, "files": 625},
      "gestalt": {"snapshot": "2026-08-07", "symbols": 9500, "files": 300}
    },
    "memorias": {"count": 13000, "kinds": ["decision","state","analysis"]},
    "skills": {"count": 90},
    "waves": {"last": "2026-08-08"}
  }
}
```

- El árbol permite a cualquier nodo/agente ver la PROFUNDIDAD de lo disponible
  sin hacer búsquedas a ciegas.
- Expansión: `GET /mesh/public/tree/repos/xavier` → detalle del snapshot.

### 3.5 RAG inteligente público

```
POST /mesh/public/rag
{ "query": "cómo funciona el IrohTransport", "repo": "xavier", "limit": 5 }
→ { "results": [ {symbol, file, line_start, line_end, snippet, score} ] }
```

- Usa el CodeGraph DB existente (FTS5 + symbols) + snippet write-through.
- **Snapshot**: cada repo tiene un snapshot del codebase (fecha + hash).
  Los nodos consumidores NO necesitan el código fuente — solo el snapshot indexado.
- **Snippet preciso**: devuelve el fragmento EXACTO (función, clase, if)
  con línea de inicio/fin → el consumidor aplica el cambio sin reescritura.

### 3.6 Completar issues/planes con snippets (ahorro de tokens)

Flujo para agentes ejecutores (Hermes, opencode, Claude, Jules):

```
1. El plan/issue dice: "cambiar la función X en archivo Y"
2. Xavier consulta el snapshot: GET /mesh/public/rag (o /code/find local)
3. Devuelve el fragmento EXACTO de X (líneas L1-L2, contexto)
4. El agente aplica el cambio solo en esas líneas (diff mínimo)
5. Sin reescritura de archivos enteros → ahorro de tokens (68%+ con snippets)
```

- Esto convierte a Xavier en el **reemplazo inteligente de GitHub** para el
  trabajo con agentes: el "repo" es el snapshot indexado, no el git clone.
- El agente NO busca en el codebase — usa las tools de Xavier (code_find,
  code_context, /mesh/public/rag).

### 3.7 Mesh privada (misma billetera)

- Nodos con la MISMA billetera de claves forman una mesh privada.
- Descubrimiento: el nodo madre conoce todos sus nodos (registro por
  billetera) — no publica sus direcciones en el directorio público.
- Transporte: IrohTransport con cifrado de sesión (ya existe el protocolo
  MeshSessionShare).
- Sincronización: memoria (vec-store) + snapshots de código entre
  dispositivos del usuario.

### 3.8 BaaS nodes (Supabase/Neon como nodos persistentes) — Ola M6

Un servicio cloud (Supabase, Neon) se convierte en nodo SWAL cuando el usuario
pega su API token: Xavier lo provisiona vía la API del provider y lo administra
de forma autónoma. Objetivo: **cualquier usuario o empresa puede crear nodos
rápido con su token**.

**Flujo de provisioning:**

```
1. xavier nodes add --provider supabase --token sbp_xxx [--visibility public|private]
2. Xavier valida el token contra la API del provider (proyecto accesible)
3. Clavis guarda el token: AES-256-GCM + SecretLease (UUID + TTL + agent_id)
   → el token NUNCA queda en disco plano, config, ni logs
4. Xavier configura el provider vía su API:
   • Supabase: RLS policies, bucket cifrado, edge functions (relay/heartbeat)
   • Neon: schema de nodo + replicación
5. Registro del nodo según visibility:
   • public  → directorio público (M1): GET /mesh/public/nodes + heartbeat firmado
   • private → mesh privada (M3): solo visible para la billetera del usuario
6. Resultado: node_id + lease_id + informe de provisioning
```

**Contrato API (Xavier):**

```
POST   /v1/nodes                  { provider: "supabase"|"neon"|"vps",
                                    credential_ref, visibility, wallet_id }
GET    /v1/nodes                  → lista (provider, status, lease_expiry, visibility)
GET    /v1/nodes/{node_id}        → detalle + health (último heartbeat)
POST   /v1/nodes/{node_id}/rotate → rota credencial (nuevo token, revoca lease viejo)
DELETE /v1/nodes/{node_id}        → revoca lease en Clavis + desregistra (M1/M3)
```

CLI: `xavier nodes add|list|show|rotate|remove|status`.

**Supabase como nodo persistente público de administración SWAL:**

| Recurso | Visibilidad | Uso |
|---------|-------------|-----|
| `node_registry` (tabla) | pública — RLS anon READ | Directorio público M1 persistido |
| `ops_feed` (tabla) | pública | Eventos ops replicables a la mesh |
| `swal-vault` (bucket) | privada | JSONs cifrados E2E (claves en la billetera; Supabase solo ve ciphertext) |

**Sync de la info pública (Yjs CRDT):** la info pública de la mesh SWAL se
replica a los nodos mesh locales como documentos Yjs CRDT (edge-mesh ya usa
Yjs/y-protocols). El `ops_feed` del nodo BaaS actúa como relay store & forward
para nodos offline — persistencia, NO autoridad: la autoridad sigue siendo la
billetera (mesh ≠ blockchain).

**Integración de seguridad (vault + Clavis + edge-mesh):**

- **swal-vault** (Flutter): administra la billetera (identidad de nodo) + claves E2E.
- **Clavis** (Xavier): administra tokens BaaS (leases UUID+TTL, revoke, rotación).
- **edge-mesh**: authz + cifrado de sesión entre nodos privados.
- **Herencia de permisos**: mesh privado → almacenamiento cifrado → bucket cloud.
  El bucket hereda la ACL de la mesh; nada se cifra con claves del provider.

**Tests M6:**

- `nodes add` contra provider mock (HTTP stubs): token queda en Clavis cifrado;
  test verifica ausencia de plaintext en disco/config/logs.
- Lease TTL expira → nodo marcado `degraded`; `nodes rotate` renueva.
- Nodo público aparece en `GET /mesh/public/nodes`; nodo privado NO es visible
  para otra billetera.
- RLS aplicadas: anon lee `node_registry`, anon NO escribe.
- Objetos del bucket `swal-vault` cifrados E2E (roundtrip solo con clave de billetera).
- Sync Yjs: cambio en `ops_feed` converge en 2 nodos mesh locales.

### 3.9 SSH/VPS private nodes — Ola M7

Un VPS alcanzable por SSH se convierte en nodo PRIVADO de la billetera del
usuario. Xavier instala el agente de nodo y lo registra en la billetera.

**Flujo de provisioning:**

```
1. xavier nodes add --provider vps --ssh user@host --key ~/.ssh/id_ed25519
   [--visibility private]   # private es el default
2. La clave SSH se guarda en Clavis (AES-256-GCM + lease TTL) — nunca en disco plano
3. Xavier conecta por SSH e instala el agente de nodo (edge-hive lite:
   subconjunto identity/sync/tunnel del edge-hive completo)
4. El agente se registra en la billetera: challenge-response Ed25519
   (mismo protocolo que M3)
5. El nodo persiste información privada del mesh interno del usuario:
   memoria + snapshots, con cifrado de sesión (MeshSessionShare)
6. Herencia de permisos: la billetera gobierna QUÉ se replica y con QUÉ
   cifrado — la ACL de la mesh aplica al nodo remoto como a cualquier nodo
```

**Seguridad:**

- Clave SSH con lease TTL en Clavis; `nodes rotate` regenera y revoca la anterior.
- Visibilidad default `private`: el nodo NO aparece en el directorio público (M1).
- El agente NO tiene acceso a la billetera: opera con su node key derivada del
  pairing; la billetera solo vive en el dispositivo del usuario (+ vault).
- `nodes remove` revoca el lease y el agente pierde acceso de inmediato.

**Tests M7:**

- Provisioning contra servidor SSH de prueba (mock/local): agente instalado
  y registrado → visible en `xavier nodes list`.
- La clave SSH solo existe en Clavis (test de ausencia en disco plano).
- Nodo de OTRA billetera NO puede unirse a la mesh privada (cross-wallet isolation).
- Revocación: tras `nodes remove`, el agente pierde acceso (heartbeat rechazado).
- El nodo sync memoria + snapshots de la mesh privada con cifrado de sesión.

## 4. Componentes a construir (por ola)

### Ola M1 — Directorio público + árbol de información
- `GET /mesh/public/nodes` — registro + heartbeat de nodos públicos
- `GET /mesh/public/tree` — manifiesto en árbol del nodo (repo/snapshot/memorias)
- Registro del nodo madre SWAL como primer nodo público
- Tests: registro, heartbeat, árbol

### Ola M2 — RAG público sobre snapshots
- `POST /mesh/public/rag` — búsqueda sobre CodeGraph de repos compartidos
- Snapshot manager: `POST /mesh/snapshot {repo}` → hash + fecha + stats
- Expansión del árbol por nodo
- Tests: RAG público, snapshot, profundidad del árbol

### Ola M3 — Mesh privada (misma billetera)
- Registro de nodos por billetera (Clavis)
- Descubrimiento privado (solo nodos de la misma billetera)
- Sync de memoria + snapshots entre dispositivos
- Tests: dos nodos de la misma billetera se encuentran; uno de otra NO

### Ola M4 — Integración edge-hive como nodo SWAL
- edge-hive se conecta al directorio público del nodo madre
- Consume RAG público (edge-hive-brain como cache local)
- Publica su propio árbol (capabilities del nodo)

### Ola M5 — CodeGraph como fuente de issues precisos
- Generador de issues: toma el snapshot + diff → issue con fragmento exacto
- Integración con el template canónico (gitcore-jules-issues)
- Métrica de ahorro de tokens (antes/después)

### Ola M6 — BaaS nodes (Supabase/Neon) (REQ-029, US-042)
- CLI `xavier nodes add --provider supabase|neon --token ... [--visibility]`
  + `nodes list|show|rotate|remove|status`
- API REST `/v1/nodes` (add/list/show/rotate/remove) — contrato en §3.8
- Clavis: storage de tokens BaaS (AES-256-GCM + lease UUID/TTL + revoke + rotación)
- Provisioner por provider: Supabase (RLS + bucket cifrado + edge functions
  relay/heartbeat), Neon (schema de nodo + replicación)
- Registro público (M1) o privado (M3) según visibility
- Supabase como nodo persistente público de administración SWAL:
  `node_registry` (RLS anon read), `ops_feed` (público replicable),
  bucket `swal-vault` (privado, JSONs cifrados E2E)
- Sync de info pública a nodos mesh locales vía Yjs CRDT (ops_feed = relay)
- Integración: vault (billetera + claves E2E) · Clavis (tokens) · edge-mesh (authz/sesión)
- Tests: provider mock, token cifrado sin plaintext, lease TTL/rotate, RLS,
  visibility pública/privada, bucket E2E, convergencia Yjs (§3.8)

### Ola M7 — SSH/VPS private nodes (REQ-030, US-043)
- CLI `xavier nodes add --provider vps --ssh user@host --key ~/.ssh/id_ed25519`
- Clave SSH → Clavis (AES-256-GCM + lease TTL + rotación); nunca en disco plano
- Instalación del agente de nodo (edge-hive lite) vía SSH
- Registro en la billetera con challenge Ed25519 (protocolo M3);
  visibility default `private`
- Nodo privado persiste info del mesh interno (memoria + snapshots)
  con cifrado de sesión (MeshSessionShare)
- Herencia de permisos: la billetera gobierna qué se replica y con qué cifrado
- Tests: provisioning SSH mock, registro, cross-wallet isolation,
  revocación pierde acceso, sync privado cifrado (§3.9)

## 5. Decisiones de diseño (a validar)

| Decisión | Opción A | Opción B | Recomendación |
|---------|----------|----------|---------------|
| Transporte | Iroh QUIC (ya implementado) | libp2p (roto) | **A** — Iroh ya funciona |
| Directorio público | Endpoint HTTP en nodo madre | DHT global | **A** — simple, SWAL es la primera red |
| Billetera | Clavis (KeyLendingEngine) | Nueva | **A** — Clavis ya existe |
| Nodo remoto | Edge-hive (19 crates) | edge-mesh TS | **Edge-hive** para nodos pesados, edge-mesh para apps |
| Snapshot | CodeGraph DB + hash | Git clones | **CodeGraph** — sin git, abstracción real |
| Credenciales de nodos (tokens BaaS / SSH keys) | Clavis (leases AES-256-GCM, ya existe) | .env / archivo plano / keychain externo | **A** — sin dependencias nuevas, tokens nunca en disco plano |
| Nodo BaaS | Supabase/Neon administrados vía API del provider | Cloud propio/docker manual | **BaaS administrado** — onboarding rápido con solo el token |
| Nodo VPS | edge-hive lite instalado vía SSH | Nodo manual sin agente | **edge-hive lite** — mismo stack, registro automático en billetera |
| Persistencia pública | Supabase (RLS anon read) + sync Yjs CRDT | DHT / blockchain | **Supabase + Yjs** — relay store&forward, mesh ≠ blockchain |

## 6. Riesgos y mitigaciones

| Riesgo | Mitigación |
|--------|-----------|
| 0 peers activos hoy | La ola M1 arranca con el nodo madre como primer peer; pruebas con 2 nodos locales |
| libp2p roto | Usar Iroh (funciona); no arreglar libp2p legacy |
| Seguridad del directorio público | Heartbeat firmado Ed25519; solo nodos registrados; rate limit |
| Privacidad de la mesh | Claves por billetera; cifrado de sesión; no publicar direcciones privadas |
| Tamaño de snapshots | Solo símbolos+edges (no archivos completos); incremental por hash |
| Token BaaS filtrado | Solo vive en Clavis (AES-256-GCM, lease TTL); rotación + revoke; nunca en disco/logs |
| Clave SSH comprometida | Lease TTL + rotación; `nodes remove` revoca y el agente pierde acceso |
| Bucket cloud expuesto | JSONs cifrados E2E con clave de billetera; provider solo ve ciphertext |
| Vendor lock-in BaaS | Provider = relay/persistencia; la autoridad y la data cifrada viven en la billetera |

## 7. Criterios de aceptación (F9 v2)

- [ ] 2 nodos Xavier locales se descubren vía el directorio público (sin config manual de IP)
- [ ] Nodo B hace `POST /mesh/public/rag` contra nodo A y obtiene snippets de su CodeGraph
- [ ] El árbol público muestra profundidad (repos → snapshots → símbolos)
- [ ] 2 nodos de la MISMA billetera forman mesh privada; un tercero NO los ve
- [ ] Un issue se genera con el fragmento EXACTO (línea/función) del snapshot
- [ ] El agente ejecutor aplica el cambio con diff mínimo (métrica de tokens)
- [ ] `xavier nodes add --provider supabase --token ...` provisiona un nodo BaaS
      (RLS + bucket + heartbeat) y el token queda SOLO en Clavis (REQ-029)
- [ ] `xavier nodes add --provider neon --token ...` crea schema + replicación (REQ-029)
- [ ] Nodo BaaS público en `/mesh/public/nodes`; privado invisible a otra billetera (REQ-029)
- [ ] `xavier nodes add --provider vps --ssh user@host --key ...` instala edge-hive
      lite y lo registra en la billetera (REQ-030)
- [ ] Clave SSH solo en Clavis; `nodes remove` revoca y el agente pierde acceso (REQ-030)
- [ ] Nodo privado VPS sync memoria+snapshots de la mesh con cifrado de sesión (REQ-030)

## 8. No-goals (esta fase)

- NO mercado público de nodos (es F12 Data Marketplace)
- NO monetización / tokenomics on-chain (ya hay diseño separado)
- NO reemplazo completo de GitHub para humanos — solo para agentes/CLIs
- NO multi-wallet complejo (una billetera por mesh privada en v1)
- NO implementación de código en las olas M6/M7 todavía — este documento es DISEÑO

## 9. ACs verificables por comando (REQ-029 / REQ-030)

Lista de aceptación ejecutable por comando para cada REQ nuevo. Cada comando
debe ser reproducible en un entorno de pruebas con providers mock (HTTP stubs)
o un VPS local, sin credenciales reales.

### REQ-029 — BaaS nodes (Ola M6)

| # | Comando | Resultado esperado |
|---|---------|--------------------|
| 1 | `xavier nodes add --provider supabase --token sbp_mock --visibility public` | Sale `node_id` + `lease_id`; RLS + bucket + edge function aplicados en el mock |
| 2 | `xavier nodes add --provider neon --token npx_mock` | Schema de nodo + replicación creados en el mock |
| 3 | `xavier nodes list` | El nodo aparece con `provider`, `status=active`, `lease_expiry`, `visibility` |
| 4 | `grep -r "sbp_mock" ~/.xavier/ .env* config*` | **Sin resultados** — token nunca en disco plano |
| 5 | `xavier nodes show {node_id}` | Token mostrado como `REDACTED`/ref; solo metadata + health |
| 6 | `xavier nodes rotate {node_id}` | Nuevo lease activo; lease anterior revocado en Clavis |
| 7 | `xavier nodes remove {node_id}` | Lease revocado; nodo fuera de `GET /mesh/public/nodes` |
| 8 | `curl http://localhost:8006/mesh/public/nodes` | Nodo `public` presente; nodo `private` ausente |
| 9 | Cross-wallet: nodo con otra billetera consulta el nodo `private` | Acceso denegado (test de aislamiento) |

### REQ-030 — SSH/VPS private nodes (Ola M7)

| # | Comando | Resultado esperado |
|---|---------|--------------------|
| 1 | `xavier nodes add --provider vps --ssh user@localhost --key ~/.ssh/id_ed25519_test` | edge-hive lite instalado; `node_id` registrado en la billetera |
| 2 | `xavier nodes list` | Nodo VPS con `visibility=private` (default) |
| 3 | `grep -r "PRIVATE KEY" ~/.xavier/ .env*` | **Sin resultados** — clave SSH solo en Clavis |
| 4 | `xavier nodes show {node_id}` | Challenge Ed25519 verificado; sesión con cifrado activo |
| 5 | Sync test: memoria escrita en nodo madre → leída en nodo VPS | Roundtrip OK con MeshSessionShare cifrado |
| 6 | Cross-wallet: agente con otra billetera intenta unirse | Rechazado (test de aislamiento) |
| 7 | `xavier nodes remove {node_id}` | Lease SSH revocado; heartbeat del agente rechazado |
