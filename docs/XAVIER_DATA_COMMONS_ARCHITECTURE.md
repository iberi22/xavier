# Xavier Data Commons — Investigación y Arquitectura de Referencia

> Basado en investigación de mejores prácticas de la industria: Ocean Protocol, Streamr, EigenTrust, libp2p, OQS (Open Quantum Safe), NIST PQ estándares, y Data DAOs.

---

## 1. Wallet Post-Cuántica

### Stack Recomendado

| Capa | Tecnología | Estado | Justificación |
|------|-----------|--------|---------------|
| **KEM** | `oqs` crate (ML-KEM / Kyber-1024) | ✅ v0.11.0, 128K descargas | Bindings oficiales a liboqs de Open Quantum Safe |
| **Firmas** | `oqs` crate (ML-DSA / Dilithium-5) | ✅ Mismo crate | ML-DSA-65 (Dilithium 3) para wallet, ML-DSA-87 (Dilithium 5) para transacciones críticas |
| **Hardware** | `tpm-rs` (Windows TPM 2.0) | ✅ v0.1.0 | RSA-2048 dentro de TPM + EK certificates + PCR quote |
| **Cifrado simétrico** | `aes-gcm` + `chacha20` | ✅ Estándar | AES-256-GCM para metadata cifrada post-PQ KEM |

### Diseño de Key Hierarchy

```
TPM 2.0 (Hardware)
  └── SRK (Storage Root Key) — RSA-2048, NUNCA sale del chip
        └── Wallet Key (RSA-2048, cifrada por SRK)
              └── Seed derivado → Kyber-1024 keypair (KEM público)
                                → Dilithium-5 keypair (firmas)
                                → Ed25519 keypair (identidad mesh existente)
```

**Flujo de firma de transacción:**
1. `oqs::sig::Sig::new(Algorithm::MlDsa87)` — carga ML-DSA-87 (Dilithium 5)
2. `tpm_rs::TpmProvider::open()` — verifica TPM presente
3. `tpm_rs::TpmKeyPair::create_or_open()` — desbloquea wallet key vía TPM
4. Firma con Dilithium-5: `sig.sign(transaction_hash, &dilithium_sk)`
5. Atestación PCR opcional: `tpm_rs::quote::generate_quote()`

**Fallback software (sin TPM):**
- `oqs` crate sin hardware — Kyber/Dilithium en software puro
- Seed derivado de OS keychain (Windows Credential Manager / macOS Keychain)

### Referencias
- **NIST FIPS 203** (ML-KEM / Kyber): Estandarizado agosto 2024
- **NIST FIPS 204** (ML-DSA / Dilithium): Estandarizado agosto 2024  
- **Open Quantum Safe**: `oqs` crate v0.11.0, compatible con liboqs 0.11.0
- **tpm-rs**: Windows TBS API + TPM 2.0, RSA-2048, EK, PCR quote

---

## 2. Sistema de Reputación Descentralizada

### Algoritmo: EigenTrust Adaptado

**Por qué EigenTrust:** Es el algoritmo de reputación P2P más probado (Stanford, 2003), con implementaciones Rust reales y resistente a ataques Sybil y collusion.

### Arquitectura

```
Capa 1: Señales Locales (cada nodo)
├── Interacciones directas: 
│   ├── "+1" si el contexto comprado fue útil (fix aplicado con éxito)
│   ├── "-1" si el contexto era basura o no aplicaba
│   └── neutral si no hay feedback
│
├── Normalización: c_ij = max(s_ij, 0) / Σ max(s_ij, 0)
│
└── Pre-trusted peers: seed nodes de Xavier Core (bootstrap)

Capa 2: Cómputo Distribuido
├── Gossipsub: cada nodo publica su vector de confianza local
├── Power iteration: t^(k+1) = C^T × t^(k)
│   ├── t^(0) = vector uniforme (todos empiezan igual)
│   ├── Teletransporte: t^(k+1) = (1-a)C^T t^(k) + a × p
│   │   └── a = 0.15 (factor de probabilidad de resetear a pre-trusted)
│   └── Convergencia: ||t^(k+1) - t^(k)|| < 0.001
│
├── Reputación híbrida: mezcla de EigenTrust + contribución directa
│   ├── Trust score (EigenTrust): -1.0 a +1.0
│   └── Contribution score: data compartida, uptime, version actualizada
│       └── Reputación final = 0.7 × EigenTrust + 0.3 × Contribution
│
└── Periodicidad: cada 24 horas o después de N transacciones (>100)

Capa 3: Consenso
├── No hay necesidad de blockchain para esto
├── Los vectores de confianza se sincronizan vía gossip asíncrono
├── Cada nodo computa localmente (el algoritmo converge igual)
└── Sybil resistance: pre-trusted peers + costo mínimo de entrada (requiere contribución)
```

### Crate Recomendado

```toml
[dependencies]
# EigenTrust en Rust, Apache 2.0
eigentrust = { git = "https://github.com/Karma3Labs/rs-eigentrust-snaps", optional = true }

# Alternativa: implementación propia simplificada
# Menos features pero sin dependencias externas
```

### Referencias
- **EigenTrust paper**: Kamvar et al., Stanford 2003
- **Karma3Labs/rs-eigentrust-snaps**: Implementación Rust + distrust adjustment
- **OpenRank**: EigenTrust como servicio, multi-contexto
- **zk-EigenTrust**: Privacy & Scaling Explorations (Ethereum Foundation)

---

## 3. Tokenomics y Mercado de Datos

### Modelo Económico (Inspirado en Ocean Protocol + Streamr Data Unions)

**Principios:**
1. No es una crypto especulativa — es un **token de utilidad** para el ecosistema Xavier
2. El valor emana de la **utilidad real**: comprar contextos que mejoran la red
3. Mecanismo Burn-and-Mint equilibrado (BME, como GARA-G y Ocean)
4. Sin pre-mining — los tokens se mintean SOLO cuando hay contribución real

### Flujo Económico

```
               CONTRIBUCIÓN                    CONSUMO
         ┌──────────────────┐          ┌──────────────────┐
         │                  │          │                  │
    Nodo comparte      ┌────▼────┐    Nodo compra     ┌──▼────┐
    contexto técnico   │ MINTER  │    contexto útil   │ BURN  │
         │            └────▲────┘         │          └──▲────┘
         ▼                 │              ▼             │
    ┌─────────┐            │        ┌─────────┐        │
    │ 40%     │◄───────────┘        │ Tokens  ├────────┘
    │ Nodo    │   Tokens minteados  │ quemados│
    ├─────────┤                     ├─────────┤
    │ 40%     │                     │ 80%     │
    │ Usuario │                     │ Quemado │
    ├─────────┤                     ├─────────┤
    │ 20%     │                     │ 20%     │
    │ Red/Gov │                     │ Rewards │
    └─────────┘                     └─────────┘
```

### Parámetros Iniciales (inspirados en DAOs de datos 2026)

| Acción | Recompensa | Split | Nota |
|--------|-----------|-------|------|
| Compartir contexto de error crítico | 10 tokens | 40/40/20 | Error validado por >3 nodos |
| Compartir benchmark | 2 tokens | 40/40/20 | Sin validación cruzada |
| Responder encuesta técnica | 5 tokens | 50/30/20 | Más incentivo al humano |
| Reportar fix exitoso | 15 tokens | 30/50/20 | Alto valor al humano que reporta |
| Validar contexto de otro nodo | 3 tokens | 40/40/20 | Validación cruzada |
| Mantener nodo activo (>99% uptime/mes) | bonus 5 tokens | 40/40/20 | Al final del mes |

**Ajuste dinámico:** Usando el **Coverage Analyzer pattern** de GARA-G: los precios suben si hay poca oferta de cierto tipo de contexto, bajan si hay mucha. Mecanismo de mercado, no fijo.

### Supply

- **No hay supply fijo máximo** — los tokens se mintean solo cuando hay contribución (deflacionario por consumo)
- **Burn rate objetivo:** 80% de los tokens pagados por consumo se queman (como Ocean Protocol)
- **Inflación neta:** 0 o negativa si el consumo supera la contribución

### Precios Dinámicos de Contextos

```
Precio_base = Precio_referencia × (1 / Rareza) × Multiplicador_calidad

Donde:
- Rareza = # nodos que reportaron el mismo error / # total nodos
- Multiplicador_calidad = EigenTrust score del vendedor (0.1 - 1.0)
- Precio_referencia = 5 tokens (ajustable por gobernanza)

Ejemplo:
- Error raro (solo 1 nodo) con trust_score alto: 5 × (1/0.01) × 1.0 = 500 tokens
- Error común (50 nodos) con trust_score medio: 5 × (1/0.5) × 0.6 = 6 tokens
```

### Referencias
- **Ocean Protocol**: Data Farming, Compute-to-Data, buyback & burn
- **Streamr**: Data Unions, sponsorships, delegators
- **GARA-G**: Coverage Analyzer, pricing dinámico, BME model
- **Synapse Protocol**: Karma-weighted rewards, Soulbound identity

---

## 4. Protocolo P2P

### Stack Técnico

```
┌──────────────────────────────────────────────┐
│            XAVIER MESH (ya existe)            │
│  NodeIdentity (Ed25519) + PeerRegistry + HTTP │
├──────────────────────────────────────────────┤
│                                              │
│  Fase 2: libp2p overlay (cuando >20 nodos)    │
│  ┌──────────────────────────────────────────┐ │
│  │ libp2p-gossipsub (pub/sub de telemetría) │ │
│  │ libp2p-kad (DHT discovery)               │ │
│  │ libp2p-noise (cifrado obligatorio)       │ │
│  │ libp2p-relay (NAT traversal)             │ │
│  │ libp2p-dcutr (hole punching)             │ │
│  │ libp2p-identify (peer metadata)          │ │
│  └──────────────────────────────────────────┘ │
└──────────────────────────────────────────────┘
```

### Versiones Seguras (post-CVE 2026)

```toml
[dependencies]
libp2p = { version = "0.56", features = [
    "noise", "kad", "gossipsub", "relay", "dcutr", "identify", 
    "tcp", "tokio", "serde", "quic"  # QUIC mejora NAT traversal
], optional = true }

# CRÍTICO: gossipsub >= 0.49.4 (CVE-2026-33040, CVE-2026-34219)
libp2p-gossipsub = { version = "0.49", optional = true }
```

### Topología Recomendada

```
Arquitectura Híbrida (HTTP + libp2p):

Fase 1 (<20 nodos):
  ├── HTTP REST (ya implementado por Antigravity)
  ├── PeerRegistry en JSON
  ├── Handshake + Manifest + Chunks
  └── Push/Pull de chunks vía HTTP

Fase 2 (>20 nodos):
  ├── libp2p overlay sobre HTTP
  ├── GossipSub para broadcasts de telemetría
  ├── Kademlia DHT para discovery autónomo
  ├── Circuit Relay v2 para NAT traversal
  └── Hole punching (DCUtR) para peers detrás de NAT
```

### Mensajes P2P (Data Commons)

```rust
/// Protocolo v2 — Data Commons sobre libp2p
pub enum DataCommonsMessage {
    /// Contexto técnico disponible para la red
    ContextOffer {
        context_id: String,       // hash SHA-256 del contenido
        module: String,           // módulo afectado
        error_type: String,       // tipo de error
        rarity: f32,              // rareza (0.0 - 1.0)
        dilithium_sig: Vec<u8>,   // firma post-cuántica
    },
    /// Solicitud de contexto (con pago)
    ContextRequest {
        context_id: String,
        buyer_id: String,
        bid_price: u64,
        dilithium_sig: Vec<u8>,
    },
    /// Entrega cifrada del contexto
    ContextDelivery {
        context_id: String,
        seller_id: String,
        kyber_encrypted: Vec<u8>,  // cifrado con Kyber-1024
        dilithium_sig: Vec<u8>,
    },
    /// Voto de reputación cruzada
    ReputationVote {
        target_id: String,
        score: f32,                // -1.0 a +1.0
        context_id: Option<String>,
        dilithium_sig: Vec<u8>,
    },
    /// Heartbeat + metadata
    Heartbeat {
        node_id: String,
        version: String,
        trust_score: f32,
        telemetry: TelemetrySummary,
        dilithium_sig: Vec<u8>,
    },
}
```

---

## 5. Gobernanza

### Modelo Híbrido (lo mejor de 2026)

```
Gobernanza de Parámetros:

┌────────────────── Voto Ponderado ──────────────────┐
│  1 token = 1 voto (pero con tope anti-whale)       │
│  Voto máximo por wallet: 1% del supply circulante  │
│  Quórum mínimo: 10% del supply activo              │
└────────────────────────────────────────────────────┘

Parámetros gobernables:
├── Splits de recompensa (40/40/20 por defecto)
├── Precios de referencia de contextos
├── Multiplicadores de rareza
├── Pre-trusted peers (la red elige a sus semillas)
├── Feature flags activables
└── Thresholds de seguridad

Período de voto: 7 días
Timer de ejecución: 48h post-aprobación
```

**Pero en la práctica (Fase 1-2):** BELA como administrador hasta que la red tenga >20 nodos activos. Luego se migra a voto ponderado progresivamente.

---

## 6. Resumen de Dependencias Técnicas

```toml
[dependencies]
# Post-Quantum Cryptography
oqs = { version = "0.11", optional = true, features = ["kem", "sig", "serde"] }
#   → ML-KEM (Kyber), ML-DSA (Dilithium), Falcon

# TPM Hardware Wallet (Windows)
tpm-rs = { version = "0.1", optional = true }

# P2P Mesh (Fase 2+)
libp2p = { version = "0.56", optional = true, default-features = false, features = [
    "noise", "kad", "gossipsub", "relay", "dcutr", "identify",
    "tcp", "tokio", "serde", "quic"
] }

# EigenTrust Reputation (implementación propia o librería)
# eigentrust = { git = "https://github.com/Karma3Labs/rs-eigentrust-snaps", optional = true }

[features]
default = []
data-commons = ["oqs"]                    # Solo wallet + collector
mesh = ["libp2p"]                         # P2P networking
post-quantum = ["oqs", "tpm-rs"]          # Wallet HW + PQ crypto
full = ["data-commons", "mesh", "post-quantum"]
```

---

## 7. Comparación con Proyectos Existentes

| Aspecto | Ocean Protocol | Streamr | GARA-G | Synapse | Xavier Data Commons |
|---------|---------------|---------|--------|---------|-------------------|
| **Datos** | Datasets (estáticos) | Streams (tiempo real) | Telemetría vehicular | Neural data | Telemetría técnica de Xaviers |
| **Incentivo** | Staking + Data Farming | Sponsorships | Proof of Telemetry | Karma weights | Contribución directa + reputation |
| **Token** | OCEAN | DATA | GARA | $KIND | Por definir ($XAV) |
| **P2P** | No | Sí (libp2p) | No | Sí (libp2p) | Fase 1 HTTP, Fase 2 libp2p |
| **Post-quántico** | No | Planeado (2025) | No | No | **Sí, desde diseño** |
| **Reputación** | No | No | Coverage Analyzer | Karma | EigenTrust adaptado |
| **Privacidad** | Compute-to-Data | Streaming | Anónimo | Soulbound | Cifrado + datos técnicos |

---

## 8. Conclusión y Recomendaciones

### Stack final recomendado (Fase 1, implementable ahora)

```toml
data-commons = ["oqs"]  # Solo post-quantum wallet + collector
```

1. **Wallet:** `oqs` crate (Kyber-1024 + Dilithium-5). Fallback software sin TPM.
2. **Reputación:** EigenTrust adaptado — implementación propia simplificada (no necesitas la librería externa, el algoritmo cabe en ~200 líneas de Rust).
3. **Mercado:** Precios dinámicos basados en rareza + trust score. Sin blockchain interno — ledgers locales sincronizados vía mesh.
4. **P2P:** Mantener HTTP existente (Antigravity Phase 1). libp2p overlay solo cuando >20 nodos.
5. **Tokenomics:** BME model, 80% burn rate objetivo. Sin pre-mining.
6. **Gobernanza:** BELA como administrador hasta >20 nodos, luego voto ponderado.

### Lo que NO recomiendo hacer ahora

- ❌ Crear un token en Solana/Ethereum desde el día 1 (gas fees, overhead)
- ❌ Implementar libp2p desde ya (el HTTP existente funciona para <20 nodos)
- ❌ Smart contracts complejos para el marketplace (SQLite local + sincronización P2P basta)
- ❌ Zero-knowledge proofs (ZK) para privacidad (los datos ya son técnicos, no personales)
