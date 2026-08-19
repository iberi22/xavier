# ADR-015: SWAL Node Provisioning — BaaS tokens + SSH keys (Olas M6/M7)

*Status: PROPOSED (DISEÑO) | Date: 2026-08-14*

---

## Contexto

La visión F9 (`docs/design/F9-MESH-SWAL-PUBLICO-PRIVADO.md`) define olas M1–M5
para la red SWAL pública y las meshes privadas. Para que la red escale sin
fricción, falta la pieza de aprovisionamiento: **que cualquier usuario o
empresa pueda crear nodos rápido con su token**. Dos familias de nodo cubren el
espectro de soberanía/coste:

- **Nodos BaaS (Ola M6):** Supabase/Neon como nodos persistentes administrados.
  El usuario solo pega un API token; Xavier configura el provider vía su API.
- **Nodos privados VPS (Ola M7):** infraestructura propia del usuario, alcanzada
  por SSH; Xavier instala un agente ligero (edge-hive lite) y lo ancla a la billetera.

Restricciones heredadas (no negociables): sin Stripe, nodo = wallet = identidad
soberana, mesh ≠ blockchain, cero dependencias nuevas innecesarias en el core,
y secreto primero (tokens/claves nunca en disco plano).

---

## Decisiones de Aceptación (Aceptados)

### 1. `src/secrets/` como ÚNICO gestor persistente de secrets de nodos
- **Estado**: **ACEPTADO** (revisado 2026-08-14 con validación Kimi k3 — hallazgo P0)
- **Decisión**: Los tokens BaaS (Supabase/Neon) y las claves SSH dedicadas se
  almacenan en **`src/secrets/`** — `LocalSecretsVault`/`HardwareVault`
  (AES-256-GCM real, persistencia en disco cifrada vía `MasterKeyManager`) —
  con **`KeyLendingEngine` + `EphemeralLease`** (session_token + real_secret_id
  + agent_id + expires_at) como capa de leases con TTL. Rotación y revocación
  vía `xavier nodes rotate|remove`.
- **⚠️ Reality check (validación Kimi 2026-08-14)**: `src/clavis/mod.rs`
  (`ClavisEngine`) es **volátil en memoria** (`RwLock<HashMap>`), sin
  persistencia AES-GCM y sin leases — NO es el store para credenciales de
  nodos. El nombre correcto de la struct de lease es `EphemeralLease`
  (`src/secrets/lending.rs`), NO `SecretLease`. **Requisito previo de M6/M7:
  las credenciales de nodos deben persistir en `src/secrets/` antes de que
  cualquier nodo pueda sobrevivir un reinicio de Xavier.** El diseño no mezcla
  ambos módulos: ClavisEngine (proxy/rotación efímera) vs secrets (store
  cifrado persistente).
- **Justificación**: `src/secrets/` ya implementa AES-256-GCM en disco
  (LocalSecretsVault/HardwareVault con fallback a keyring). Reutilizarlo
  garantiza "tokens nunca en disco plano" Y "tokens sobreviven reinicios".
- **Mapeo al Código**: `src/secrets/local_vault.rs` (AES-GCM), `src/secrets/vault.rs`
  (HardwareVault keyring), `src/secrets/lending.rs` (EphemeralLease/KeyLendingEngine),
  `src/secrets/audit.rs` (audit logger — requiere estructuración + masking).

### 2. Supabase/Neon = nodos BaaS administrados
- **Estado**: **ACEPTADO**
- **Decisión**: El provisioning se hace vía la API del provider: Supabase
  (RLS policies, bucket cifrado, edge functions relay/heartbeat) y Neon
  (schema de nodo + replicación). Supabase actúa además como nodo persistente
  público de administración SWAL: `node_registry` (RLS anon READ), `ops_feed`
  (público replicable), bucket `swal-vault` (privado, JSON cifrados E2E).
- **Justificación**: Onboarding de un solo paso (pegar el token). El provider
  es relay/persistencia, NO autoridad: la data viaja cifrada E2E y la autoridad
  reside en la billetera (mesh ≠ blockchain).

### 3. VPS = nodos privados vía edge-hive lite
- **Estado**: **ACEPTADO** (revisado 2026-08-14 — hallazgos P0 Kimi #4/#5)
- **Decisión**: `xavier nodes add --provider vps --ssh user@host` instala un
  agente ligero (edge-hive lite: subconjunto identity/sync/tunnel) y lo
  registra en la billetera con challenge-response Ed25519 (protocolo M3).
  Visibility default `private`.
- **⚠️ SSH key dedicada (P0)**: **PROHIBIDO importar la clave personal del
  usuario** (`~/.ssh/id_ed25519`). Xavier **genera un keypair dedicado por
  nodo** en el momento del provisioning; instala SOLO la pubkey vía el acceso
  existente del usuario (una vez); Clavis guarda únicamente la clave dedicada.
  Revocar el nodo no afecta ningún otro acceso del usuario.
- **⚠️ Host key pinning (P0, anti-MITM)**: verificación TOFU con fingerprint
  del host key del servidor, guardado en el registro del nodo y verificado en
  cada conexión posterior; flag opcional `--host-key` para pinning estricto.
- **⚠️ Cadena de suministro (P1)**: edge-hive lite se distribuye con firma +
  checksum verificado en destino ANTES de ejecutar; usuario dedicado no-root;
  sudoers mínimo o nulo; systemd unit con hardening.
- **Justificación**: Reutiliza el stack edge-hive ya existente; la información
  privada del mesh interno persiste en infraestructura del usuario bajo herencia
  de permisos de la billetera.

### 4. Visibilidad pública/privada por nodo + sync Yjs CRDT
- **Estado**: **ACEPTADO** (revisado 2026-08-14 — hallazgos P1 Kimi #8/#10)
- **Decisión**: Cada nodo BaaS declara `visibility`: `public` → directorio M1
  (`GET /mesh/public/nodes`); `private` → solo visible para la billetera (M3).
  **`private` es el default en TODOS los providers; `--visibility public`
  siempre explícito** (P2). La info pública de la mesh se replica a nodos mesh
  locales como documentos Yjs CRDT; `ops_feed` actúa de relay store&forward.
- **⚠️ Escritura del directorio público (P1)**: `node_registry` con RLS anon
  READ pero escritura SOLO vía edge function que verifica la firma Ed25519 del
  heartbeat contra `node_id = hash(pubkey)`; la service key del provider vive
  SOLO en la edge function, nunca en clientes. RLS write = denegado para todo
  rol externo.
- **⚠️ Updates Yjs firmados (P1)**: convergencia CRDT ≠ validez. Cada update
  Yjs del `ops_feed` lleva firma Ed25519 del nodo emisor + vector clock
  monótono; al aplicar se valida firma y frescura (rechazar updates viejos =
  anti rollback attack). `ops_feed` en Supabase almacena blobs opacos firmados.
- **Justificación**: edge-mesh ya usa Yjs/y-protocols. Mantiene la separación
  red pública / mesh privada sin exponer direcciones privadas.

### 5. Ciclo de vida de credenciales (rotación y revocación) — P0 Kimi #2/#3
- **Estado**: **ACEPTADO** (revisado 2026-08-14)
- **Decisión**: Dos flujos separados de rotación:
  - **`nodes rotate`** = el usuario provee un token NUEVO (o Xavier llama a la
    API del provider para emitir uno nuevo y revocar el viejo). El generador
    local de Clavis (`clavis_{name}_{uuid}`) produce credenciales INVALIDAS
    para providers externos — **prohibido usarlo para tokens BaaS**.
  - **TTL expirado** → estado `degraded` + solicitud de re-auth al usuario con
    grace period; comportamiento fail-closed (el agente rechaza nuevas sesiones
    y solo re-autentica tras rotate). Nunca rotación local silenciosa.
- **⚠️ Revocación NO es local-only (P0)**: `nodes remove` debe incluir
  deprovisioning real: revocar el token vía management API del provider
  (Supabase/Neon) y teardown SSH (desinstalar agente + borrar pubkey dedicada
  de `authorized_keys`). Si la revocación remota falla, reportar
  **"revocación parcial"** explícitamente — nunca éxito falso.
- **⚠️ Re-key de mesh al revocar (P1)**: al expulsar un nodo, la billetera
  emite una nueva epoch de clave de sesión MeshSessionShare distribuida a los
  nodos restantes (forward secrecy); declarar que el ciphertext histórico ya
  descifrado no es recuperable.

### 6. Certificado de nodo = mecanismo criptográfico de aislamiento (P0 Kimi #6)
- **Estado**: **ACEPTADO** (revisado 2026-08-14)
- **Decisión**: El aislamiento cross-wallet se garantiza criptográficamente:
  - **Certificado de nodo** = firma de la billetera sobre
    `(node_pubkey + node_id + expiry)`.
  - Derivación de node key: `HKDF(wallet_secret, node_id)` documentada.
  - Handshake M3 exige certificado válido emitido por la billetera; nodos con
    certificados de billeteras distintas NO convergen en la misma mesh.
  - Compromiso de una node key NO revela la billetera ni otras node keys.
- **Justificación**: sin este mecanismo, "nodos de la misma billetera" es una
  ambigüedad de implementación; el certificado lo convierte en una propiedad
  verificable. Requiere ceremonia de pairing definida (quién firma: la
  billetera firma un cert para el pubkey del nodo).

---

## Decisiones de Rechazo (Rechazados)

### 1. Gestor de secretos externo (Vault/HashiCorp, AWS Secrets Manager, .env plano)
- **Estado**: **RECHAZADO**
- **Justificación**: Introduce dependencia externa y rompe la soberanía local-first.
  Clavis ya cubre AES-256-GCM + leases TTL + rotación. Un `.env` plano viola la
  regla de seguridad y REQ-006.

### 2. Stripe/pago como gate del provisioning
- **Estado**: **RECHAZADO (Violación de AGENTS.md)**
- **Justificación**: Pro y participación de red se rigen por nodo SWAL activo y
  $SWAL, no por pasarelas web2. `AGENTS.md` prohíbe Stripe explícitamente.

### 3. Provider BaaS como autoridad / blockchain
- **Estado**: **RECHAZADO**
- **Justificación**: El provider es relay store&forward. La autoridad, las claves
  E2E y la identidad viven en la billetera del usuario. Mesh ≠ blockchain.

### 4. Agente VPS con acceso a la billetera completa
- **Estado**: **RECHAZADO**
- **Justificación**: El agente remoto opera con una node key derivada del pairing;
  la billetera solo vive en el dispositivo del usuario (+ vault). Minimiza el
  radio de explosión si el VPS se compromete.

---

## Herencia de permisos (vault + Clavis + edge-mesh)

- **swal-vault** (Flutter): administra la billetera (identidad de nodo) + claves E2E.
- **Clavis** (Xavier): administra tokens BaaS + claves SSH (leases, TTL, revoke).
- **edge-mesh**: authz + cifrado de sesión entre nodos privados.
- Cadena: **mesh privado → almacenamiento cifrado → bucket cloud**. El bucket
  hereda la ACL de la mesh; nada se cifra con claves del provider.

---

## Consecuencias

- **Positivas**: onboarding de nodos en un paso; secretos centralizados y rotables;
  reutiliza `src/secrets/` + edge-hive + Yjs sin dependencias nuevas; preserva soberanía.
- **Negativas/riesgos**:
  - Dependencia operativa de la API del provider (mitigado: el provider es
    relay reemplazable).
  - Superficie SSH en VPS (mitigado: lease TTL, revocación con deprovisioning,
    node key acotada, keypair dedicado).
  - Tokens solo en CLI para tests con mocks — en producción se leen de
    stdin/prompt/`XAVIER_NODE_TOKEN` env para no caer en shell history ni `ps` (P1 #11).
  - Audit de ciclo de vida: eventos add/rotate/remove/lend pasan por audit log
    estructurado append-only con masking (`ClavisLogMasker`), no `println!` plano (P1 #12).
  - Recuperación ante pérdida de `~/.xavier/secrets`: runbook de re-paste de
    token + re-pairing de nodos con nuevos certificados (P1 #14).
- **Seguimiento**: implementar Olas M6/M7 según `docs/design/F9-MESH-SWAL-PUBLICO-PRIVADO.md`
  §3.8/§3.9; validar ACs de REQ-029..030 antes de marcar `feat-node-provisioning`
  como `implemented`.

---

## Referencias

- `docs/design/F9-MESH-SWAL-PUBLICO-PRIVADO.md` §3.8, §3.9, Olas M6/M7
- `docs/SRS/REQUIREMENTS.md` REQ-029, REQ-030
- `docs/SRS/USER-STORIES.md` US-042, US-043
- `.gitcore/features.json` → `feat-node-provisioning`
- `AGENTS.md` (no Stripe, nodo = wallet, mesh ≠ blockchain)
