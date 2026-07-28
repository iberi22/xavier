# Diseño técnico — Identidad y login descentralizado SWAL

| Campo | Valor |
|-------|--------|
| **ID** | SRC-SWAL-LOGIN-IDENTITY |
| **Tipo** | Diseño fuente / arquitectura (capa SWAL) |
| **Fecha** | 2026-07-28 |
| **Roadmap** | [DECENTRALIZED_LOGIN.md](./DECENTRALIZED_LOGIN.md) |
| **SRS** | [SRS.md](./SRS.md) §2, §16 |

Este documento describe **cómo** se diseña el login (no el backlog). Para fases y DoD ver el roadmap de features.

---

## 1. Principios de diseño

1. **Nodo = identidad** — par de claves; no “usuario/password” central.
2. **Sin servidor de login** — challenge-response; vault local.
3. **Mesh ≠ blockchain** — edge-mesh transporta y sincroniza; **Polygon** ancla metadata.
4. **Off-chain first** — ciphertext / sealed packs fuera de chain; on-chain: hash, pubkey, CID, epoch.
5. **Estándares > crypto casera** — BIP39, SLIP39/Shamir, WebAuthn, ML-DSA (FIPS 204), ML-KEM (FIPS 203) si aplica.
6. **Pro = nodo activo** — heartbeat + identidad; **nunca** Stripe.

---

## 2. Arquitectura lógica

```
┌─────────────────────────────────────────────────────────────┐
│ App PWA / Maloca onboarding                                 │
│  · UX seed / recovery / WebAuthn                            │
└──────────────────────────┬──────────────────────────────────┘
                           │ derive keys
                           ▼
┌─────────────────────────────────────────────────────────────┐
│ Vault local (device)                                        │
│  · BIP39-24 entropy (+ passphrase BIP39 opcional)           │
│  · Seal(seed) con device key + PIN (Argon2id)               │
│  · Shares SLIP39 / Shamir 2-of-3 (backup físico/social)     │
└──────────────────────────┬──────────────────────────────────┘
                           │
           ┌───────────────┼───────────────┐
           ▼               ▼               ▼
     Ed25519 nodo    ML-DSA-65 id     Wallet / $SWAL
     (compat)        (mesh canónico)  (settlement)
           │               │
           └───────┬───────┘
                   ▼
┌─────────────────────────────────────────────────────────────┐
│ edge-mesh — data plane                                      │
│  · Signed nonce challenge                                   │
│  · Namespaces swal/{app}/{instance}                         │
│  · CRDT / telemetría cifrada                                │
└──────────────────────────┬──────────────────────────────────┘
                           │ content_hash / pubkey commit
                           ▼
┌─────────────────────────────────────────────────────────────┐
│ Polygon (Amoy → mainnet → CDK) — metadata ledger            │
│  · Registry identidad / hashes sealed packs                 │
│  · NO payloads de negocio                                   │
└─────────────────────────────────────────────────────────────┘

Xavier: memoria agentic + ACL por identidad/nodo (fuera BD negocio).
```

---

## 3. Seed y recuperación (Fase 0)

### 3.1 Canónico

| Pieza | Estándar | Notas |
|-------|----------|-------|
| Master seed | BIP39 **24** palabras | Una sola frase; no 3×BIP39 concatenados |
| Passphrase | BIP39 25ª palabra (opcional) | Frase mental, no segundo BIP39 |
| Backup | SLIP39 o Shamir **2-of-3** | Spike: `spikes/sealed-pack/shamir_dek_spike.mjs` |
| Desbloqueo diario | WebAuthn + PIN | Biometría del **dispositivo**, no template en red |
| Check-codes | HMAC-SHA256(seed, "swal-recovery-v1") | Solo posesión/integridad; no entropy |

Flujo detallado: [AUTH_RECOVERY_SPIKE.md](./AUTH_RECOVERY_SPIKE.md).

### 3.2 Derivación de claves (conceptual)

```
master_seed
  → HKDF / BIP32-style domain separation (documentar en implementación)
      → ed25519_node_sk
      → ml_dsa65_seed
      → optional wallet path ($SWAL)
```

La implementación debe fijar **domain separation** explícita (cadenas de contexto) y tests de vectores.

---

## 4. Identidad mesh (Fase 1)

### 4.1 Código existente

- `edge-mesh/src/identity/` — ML-DSA-65 (`@noble/post-quantum`), challenge nonce firmado.
- Xavier mesh — madurez: HTTP transport alto; libp2p bajo; **onchain_gov 0%** (honesto).

### 4.2 Protocolo de login peer

1. Peer A publica `nodoId` + `parPublico` (ML-DSA).
2. Peer B emite challenge (nonce + TTL).
3. A firma; B verifica.
4. Sesión / ACL atada a `nodoId` + `instanceId`.

**No** hay path de “confianza social” que salte la prueba criptográfica.

### 4.3 DID

Día 1: **commitment** (`nodoId` ↔ pubkey) basta. Un DID Method W3C completo es opcional post–Fase 2 (ancla Polygon como registro).

---

## 5. Anclas on-chain (Fase 2)

| Vive on-chain | Vive off-chain |
|---------------|----------------|
| Pubkey / commitment de nodo | Seed, vault, biometría |
| content_hash / CID sealed pack | Ciphertext AES-GCM |
| Eventos de registro (epoch) | Xavier working memory |
| Settlement $SWAL (gara-g) | BD de negocio de apps |

Ledger: **solo Polygon** ([ADR-SWAL-MESH-GOV.md](./ADR-SWAL-MESH-GOV.md)).  
**Prohibido:** tratar edge-mesh como cadena de bloques interna con consenso BFT.

---

## 6. Post-quantum (Fase 3)

| Algoritmo | Rol SWAL | Estado |
|-----------|----------|--------|
| **ML-DSA-65** (Dilithium) | Identidad / firmas mesh | Parcial en edge-mesh |
| **Ed25519** | Compat / híbrido | En uso |
| **ML-KEM** (Kyber) | Encapsulado DEK opcional | Evaluación Rust (`ml-kem`, pqcrypto) |

Híbrido recomendado para sealed packs: verificar **ambas** firmas (clásica + PQ) hasta deprecar clásica por política de consejo.

---

## 7. Biometría y ZKP (Fase 4 — research)

### 7.1 Fuzzy extractors

- Primitiva clásica (Dodis et al.): helper data público + reproducción de clave desde biometría ruidosa **sin** guardar template en claro.
- Papers recientes (iris ~105 bits) muestran viabilidad **experimental**, no producto día 1.
- Regla SWAL: helper data **local**; nunca subir template a Xavier ni al mesh.

### 7.2 ZKP biométrico

| Nombre | Qué es | Uso SWAL |
|--------|--------|----------|
| **zk-SABER** | Paper IEEE BRAINS 2025 — zkSNARK + embeddings | Referencia research |
| **Pramaana / Z Auth** | Prototipos / hackathon Groth16 + WebAuthn | No dependencia de producto |
| “Bio-Rollup” | Esquema paper / marketing | No canónico |

Criterio go/no-go: amenaza de privacidad on-chain concreta + TAR/FAR + costo de prueba en dispositivo.

### 7.3 Relación con WebAuthn

WebAuthn (Fase 0) usa biometría del **SO/dispositivo** (passkeys). Eso **no** es fuzzy extractor ni ZKP; es el camino de producción inmediato.

---

## 8. Mapa a código (SRC traces)

| Concern | Path canónico |
|---------|----------------|
| Identidad PQ + challenge | `edge-mesh/src/identity/` |
| Namespaces / CRDT | `edge-mesh/src/` (sync, protocol) |
| Mesh memory / ACL Xavier | `xavier/src/mesh/` |
| Sealed pack spike | `docs/SWAL/spikes/sealed-pack/` |
| Economic / Polygon | `gara-g` (settlement); ADR §4 |
| Memoria agentic | `xavier` HTTP `:8006` / MCP |
| GOS domain P2P data | `gos-p2p-data` (producto GOS; no dueño de identidad) |
| Experimentos mesh | `mesh-core` (preferir edge-mesh) |

---

## 9. Requisitos no funcionales (resumen)

| NFR | Objetivo |
|-----|----------|
| Confidencialidad seed | Nunca logs, nunca telemetría, nunca MCP memory body |
| Aislamiento instancias | `instance_id` por defecto |
| Disponibilidad login | Offline-capable vault (Fase 0) |
| Resistencia cuántica | Roadmap híbrido (Fase 3); no bloquear Fase 0 |
| Auditabilidad | Hashes en Polygon (Fase 2) |
| Simulabilidad | Threat model + fallos de recovery documentados (SRS transversal) |

---

## 10. Referencias

- [DECENTRALIZED_LOGIN.md](./DECENTRALIZED_LOGIN.md) — fases y DoD  
- [AUTH_RECOVERY_SPIKE.md](./AUTH_RECOVERY_SPIKE.md) — esquema recovery  
- [ADR-SWAL-MESH-GOV.md](./ADR-SWAL-MESH-GOV.md) — capas y ledger  
- [NODE_PRO_AND_INSTANCES.md](./NODE_PRO_AND_INSTANCES.md) — Pro gate  
- NIST FIPS 203 (ML-KEM), FIPS 204 (ML-DSA)  
- BIP39 / SLIP39  
- Dodis et al., Fuzzy Extractors  
- zk-SABER (IEEE BRAINS 2025)
