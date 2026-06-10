# Xavier Data Commons — User Stories & Features

> **Token:** $XAV
> **Filosofía:** 1 wallet = 1 voto. 100% democrático, 100% anónimo.
> **Wallet:** Post-cuántica (Kyber + Dilithium). TPM si disponible, software puro si no.
> **Bridge GARA:** No por ahora.

---

## 👤 Personas

### 1. Usuario Técnico (el que comparte datos)
- Tiene uno o más nodos Xavier corriendo
- Delega datos técnicos (logs, errores, telemetría, métricas de workspace)
- Quiere ser recompensado por contribuir a la mejora de la red
- Puede tener múltiples nodos bajo una misma wallet

### 2. Consumidor de Contexto (el que compra)
- Es otro nodo Xavier que necesita datos para diagnosticar un error
- Paga $XAV por acceder a contextos técnicos de alta calidad
- Puede ser el mismo usuario técnico en otro momento

### 3. Validador (el que atestigua)
- Verifica que un contexto compartido es legítimo y útil
- Gana $XAV por su labor de validación cruzada
- Su EigenTrust score sube si acierta, baja si valida basura

### 4. Gobernante (todos con wallet)
- Cualquier wallet con $XAV token vota en decisiones de red
- 1 wallet = 1 voto. Sin importar saldo.
- Votación anónima (cifrada con Kyber)

---

## 🎯 Features & User Stories

### EPIC 1: Post-Quantum Wallet ($XAV)

**Feature 1.1 — Wallet Creation**
```
Feature: Creación de wallet post-cuántica
  Como: Usuario técnico
  Quiero: Crear una wallet $XAV con criptografía post-cuántica
  Para: Tener una identidad segura contra ataques cuánticos futuros

  Acceptance Criteria:
  - [ ] Generar keypair ML-KEM (Kyber-1024) para cifrado
  - [ ] Generar keypair ML-DSA (Dilithium-5) para firmas
  - [ ] Generar keypair Ed25519 para identidad mesh (compatibilidad existente)
  - [ ] Derivar seed phrase BIP-39 en español de 12/24 palabras
  - [ ] Derivat wallet address desde hash público (xv1_ + hash del public key ML-DSA)
  - [ ] Almacenar seed cifrada con contraseña del usuario (Argon2id + AES-256-GCM)
  - [ ] Exportar seed phrase como QR code
  - [ ] Importar wallet existente desde seed phrase
```

**Feature 1.2 — TPM Hardware Wallet**
```
Feature: Wallet con TPM 2.0 (cuando disponible)
  Como: Usuario técnico con hardware compatible
  Quiero: Almacenar la seed de mi wallet dentro del TPM 2.0
  Para: Seguridad hardware-level, la seed nunca sale del chip

  Acceptance Criteria:
  - [ ] Detectar TPM 2.0 en Windows (tpm-rs o TBS API)
  - [ ] Crear Storage Root Key (SRK) RSA-2048 en TPM
  - [ ] Cifrar seed con SRK (nunca desencriptarla fuera del TPM)
  - [ ] Firmar transacciones dentro del TPM
  - [ ] Atestación PCR quote: probar que el TPM es legítimo
  - [ ] Fallback automático a software si no hay TPM
  - [ ] Mostrar en `xavier wallet status` qué tipo de wallet está activa
```

**Feature 1.3 — Múltiples Nodos por Wallet**
```
Feature: Vincular múltiples nodos a una wallet
  Como: Usuario técnico
  Quiero: Registrar varios nodos Xavier bajo mi misma wallet
  Para: Que todos mis nodos compartan la reputación y recompensas

  Acceptance Criteria:
  - [ ] Cada nodo tiene NodeID único (Ed25519, ya existe)
  - [ ] Registrar nodo: firmar (NodeID + WalletAddress) con Dilithium-5
  - [ ] Wallet puede tener N nodos registrados
  - [ ] Recompensas acumuladas por nodo → wallet central
  - [ ] Revocar nodo: el wallet firma la revocación
  - [ ] Visualización: `xavier wallet nodes` lista todos los nodos vinculados
```

---

### EPIC 2: Data Collector (Dogfood — primer contribuidor)

**Feature 2.1 — Telemetría Técnica Automática**
```
Feature: Colector automático de datos técnicos
  Como: Módulo Xavier
  Quiero: Recolectar datos de telemetría, errores, logs y métricas
  Para: Que el usuario pueda decidir compartirlos y ser recompensado

  Datos a recolectar (SOLO técnicos):
  - [ ] Errores de compilación/ejecución (stack traces, exit codes)
  - [ ] Logs de módulos (info, warn, error con niveles)
  - [ ] Métricas de workspace (archivos, tamaño, lenguajes)
  - [ ] Benchmarks de rendimiento (CPU, RAM, disco)
  - [ ] Versiones de dependencias (crates, paquetes)
  - [ ] Tiempos de ejecución de tareas comunes
  - [ ] Errores de red (timeouts, conexiones fallidas)
  - [ ] Anomalías de comportamiento

  NO recolectar NUNCA:
  - ❌ Nombres de archivos/contenido de código del usuario
  - ❌ Variables de entorno con secrets
  - ❌ Datos personales (nombre, email, IP real)
  - ❌ Historial de comandos del usuario
  - ❌ Navegación web
```

**Feature 2.2 — Consentimiento del Usuario**
```
Feature: Consentimiento granular del usuario
  Como: Usuario técnico
  Quiero: Decidir qué datos compartir y cuándo
  Para: Tener control total sobre mi privacidad

  Acceptance Criteria:
  - [ ] Prompt modal antes de compartir datos por primera vez
  - [ ] Tres niveles de consentimiento:
       a) "Compartir todo automáticamente" (default-off)
       b) "Preguntar cada vez" (default)
       c) "No compartir nunca" (modo offline)
  - [ ] El usuario puede ver exactamente qué datos se van a compartir
  - [ ] El usuario puede revocar el consentimiento en cualquier momento
  - [ ] Datos no compartidos → no se eliminan, quedan en cola local
  - [ ] Consentimiento configurable por tipo de dato
  - [ ] `xavier data-commons consent` — CLI para ver/editar consentimiento
```

**Feature 2.3 — Anonimización**
```
Feature: Limpieza y anonimización de datos
  Como: Sistema Xavier Data Commons
  Quiero: Sanitizar datos antes de ofrecerlos a la red
  Para: Garantizar que no salga información del usuario

  Acceptance Criteria:
  - [ ] Reemplazar NodeID interno por pseudónimo rotativo
  - [ ] Quitar IPs, hostnames, paths absolutos
  - [ ] Hash de paths de workspace (SHA-256)
  - [ ] Quitar timestamps exactos → solo rangos (última hora, hoy, esta semana)
  - [ ] Verificar que no hay secrets en logs (regex patterns)
  - [ ] El usuario puede previsualizar los datos anonimizados
```

---

### EPIC 3: Funnel de Recompensas

**Feature 3.1 — MINTER Automático**
```
Feature: Minteo automático de $XAV
  Como: Sistema Xavier Data Commons
  Quiero: Emitir tokens $XAV automáticamente cuando hay contribución válida
  Para: Recompensar a los nodos que mejoran la red

  Acceptance Criteria:
  - [ ] Evento: nodo comparte contexto → MINTER se activa
  - [ ] Calcular recompensa según:
       a) Rareza del dato (cuántos nodos reportaron lo mismo)
       b) Trust score del nodo (EigenTrust)
       c) Tipo de dato (error crítico > benchmark > telemetría normal)
       d) Utilidad comprobada (el contexto fue comprado por otros)
  - [ ] Split de recompensa:
       a) 40% — nodo que compartió
       b) 40% — usuario (wallet)
       c) 20% — red (reserva para recompensas futuras, gobernanza)
  - [ ] No hay pre-mining — solo se mintea cuando hay contribución
  - [ ] Supply no fijo (inflación controlada por consumo)
  - [ ] Los tokens minteados van a la wallet del usuario
```

**Feature 3.2 — Anti-Manipulación**
```
Feature: Prevención de abuso del sistema de recompensas
  Como: Sistema Xavier Data Commons
  Quiero: Detectar y prevenir manipulación del minteo
  Para: El token $XAV tenga valor real, no inflado artificialmente

  Patrones de abuso a prevenir:

  1. Sybil Attack: Crear 1000 wallets para mintear desde todos
     → Mitigación: Se requiere al menos 1 nodo activo por wallet.
       Si no hay nodo corriendo ≥24h, la wallet no puede mintear.
       EigenTrust baja drásticamente a wallets sin historial.

  2. Self-Dealing: Compartir contexto basura y comprarlo con otra wallet
     → Mitigación: Si un wallet compra contexto de sí mismo (mismo seed),
       la transacción se rechaza. Validación cruzada: >1 validador necesario
       para contextos con trust_score < 0.5.

  3. Flood de datos basura: Enviar miles de contextos sin valor
     → Mitigación: Rate limiting por nodo (max 10 contextos/día si
       trust_score < 0.3). Combinar contextos similares en uno solo.

  4. Collusion: Varios nodos acuerdan validarse mutuamente basura
     → Mitigación: EigenTrust detecta subgrafos densos (co-validación
       recíproca). Si A y B siempre se validan mutuamente sin variación,
       ambos bajan de score.

  5. Replay Attack: Compartir el mismo contexto una y otra vez
     → Mitigación: Hash del contexto como ID único. Si ya existe,
       se rechaza. Si es similar (>80% overlap), se considera duplicado.

  6. Cónclave de validación falsa: Muchas wallets en collusion
     → Mitigación: El sistema tiene pre-trusted seeds (Xavier Core).
       Los seeds pueden revocar la confianza de wallets que participan
       en collusion probada. Voting bridge para expulsión.

  ⚠️ Esta feature es CRÍTICA. No se despliega sin pruebas extensivas.
```

**Feature 3.3 — Burn al Consumir**
```
Feature: Quema de $XAV al comprar contextos
  Como: Sistema Xavier Data Commons
  Quiero: Quemar tokens cuando un nodo compra un contexto
  Para: Crear presión deflacionaria y evitar inflación descontrolada

  Acceptance Criteria:
  - [ ] 80% del precio pagado se quema (enviar a address null)
  - [ ] 20% va a rewards pool para validadores
  - [ ] Cada transacción de compra registra el burn
  - [ ] Supply circulante es visible: `xavier data-commons supply`
  - [ ] Total supply minteado vs quemado: dashboard
```

---

### EPIC 4: Sistema de Reputación

**Feature 4.1 — EigenTrust Scoring**
```
Feature: Cálculo de reputación EigenTrust
  Como: Nodo Xavier
  Quiero: Calcular trust scores de peers basado en interacciones
  Para: Saber qué nodos son confiables y cuáles no

  Acceptance Criteria:
  - [ ] Implementar EigenTrust algorithm core (~200 líneas)
  - [ ] Señales de entrada: +1 (útil), -1 (basura), 0 (neutral)
  - [ ] Pre-trusted peers: seed nodes de Xavier Core
  - [ ] Power iteration hasta convergencia (||diff|| < 0.001)
  - [ ] Teletransporte: 15% probabilidad de resetear a pre-trusted
  - [ ] Distrust adjustment: un nodo no puede desacreditar más peers
       que su propio trust score
  - [ ] Output: trust score -1.0 a +1.0 por wallet
  - [ ] Periodicidad: cada 24h o después de 100 nuevas interacciones
  - [ ] Sincronización vía gossip (no requiere blockchain)
```

**Feature 4.2 — Contribution Score**
```
Feature: Score de contribución complementario
  Como: Sistema Xavier Data Commons
  Quiero: Medir la contribución real de cada nodo
  Para: Tener una reputación híbrida no manipulable solo con votos

  Métricas de contribución:
  - [ ] # de contextos compartidos (únicos, no duplicados)
  - [ ] % de contextos que fueron comprados por otros (utilidad)
  - [ ] Uptime del nodo (min 99% para bonus)
  - [ ] Versión actualizada (correr última versión = +)
  - [ ] Validaciones realizadas (con acierto)

  Fórmula:
  Reputation = 0.7 × EigenTrust + 0.3 × ContributionScore

  Sí, EigenTrust pesa más porque mide confianza social.
  ContributionScore evita que nodos nuevos queden excluídos.
```

---

### EPIC 5: Marketplace de Contextos

**Feature 5.1 — Publicar Contexto**
```
Feature: Publicar contexto técnico a la red
  Como: Nodo Xavier con consentimiento del usuario
  Quiero: Publicar un contexto técnico (error, log, telemetría) a la red
  Para: Que otros nodos puedan comprarlo y yo gane $XAV

  Acceptance Criteria:
  - [ ] Seleccionar qué datos compartir (modal de consentimiento)
  - [ ] Anonimizar datos automáticamente
  - [ ] Calcular hash del contenido (SHA-256) — ID único
  - [ ] Cifrar contenido con Kyber-1024 (solo quien paga puede ver)
  - [ ] Firmar oferta con Dilithium-5
  - [ ] Broadcast a peers via gossip (o HTTP push)
  - [ ] Incluir metadata pública: tipo, rareza, trust_score del vendedor
  - [ ] Si el contexto ya existe (mismo hash), rechazar (no duplicados)
```

**Feature 5.2 — Buscar y Comprar Contexto**
```
Feature: Buscar y comprar contexto técnico
  Como: Nodo Xavier
  Quiero: Buscar contextos técnicos relevantes y comprarlos
  Para: Obtener información para diagnosticar y resolver errores

  Acceptance Criteria:
  - [ ] Buscar por: tipo de error, módulo, palabras clave
  - [ ] Ver metadata de contexto disponible (sin revelar contenido)
  - [ ] Seleccionar y pagar: precio dinámico basado en rareza + trust
  - [ ] Pago en $XAV → quema 80%, rewards 20%
  - [ ] Recibir contenido descifrado (el vendedor lo cifró con Kyber
       usando la key pública del comprador)
  - [ ] Calificar utilidad: +1 (útil), -1 (basura)
  - [ ] La calificación actualiza EigenTrust inmediatamente
```

**Feature 5.3 — Precios Dinámicos**
```
Feature: Precios automatizados por oferta y demanda
  Como: Sistema Xavier Data Commons
  Quiero: Ajustar precios de contextos automáticamente
  Para: Reflejar valor real de mercado sin intervención humana

  Fórmula:
  ```
  Precio = PrecioReferencia × (1 / Rareza) × TrustScore × MultiplicadorTipo

  Donde:
  - PrecioReferencia = 5 $XAV (ajustable por gobernanza)
  - Rareza = #nodosQueReportaron / #totalNodos
    - Si 1 de 100 nodos lo reportó → rareza = 0.01 → precio × 100
    - Si 50 de 100 → rareza = 0.5 → precio × 2
  - TrustScore = EigenTrust del vendedor (0.1 - 1.0)
  - MultiplicadorTipo:
    - Error crítico (crash, data loss) → x3.0
    - Error funcional (feature no funciona) → x2.0
    - Benchmark/métrica → x1.5
    - Log normal → x1.0
    - Telemetría básica → x0.5
  ```

  Precio mínimo: 1 $XAV (evita spam de micro-transacciones)
  Precio máximo: 10,000 $XAV (evita exploits)
```

---

### EPIC 6: Gobernanza Democrática

**Feature 6.1 — Sistema de Votación**
```
Feature: Votación descentralizada 100% democrática
  Como: Cualquier wallet $XAV
  Quiero: Votar en decisiones de la red
  Para: Participar en la gobernanza del ecosistema

  Reglas de Votación:
  - [ ] 1 wallet = 1 voto. SIN importar saldo de $XAV
  - [ ] Voto ANÓNIMO: cifrado con Kyber, revelado solo al contar
  - [ ] Quórum mínimo: 10% de wallets activas (que hayan votado en ≥1 mes)
  - [ ] Período de voto: 7 días
  - [ ] Mayoría simple gana (>50% de votos emitidos)
  - [ ] Timer de ejecución: 48h post-aprobación

  No hay delegación — si no votas, no votas. Simple.
  Esto evita que grandes wallets acumulen poder delegado.
```

**Feature 6.2 — Parámetros Gobernables**
```
Feature: Parámetros modificables por voto
  Como: Gobernanza de Xavier Data Commons
  Quiero: Permitir la modificación de parámetros del sistema por voto
  Para: Que la red evolucione orgánicamente

  Parámetros gobernables (iniciales):
  - [ ] PrecioReferencia (default 5 $XAV)
  - [ ] MultiplicadoresPorTipo (default abajo)
  - [ ] Split de recompensas (default 40/40/20)
  - [ ] Rate limits por nodo (default 10/día para trust < 0.3)
  - [ ] Burn rate (default 80%)
  - [ ] Período de voto (default 7 días)
  - [ ] Quórum mínimo (default 10%)
  - [ ] Precio mínimo/máximo (default 1 / 10,000 $XAV)
  - [ ] Candidatos a pre-trusted seeds
  - [ ] Expulsión de wallets por collusion probada (requiere 66% de votos)
```

**Feature 6.3 — Propuestas**
```
Feature: Sistema de propuestas (XIP — Xavier Improvement Proposal)
  Como: Cualquier wallet $XAV
  Quiero: Crear una propuesta de mejora para la red
  Para: Proponer cambios que beneficien a todos

  Acceptance Criteria:
  - [ ] Cualquier wallet puede crear una propuesta
  - [ ] Propuesta debe tener: título, descripción, parámetros a cambiar
  - [ ] Período de discusión: 3 días (comentarios en mesh)
  - [ ] Pasar a votación si tiene ≥5 apoyos de wallets distintas
  - [ ] Si pasa la votación → ejecución automática (timer 48h)
  - [ ] Historial de propuestas: `xavier gov proposals`
```

---

### EPIC 7: Observabilidad (Dogfood primero)

**Feature 7.1 — Dashboard de Red**
```
Feature: Dashboard de estado de Data Commons
  Como: Usuario técnico
  Quiero: Ver métricas de la red Data Commons
  Para: Entender el estado del ecosistema

  Métricas a mostrar:
  - [ ] # wallets activas
  - [ ] # nodos activos
  - [ ] # contextos compartidos (total / últimos 24h)
  - [ ] # contextos comprados (total / últimos 24h)
  - [ ] Total $XAV minteado
  - [ ] Total $XAV quemado
  - [ ] Supply circulante
  - [ ] Precio promedio de contexto
  - [ ] Mi trust score
  - [ ] Mi contribution score
  - [ ] Mis recompensas acumuladas
```

---

## 📋 Priorización para Features Stories

```
Fase 0 — AHORA (investigación + diseño)
├── 📄 Este documento (User Stories & Features)
├── 📄 Documento de Arquitectura (ya existe)
├── 📄 Especificaciones técnicas (por feature)
└── ⏳ Aprobación de BELA antes de implementar

Fase 1 — Core Wallet (posterior)
├── Feature 1.1: Wallet Creation
├── Feature 1.3: Multi-nodo por wallet
└── Feature 1.2: TPM support (si disponible)

Fase 2 — Collector + Funnel
├── Feature 2.1: Colector automático
├── Feature 2.2: Consentimiento del usuario
├── Feature 2.3: Anonimización
└── Feature 3.1: MINTER automático

Fase 3 — Reputación
├── Feature 4.1: EigenTrust scoring
└── Feature 4.2: Contribution score

Fase 4 — Marketplace
├── Feature 5.1: Publicar contexto
├── Feature 5.2: Buscar y comprar
└── Feature 5.3: Precios dinámicos

Fase 5 — Gobernanza
├── Feature 6.1: Sistema de votación
├── Feature 6.2: Parámetros gobernables
└── Feature 6.3: Propuestas (XIP)

Fase 6 — Dashboard
└── Feature 7.1: Dashboard de red
```

---

## 🧠 Anti-Manipulación (Research Deep Dive)

### Patrones de Abuso Identificados y Mitigaciones

#### 1. Sybil Attack
**Qué es:** Crear cientos de wallets para mintear $XAV desde todas.

**Mitigación multi-capa:**
- **Layer 1 — Proof of Liveliness:** La wallet debe tener al menos 1 nodo activo (≥24h uptime) para poder mintear. El nodo firma heartbeats periódicos.
- **Layer 2 — EigenTrust threshold:** Wallets sin historial de validaciones tienen trust_score ≈ 0. Los contextos de wallets con trust < 0.1 pagan solo 10% de la recompensa base.
- **Layer 3 — Rate limiting:** Max 10 contextos/día para wallets con trust < 0.3.

#### 2. Collusion en Validaciones
**Qué es:** Varias wallets se validan mutuamente basura para subir trust_score.

**Mitigación:**
- EigenTrust detecta subgrafos densos de co-validación recíproca.
- Si A valida a B y B valida a A repetidamente: se considera colusión.
- La confianza se propaga a través de cadenas, no pares aislados.

#### 3. Contexto Falso + Compra Propia
**Qué es:** Usuario A comparte contexto falso, usuario B (su otra wallet) lo compra → A gana tokens, B gana reputación.

**Mitigación:**
- Si dos wallets comparten el mismo seed phrase, se detecta. Transacción rechazada.
- Validación cruzada obligatoria: contextos con trust < 0.5 requieren 3 validadores distintos.
- Los validadores NO pueden ser wallets del mismo seed.

#### 4. Ataque de Replay
**Qué es:** Compartir el mismo contexto muchas veces.

**Mitigación:**
- Hash SHA-256 del contenido = ID único.
- Si el hash ya existe en la red, se rechaza.
- Si el contenido es >80% similar a uno existente, se rechaza como duplicado.

#### 5. Ataque de Inflación por Micro-contextos
**Qué es:** Compartir miles de contextos diminutos (cada línea de log como contexto separado) para maximizar recompensa.

**Mitigación:**
- Precio mínimo de 1 $XAV por contexto (no se puede dividir en micro-pagos).
- Rate limiting dinámico basado en trust_score.
- Contextos similares se agrupan automáticamente.

#### 6. Ballot Stuffing (Votación Fraudulenta)
**Qué es:** Crear miles de wallets para influir en votaciones.

**Mitigación:**
- Quórum de 10% de wallets ACTIVAS (que hayan votado en ≥1 mes).
- Votación anónima no requiere desanonimizar, pero sí verificar que el wallet haya existido por ≥7 días para votar.
- Voto máximo: 1 wallet = 1 voto. Sin importar saldo — esto LIMITA el incentivo de crear wallets para votar.

### Investigación de Referencia
- **EigenTrust (Stanford 2003):** Base teórica de trust scoring
- **Distributed Hash Tables:** Kademlia para discovery sin SPOF
- **Sybil-resistant DHTs:** Defeating Sybil Attacks with DHT Rerouting
- **Ocean Protocol Anti-Gaming:** Rate limiting + reputation-weighted rewards
- **Gitcoin Passport:** Sybil resistance for DAO voting (unique humanity, pero acá es unique wallet)

---

## 🔐 Resumen de Criptografía

| Propósito | Algoritmo | Crate | Estándar |
|-----------|-----------|-------|----------|
| Cifrado de contexto | ML-KEM-1024 (Kyber-1024) | `oqs` | NIST FIPS 203 |
| Firmas de transacciones | ML-DSA-87 (Dilithium-5) | `oqs` | NIST FIPS 204 |
| Identidad mesh (existente) | Ed25519 | `ed25519-dalek` | RFC 8032 |
| Wallet seed | BIP-39 (español, 24 palabras) | `bip39` | BIP 39 |
| Hashing de contenido | SHA-256 | `sha2` | FIPS 180-4 |
| Cifrado simétrico local | AES-256-GCM | `aes-gcm` | NIST SP 800-38D |
| Derivation key (seed→wallet) | Argon2id | `argon2` | RFC 9106 |
| TPM (cuando disponible) | RSA-2048 + SRK | `tpm-rs` | TCG TPM 2.0 |
