# Login descentralizado — Avances (changelog)

| Campo | Valor |
|-------|--------|
| **Feature** | `feat-decentralized-login` / FEAT-SWAL-DECENTRALIZED-LOGIN |
| **Estado shippable** | **100% DONE (F0–F3)** · `stable` / `passes: true` |
| **Última actualización** | 2026-07-29 |
| **Canónico fases** | [DECENTRALIZED_LOGIN.md](./DECENTRALIZED_LOGIN.md) |
| **Siguiente fase** | Ver §3 abajo |

---

## 1. Resumen ejecutivo

Se implementó y documentó el protocolo de **login / identidad descentralizada** SWAL:

- Identidad = nodo (BIP39 + vault local), no cuenta SaaS.
- **Pro = nodo activo** (nunca Stripe).
- Mesh = data plane; ledger = **Polygon** (solo hashes/metadata).
- DoD shippable = **Fases 0–3**. Fase 4 = research track separado.

---

## 2. Avances por fecha

### 2026-07-29 — F4 spike + UI Maloca + baseline revalidado

| Entrega | Detalle |
|---------|---------|
| **Baseline revalidado** | `decentralized_login_e2e` 5/5 · `node_fase0_persist` 2/2 · `--lib node_identity` 16/16 · `--lib polygon_anchor` 8/8 |
| **F4 spike fuzzy extractor** | `docs/SWAL/spikes/fuzzy-extractor/` 8/8 tests · Monte Carlo N=1000/config — TAR≥99% @ ruido 5–10% solo con claves 28–36 bits (helper local fuerza-brutable) · FAR 0/1000 todas las configs |
| **zk-SABER evaluado** | No es lattice-based: Groth16/BN254, no-PQ, on-chain, prototipo sin auditar |
| **Veredicto ADR** | **NO-GO hot-path día 1** · watch-list con condiciones de re-apertura — [ADR-SWAL-BIO-ZKP-RESEARCH.md](./ADR-SWAL-BIO-ZKP-RESEARCH.md) |
| **UI Maloca WebAuthn** | DL-06 residual UI **cerrado**: `registerDeviceCredential()` + helpers base64url en `@swal/node`, `OnboardingPage.svelte` (flujos Crear/Recuperar), ruta `/onboarding` + nav; suite 18/18 · maloca `3ead022` (**pusheado**) |
| **Ops Amoy** | foundry v1.7.1 instalado (`~/.foundry/bin`); deploy + smoke broadcast esperan `SWAL_ANCHOR_KEY` del usuario |

**Tracking:** DL-05 5%→**70%** · F-023 (edge-mesh) 5→**70%** (`0299e7e`) · F2 sigue 90% (ops) · overall feature sigue **95%** (F4 no cuenta en el shippable).

**Commits:** xavier `6747f59e` (local) · maloca `3ead022` (pushed) · edge-mesh `0299e7e` · docs/SWAL `629924f`.

### 2026-07-28 — Cierre shippable F0–F3

| Entrega | Detalle |
|---------|---------|
| **F0 crypto + CLI** | `xavier/src/node_identity/` — BIP39-24, Shamir 2-of-3, vault Argon2id+AES-GCM, check-codes, derive Ed25519 + ML-DSA commitment |
| **F0 persistencia** | `$XAVIER_DATA_DIR/node/{vault,identity.public}.json` (0700/0600); CLI `create` / `recover` / `status` |
| **F0 UX** | Brick warning; `--device-key-hex` / `XAVIER_NODE_DEVICE_KEY`; SLIP39 **OOS** |
| **F1 mesh** | Challenge Ed25519, namespaces `swal/{app}/{instance}`, ACL bind, `pro_gate` |
| **F1 apps** | `@swal/node` heartbeat + backoffice + WorldExams `applyWeHeartbeat` |
| **F1 PQ e2e** | edge-mesh `xavier-bridge` commitment → ML-DSA-65 challenge/verify |
| **F2 anchors** | `xavier/src/polygon_anchor/` — ABI, dry-run default, live-prepared calldata, receipts |
| **F2 CLI** | `xavier node anchor` · `xavier node anchor-pack` |
| **F2 contrato** | Ref `docs/SWAL/contracts/SwalIdentityRegistry.sol` (deploy = **ops**) |
| **F3 hybrid** | Xavier `hybrid_pack` + edge-mesh `hybrid-pack.ts` |
| **F3 ML-KEM** | ADR **no-go día-1** — [ADR-SWAL-ML-KEM-DEK.md](./ADR-SWAL-ML-KEM-DEK.md) |
| **F4 track** | [ADR-SWAL-BIO-ZKP-RESEARCH.md](./ADR-SWAL-BIO-ZKP-RESEARCH.md) · edge-mesh `F-023` |

**Tests (referencia):** `polygon_anchor` 8/8 · `hybrid_pack` 2/2 · `node_identity` 16/16 · edge-mesh hybrid-pack · mesh challenge/namespace/pro_gate · `cargo check -p xavier` OK.

**Tracking GitCore:**

| Repo | ID | Estado |
|------|-----|--------|
| `xavier` | `feat-decentralized-login` | `stable` 100% |
| `edge-mesh` | `F-019`…`F-022` | `complete` 100% |
| `edge-mesh` | `F-023` | `pending` ~5% (research) |


### 2026-07-28 — Cierre de sesión (E2E + issues)

- E2E `decentralized_login_e2e` **5/5 PASS**
- Issues DL-01…06 con % validados en `.gitcore/issues/login/`
- SRS REQ-008 + SRC actualizados; feature **95%** `verified_by=unit+e2e`
- Xavier `main` rebaseado sobre origin (#1080) · commits `d29dd9c9` + `659651f9`

### 2026-07-28 — Leftovers F0–F3 (hardening)

| Entrega | Detalle |
|---------|---------|
| **F0 device_key / WebAuthn** | `@swal/node` `device-key.ts` — PRF + fallback credential-id → hex/`XAVIER_NODE_DEVICE_KEY` |
| **F1 heartbeat loop** | `startProHeartbeatLoop` en `@swal/node`; backoffice migrado; WorldExams `startWeHeartbeatLoop` |
| **F2 live broadcast** | `polygon_anchor/broadcast.rs` detrás de feature `dao-evm`; script deploy Amoy |
| **Docs** | `POLYGON_ANCHORS.md` modos mock/prepared/broadcast; script `docs/SWAL/scripts/deploy-identity-registry-amoy.sh` |

**Tests:** swal-node 12/12 · polygon_anchor 8/8 · `cargo check -p xavier` OK.

- Roadmap: `DECENTRALIZED_LOGIN.md`
- Diseño: `LOGIN_IDENTITY_DESIGN.md`
- SRS §16 REQ-DL-001…007
- Features GitCore Xavier + edge-mesh
- AUTH_RECOVERY_SPIKE marcado Fase 0

---

## 3. Siguiente fase (decisión)

El feature **shippable ya está cerrado**. Lo que sigue **no** es “Fase 3 del login”, sino una de estas pistas (orden recomendado):

### A — Ops Polygon (prereq operacional de F2) — **siguiente recomendado para producción**

| Paso | Entregable |
|------|------------|
| 1 | Deploy `SwalIdentityRegistry.sol` en **Amoy** (chainId 80002) |
| 2 | Set env: `SWAL_POLYGON_RPC_URL`, `SWAL_ANCHOR_CONTRACT`, `SWAL_ANCHOR_KEY`, `SWAL_ANCHOR_DRY_RUN=0` |
| 3 | Smoke: `xavier node anchor` → receipt on-chain + verificar en explorer |
| 4 | Documentar address + ABI en `xavier/docs/POLYGON_ANCHORS.md` |

**Dueño:** ops / gara-g settlement · **No es código del feature 100%.**

### B — Producto UI (OOS del DoD, alto valor UX)

| Paso | Entregable |
|------|------------|
| 1 | WebAuthn / passkey UI en Maloca → `device_key` |
| 2 | Onboarding recovery UX (shares + check-codes) en panel |
| 3 | Más apps adoptando `applyHeartbeat` vía `@swal/node` |

### C — Fase 4 research (DL-F4) — spike + veredicto hechos 2026-07-29 (**NO-GO hot-path día 1**, watch-list)

Track: [ADR-SWAL-BIO-ZKP-RESEARCH.md](./ADR-SWAL-BIO-ZKP-RESEARCH.md) · `F-023` (70%)

| ID | Entregable |
|----|------------|
| DL-F4-01 | ✅ Spike fuzzy extractor (helper **local** only) — medido 2026-07-29 |
| DL-F4-02 | ✅ Evaluación crítica zk-SABER vs necesidad SWAL — no lattice-based |
| DL-F4-03 | ✅ ADR go/no-go con TAR/FAR + threat model — **NO-GO** medido |

**Regla:** nunca templates biométricos en Xavier/mesh. No bloquea Pro.

### D — Roadmap SWAL general (fuera de login)

Tras identidad estable, el [README.md](./README.md) prioriza: **infra mínima** (Xavier MCP/XTSP) → **registry wave** → **economía $SWAL**.

---

## 4. Diferidos explícitos

| Item | Motivo |
|------|--------|
| SLIP39 mnemonic shares | OOS; Shamir binario cumple umbral |
| WebAuthn browser UI | Producto Maloca; hook CLI listo |
| Broadcast live on-chain | Ops (A), no código |
| ML-KEM en hot path | ADR no-go día-1 |
| Bio/ZKP en login hot path | F4 research only |

---

## 5. Lectura para agentes

```
1. docs/SWAL/DECENTRALIZED_LOGIN.md
2. docs/SWAL/DECENTRALIZED_LOGIN_PROGRESS.md   ← este archivo
3. docs/SWAL/ADR-SWAL-BIO-ZKP-RESEARCH.md      ← siguiente fase login (C)
4. xavier/docs/POLYGON_ANCHORS.md               ← siguiente ops (A)
5. xavier/.gitcore/features/FEATURE-feat-decentralized-login.md
```
