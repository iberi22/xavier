# Software Requirements Specification — xavier

> **Protocol:** GitCore 3.8.0 · **Updated:** 2026-07-17  
> IEEE 830 reduced. Structure **100%**. Keep REQ-IDs in sync with code.

## REQ-001: Protocol compliance (GitCore)

- **Category:** Process  
- **Priority:** High  
- **SRS Estado:** `draft`  
- **Files:** `AGENTS.md`, `.gitcore/ARCHITECTURE.md`, `.git-core-protocol-version`, `SRC.md`, `docs/SRS/`

### Descripción
El repositorio cumple GitCore 3.8.0: lectura de agentes, planning local, SRC y SRS presentes.

### Criterios de aceptación
- [ ] `.git-core-protocol-version` = 3.8.0
- [ ] `AGENTS.md` define orden de lectura
- [ ] `.gitcore/planning/PLANNING.md` y `TASK.md` existen
- [ ] `SRC.md` completo (secciones obligatorias)
- [ ] `docs/SRS/{index,REQUIREMENTS,ARCHITECTURE}.md` existen

---

## REQ-002: Source map (SRC)

- **Category:** Documentation  
- **Priority:** High  
- **SRS Estado:** `draft`  
- **Files:** `SRC.md`

### Descripción
SRC.md describe árbol real, build/test, y enlaces a SRS/.gitcore.

### Criterios de aceptación
- [ ] Tree refleja módulos reales
- [ ] Comandos build/test documentados
- [ ] Cross-links a docs/SRS y AGENTS.md

---

## REQ-003: SWAL node Pro gate (product apps)

- **Category:** Functional  
- **Priority:** High (N/A for pure libraries)  
- **SRS Estado:** `draft`  
- **Files:** *(app-specific Pro gate modules)*

### Descripción
Funciones Pro se habilitan solo con **nodo SWAL activo**. No Stripe para Pro.

### Criterios de aceptación
- [ ] No hay Checkout/webhook Stripe como unlock Pro
- [ ] Gate documentado Free vs Pro
- [ ] Heartbeat/identidad de nodo definido o planificado

---

## REQ-004: Instance isolation (mesh / multi-workspace)

- **Category:** Functional  
- **Priority:** High (N/A if single-tenant tool)  
- **SRS Estado:** `draft`  
- **Files:** *(storage / mesh namespace modules)*

### Descripción
Dos instancias de la misma app no mezclan datos de negocio por defecto. Namespace `swal/{app_id}/{instance_id}`.

### Criterios de aceptación
- [ ] `instance_id` persistido por workspace
- [ ] Sync cruzado solo con vínculo opt-in
- [ ] Memoria Xavier namespaced por instance

---

## REQ-005: Agentic memory (Xavier)

- **Category:** Functional  
- **Priority:** High  
- **SRS Estado:** `draft`  
- **Files:** *(xavier client / config)*

### Descripción
Memoria agentic vía Xavier HTTP (`:8006`) y/o **MCP**, fuera de la BD de negocio.

### Criterios de aceptación
- [ ] Paths de memoria documentados
- [ ] No se persiste working memory agentic solo en DB de dominio
- [ ] Fallo de Xavier no corrompe datos de negocio

---

## REQ-006: Security & secrets

- **Category:** Non-functional  
- **Priority:** High  
- **SRS Estado:** `draft`  
- **Files:** `.gitignore`, `.env.example`, `SECURITY.md` (if any)

### Descripción
Sin secretos en git; `.env.example` sin valores reales.

### Criterios de aceptación
- [ ] `.env` gitignored
- [ ] No API keys en docs de ejemplo
- [ ] Repo **private** salvo excepción documentada

---

## REQ-007: Local CI preference

- **Category:** Process  
- **Priority:** Medium  
- **SRS Estado:** `draft`  
- **Files:** `.github/workflows.disabled/` (if present)

### Descripción
GitHub Actions desactivados por defecto en era privada SWAL; tests locales preferidos.

### Criterios de aceptación
- [ ] Workflows no se ejecutan en GitHub (disabled/moved)
- [ ] Comandos de test locales en SRC.md

---

## REQ-008: Decentralized login / node identity (SWAL)

- **Category:** Identity / Security  
- **Priority:** High  
- **SRS Estado:** `implemented` (~95% — residual ops/UI)  
- **Files:** `src/node_identity/`, `src/polygon_anchor/`, `src/mesh/{challenge,namespace,pro_gate}.rs`, `src/cli/commands/node.rs`  
- **Feature:** `feat-decentralized-login` · Docs: `.gitcore/docs/DECENTRALIZED_LOGIN.md`

### Descripción
Login local sin cuenta central: BIP39-24 + Shamir 2-of-3 + vault; challenge mesh; anclas Polygon (solo hashes); firmas híbridas Ed25519+ML-DSA commitment. Pro = nodo activo, nunca Stripe. Mesh ≠ blockchain.

### Criterios de aceptación
- [x] Crear/recuperar nodo vía CLI sin servidor de cuentas
- [x] Seed nunca en logs / mesh / on-chain
- [x] Challenge-response Ed25519 + commitment ML-DSA
- [x] Anchor dry-run / live-prepared / broadcast (`dao-evm`)
- [ ] Deploy Amoy + smoke live (ops)
- [ ] UI Maloca `obtainDeviceKeyViaWebAuthn` (producto)

### Trazabilidad fases
| Fase | % | REQ map |
|------|---|---------|
| F0 | 95% | vault/BIP39/Shamir/device_key |
| F1 | 95% | mesh challenge / Pro gate |
| F2 | 90% | Polygon anchors |
| F3 | 100% | hybrid packs |
| F4 | research | ADR-SWAL-BIO-ZKP (fuera del 95%) |

---

*Add domain-specific REQ-009+ below. Keep numbering stable. Updated 2026-07-28.*

