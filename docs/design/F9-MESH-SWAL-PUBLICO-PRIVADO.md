# Xavier Mesh — Red SWAL Pública + Meshes Privadas (F9 v2)

> **Estado: DISEÑO — requiere aprobación antes de implementar** (protocolo cores críticos)
> Fecha: 2026-08-08 · Autor: Hermes orquestador · Versión: 0.1 (propuesta)

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

## 5. Decisiones de diseño (a validar)

| Decisión | Opción A | Opción B | Recomendación |
|---------|----------|----------|---------------|
| Transporte | Iroh QUIC (ya implementado) | libp2p (roto) | **A** — Iroh ya funciona |
| Directorio público | Endpoint HTTP en nodo madre | DHT global | **A** — simple, SWAL es la primera red |
| Billetera | Clavis (KeyLendingEngine) | Nueva | **A** — Clavis ya existe |
| Nodo remoto | Edge-hive (19 crates) | edge-mesh TS | **Edge-hive** para nodos pesados, edge-mesh para apps |
| Snapshot | CodeGraph DB + hash | Git clones | **CodeGraph** — sin git, abstracción real |

## 6. Riesgos y mitigaciones

| Riesgo | Mitigación |
|--------|-----------|
| 0 peers activos hoy | La ola M1 arranca con el nodo madre como primer peer; pruebas con 2 nodos locales |
| libp2p roto | Usar Iroh (funciona); no arreglar libp2p legacy |
| Seguridad del directorio público | Heartbeat firmado Ed25519; solo nodos registrados; rate limit |
| Privacidad de la mesh | Claves por billetera; cifrado de sesión; no publicar direcciones privadas |
| Tamaño de snapshots | Solo símbolos+edges (no archivos completos); incremental por hash |

## 7. Criterios de aceptación (F9 v2)

- [ ] 2 nodos Xavier locales se descubren vía el directorio público (sin config manual de IP)
- [ ] Nodo B hace `POST /mesh/public/rag` contra nodo A y obtiene snippets de su CodeGraph
- [ ] El árbol público muestra profundidad (repos → snapshots → símbolos)
- [ ] 2 nodos de la MISMA billetera forman mesh privada; un tercero NO los ve
- [ ] Un issue se genera con el fragmento EXACTO (línea/función) del snapshot
- [ ] El agente ejecutor aplica el cambio con diff mínimo (métrica de tokens)

## 8. No-goals (esta fase)

- NO mercado público de nodos (es F12 Data Marketplace)
- NO monetización / tokenomics on-chain (ya hay diseño separado)
- NO reemplazo completo de GitHub para humanos — solo para agentes/CLIs
- NO multi-wallet complejo (una billetera por mesh privada en v1)
