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
- **SRS Estado:** `implemented` (**95%** — E2E+unit verdes; residual ops Amoy + UI Maloca)  
- **Files:** `src/node_identity/`, `src/polygon_anchor/`, `src/mesh/{challenge,namespace,pro_gate}.rs`, `src/cli/commands/node.rs`  
- **Feature:** `feat-decentralized-login` · Issues: `.gitcore/issues/login/` · Evidence: `TEST_EVIDENCE.md`

### Descripción
Login local sin cuenta central: BIP39-24 + Shamir 2-of-3 + vault; challenge mesh; anclas Polygon (solo hashes); firmas híbridas Ed25519+ML-DSA commitment. Pro = nodo activo, nunca Stripe. Mesh ≠ blockchain.

### Criterios de aceptación
- [x] Crear/recuperar nodo vía CLI sin servidor de cuentas
- [x] Seed nunca en logs / mesh / on-chain
- [x] Challenge-response Ed25519 + commitment ML-DSA
- [x] Anchor dry-run / live-prepared / broadcast (`dao-evm`)
- [x] E2E pipeline `decentralized_login_e2e` (5/5 PASS, 2026-07-28)
- [ ] Deploy Amoy + smoke live (ops)
- [ ] UI Maloca `obtainDeviceKeyViaWebAuthn` (producto)

### Trazabilidad fases ↔ issues ↔ % validados

| Fase | Issue | % | Tests |
|------|-------|---|-------|
| F0 | DL-01 | 95% | node_identity 16 + persist 2 + E2E F0 |
| F1 | DL-02 | 95% | challenge/ns/pro_gate 10 + E2E F1 |
| F2 | DL-03 | 90% | polygon_anchor 8 + E2E F2 |
| F3 | DL-04 | 100% | hybrid_pack + E2E F3 |
| F4 | DL-05 | 5% | ADR research |
| Apps | DL-06 | 90% | `@swal/node` 12 |

---

*Add domain-specific REQ-009+ below. Keep numbering stable. Updated 2026-07-28.*

