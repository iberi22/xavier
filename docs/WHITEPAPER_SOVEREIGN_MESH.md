# Xavier Sovereign Mesh — Whitepaper Económico

**Versión:** 0.11.0-22-06-2026
**Autor:** Xavier Core Team + La.Santacruz Labs
**Licencia:** MIT + Mesh License

---

## Resumen Ejecutivo

El Xavier Sovereign Mesh es un ecosistema descentralizado donde nodos (agentes, humanos,
organizaciones) intercambian valor económico — cómputo, datos, ancho de banda, almacenamiento
y servicios de IA — mediante un token de gobernanza y utilidad: **XAV**.

Este whitepaper describe el modelo matemático de economía estable, el sistema de recompensas
con cláusulas de permanencia (vesting progresivo), el mecanismo de inversión y retorno, y
la estructura de gobernanza bicameral.

---

## 1. Principios Económicos Fundamentales

### 1.1 Ley de Gresham Aplicada
> "El dinero malo expulsa al bueno" — en un mesh abierto, sin mecanismos de calidad,
> los nodos de baja calidad desplazan a los de alta. El sistema de reputación y staking
> previene esto.

### 1.2 Ecuación de Intercambio (Fisher Modificada)
```
M * V = P * Q
```
Donde:
- **M** = Oferta monetaria de XAV en circulación
- **V** = Velocidad de circulación del token
- **P** = Precio promedio de los servicios del mesh
- **Q** = Cantidad de servicios/computo intercambiados

### 1.3 Ley de Metcalfe Aplicada al Mesh
```
V_red = k * n²
V_económico = α * V_red * log₂(n)
```
Donde:
- **V_red** = Valor de la red
- **n** = Número de nodos activos
- **α** = Factor de monetización (0 < α ≤ 1)
- **V_económico** = Valor económico real del mesh

---

## 2. Tokenomics: XAV Token

### 2.1 Suministro
```
Suministro Máximo (S_max) = 100,000,000 XAV
Suministro Inicial (S_0) = 10,000,000 XAV
Tasa de Inflación Anual (i) = 2% (decreciente 0.25% cada año hasta 0.5%)
```
La inflación se destina únicamente a recompensas de nodos activos.

### 2.2 Distribución Inicial (S_0 = 10M XAV)

| Asignación | % | Tokens | Vesting |
|-----------|---|--------|---------|
| Community Reserve | 40% | 4,000,000 | Bonding curve (demanda) |
| Core Team | 15% | 1,500,000 | 12m cliff + 24m linear |
| Early Backers | 10% | 1,000,000 | 6m cliff + 18m linear |
| Liquidity Pool | 10% | 1,000,000 | 100% unlocked |
| Mesh Treasury | 15% | 1,500,000 | Governance-controlled |
| Ecosystem Grants | 10% | 1,000,000 | 3m cliff + 36m linear |

### 2.3 Bonding Curve Primaria

El precio base del token se rige por una **bonding curve exponencial suavizada**:

```
P(S) = P_0 * e^(k * S / S_max)
```

Donde:
- **P(S)** = Precio en USD cuando el suministro circulante es S
- **P_0** = Precio inicial (seed): $0.10
- **k** = Factor de curvatura: 4.0
- **S** = Suministro circulante actual
- **S_max** = Suministro máximo: 100,000,000

**Precio máximo teórico** (cuando S = S_max):
```
P_max = 0.10 * e^(4.0) ≈ $5.46
```

### 2.4 Curva de Recompra (Buyback Curve)

Para estabilidad, el treasury opera una curva de recompra:
```
P_buy(S) = P(S) * 0.90
```

El treasury siempre compra a 90% del precio de curva, creando un piso artificial.

---

## 3. Sistema de Inversión con Cláusulas de Permanencia (Stake & Lock)

Este es el mecanismo principal que pide el diseño: **inversión con desbloqueo progresivo**.

### 3.1 Niveles de Lock

| Nivel | Monto Mínimo | Lock Total | Desbloqueo Parcial (50%) | Desbloqueo Total (100%) |
|-------|-------------|------------|-------------------------|------------------------|
| Bronze | $100 | 2 meses | Mes 2 | Mes 4 |
| Silver | $500 | 4 meses | Mes 2 | Mes 6 |
| Gold | $1,000 | 6 meses | Mes 3 | Mes 9 |
| Platinum | $5,000 | 9 meses | Mes 4 | Mes 12 |
| Diamond | $10,000 | 12 meses | Mes 6 | Mes 18 |
| Sovereign | $50,000+ | 18 meses | Mes 8 | Mes 24 |

### 3.2 Modelo Matemático de Liberación

Para un inversor que deposita **T** tokens en el nivel **N**:
```
Tiempo de lock total: L_N (en meses)
Tiempo para desbloqueo parcial: L_N / 2
Porcentaje liberado en tiempo t:

f(t) = {
    0,                              si t < L_N * 0.25
    0.30,                           si L_N * 0.25 ≤ t < L_N / 2
    0.30 + 0.70 * (t - L_N/2) / L_N,  si L_N/2 ≤ t ≤ L_N
    1.0,                            si t > L_N
}
```

**Ejemplo Gold ($1,000, L=6 meses):**
- Mes 0-1.5: 0% liberado
- Mes 1.5: 30% liberado (desbloqueo inicial)
- Mes 3: 50% liberado
- Mes 4.5: ~73% liberado
- Mes 6: 100% liberado

### 3.3 Penalización por Retiro Anticipado

```
Penalidad(t) = {
    30%,        si t < L_N * 0.25
    15%,        si L_N * 0.25 ≤ t < L_N / 2
    5%,         si L_N / 2 ≤ t < L_N * 0.75
    0%,         si t ≥ L_N * 0.75
}
```

Los tokens penalizados van al treasury del mesh.

### 3.4 Recompensas por Staking (APY)

```
APY_anual = APY_base * multiplicador_nivel
```

| Nivel | Multiplicador | APY Efectivo |
|-------|--------------|-------------|
| Base (sin lock) | 1.0x | 5% |
| Bronze | 1.5x | 7.5% |
| Silver | 2.0x | 10% |
| Gold | 2.5x | 12.5% |
| Platinum | 3.5x | 17.5% |
| Diamond | 5.0x | 25% |
| Sovereign | 8.0x | 40% |

### 3.5 Fórmula de Recompensa Diaria

```
R_diaria = (T_depositado * APY_efectivo) / 365
```

Donde:
- **R_diaria** = Tokens XAV liberados como recompensa cada día
- **T_depositado** = Cantidad de tokens en stake
- Recompensas se distribuyen diariamente y **no tienen lock** (se pueden retirar inmediatamente)

---

## 4. Reputación y Gobernanza (Soulbound Score)

### 4.1 Score de Reputación Compuesto

```
R_score = (w_1 * S_largo_plazo) + (w_2 * S_contribucion) + (w_3 * S_antiguedad) + (w_4 * S_staking)
```

Donde:
- **w_1** = 0.35 (peso de locks a largo plazo)
- **w_2** = 0.30 (peso de contribuciones al mesh)
- **w_3** = 0.20 (peso de antigüedad en la red)
- **w_4** = 0.15 (peso de staking actual)

### 4.2 Peso de Voto

```
Peso_voto = T_stakeados * (1 + log₂(1 + R_score))
```

Esto asegura que un nodo con alta reputación tenga más peso que uno con solo tokens,
fomentando participación activa.

---

## 5. Sistema Bicameral de Gobernanza

### 5.1 Cámara Baja — Asamblea de Nodos
- Cualquier nodo con ≥ 100 XAV en stake puede votar
- Vota sobre: asignación de grants, parámetros de red, tarifas
- **Quórum mínimo:** 15% del poder de voto total
- **Mayoría simple:** >50% para aprobar

### 5.2 Cámara Alta — Consejo de Guardianes
- Elegida por la Cámara Baja cada 6 meses
- 7 miembros con mandato escalonado
- Veto sobre cambios constitucionales y parámetros críticos
- **Requiere:** 60% de supermayoría para revocar veto

### 5.3 Tiempo de Espera (Timelock)
```
Timelock = {
    propuestas regulares:      48 horas
    cambios de parámetros:     72 horas
    cambios constitucionales:  7 días
}
```

---

## 6. Economía del Mesh: Flujo de Valor

### 6.1 Servicios y Tarifas

| Servicio | Tarifa Base | Descuento con Stake |
|---------|------------|-------------------|
| Almacenamiento (GB/mes) | 0.01 XAV | -50% con ≥ 1000 XAV |
| Cómputo (CPU-hora) | 0.05 XAV | -30% con ≥ 500 XAV |
| Ancho de banda (GB) | 0.001 XAV | -20% con ≥ 100 XAV |
| Embeddings (1000 calls) | 0.10 XAV | -40% con ≥ 500 XAV |
| Inferencia IA (hora) | 0.50 XAV | -25% con ≥ 2000 XAV |

### 6.2 Protocol-Owned Liquidity (POL)

El treasury mantiene liquidez propia en pools DEX:
```
POL_objetivo = 20% del market cap
POL_mínimo = 10% del market cap
```

Si POL cae debajo del mínimo, las tarifas de red se incrementan un 10% temporalmente.

### 6.3 Tasa de Quema (Burn Rate)

```
Burn_rate = 0.05 * Tarifas_totales
```

El 5% de todas las tarifas se queman permanentemente, generando presión deflacionaria.

---

## 7. Estabilidad del Sistema (Mecanismos Anti-Volatilidad)

### 7.1 Reserve Ratio (RR)

```
RR = Reservas_del_treasury / Market_cap_circulante
```

- **Objetivo:** RR ≥ 0.25 (25%)
- Si RR < 0.25: El bonding curve se aplana (k se reduce) y las recompensas de staking bajan 50%
- Si RR > 0.50: El bonding curve se empina (k aumenta) y parte del exceso se quema

### 7.2 Circuit Breakers (Tres Niveles)

| Nivel | Condición | Acción |
|-------|-----------|--------|
| 🟡 Amarillo | -15% en 24h | Pausar retiros de staking por 12h + treasury compra |
| 🟠 Naranja | -25% en 24h | Congelar bonding curve + emergencia governance |
| 🔴 Rojo | -40% en 24h | Emergency DAO shutdown + liquidation protection |

### 7.3 Dynamic Emissions

Las recompensas por bloque se ajustan según actividad de la red:
```
Emission_rate = E_base * (1 + 0.5 * (U_red - U_target) / U_target)
```

Donde **U_red** es la utilización actual de la red y **U_target** es la utilización objetivo (70%).

---

## 8. Implementación Técnica

### 8.1 Smart Contracts (EVM)

El sistema se despliega como contratos EVM usando `alloy`:

```
contracts/
├── XAVToken.sol          # ERC20 con mint/burn controlado
├── StakingVault.sol       # Staking con niveles y locks
├── VestingSchedule.sol    # Vesting linear + cliff
├── BondingCurve.sol       # Curva precio-suministro
├── TreasuryReserve.sol    # Treasury con reserve ratio
├── Governance.sol         # Bicameral DAO
├── TimeLock.sol           # Timelock executor
└── MeshTokenomics.sol     # Orquestador global
```

### 8.2 Módulo Xavier (Rust)

El módulo `mesh::tokenomics` en Xavier implementa la lógica off-chain:

- Cálculo de bonding curves
- Scoring de reputación
- Reward distribution
- Monitoreo de reserve ratio
- Circuit breakers

### 8.3 Oracle de Precios

Para el cálculo de bonding curves on-chain, se requiere un oracle (Chainlink o similar)
que reporte el precio de XAV en USD.

---

## 9. Roadmap de Implementación

| Fase | Timeline | Features |
|------|----------|----------|
| Fase 1: Pre-Mesh | Q3 2026 | Staking vault + bonding curve + vesting |
| Fase 2: Lanzamiento | Q4 2026 | DEX listing + treasury + POL inicial |
| Fase 3: Gobernanza | Q1 2027 | Bicameral DAO + timelock + grants |
| Fase 4: Madurez | Q2 2027 | Mesh multi-nodo + oráculos + estabilidad |
| Fase 5: Escalamiento | Q3 2027 | Sovereign Mesh completo + Data Commons |

---

## 10. Riesgos y Mitigaciones

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|------------|---------|-----------|
| Flash crash | Media | Alto | Circuit breakers + POL |
| Ataque gobernanza | Baja | Crítico | Timelock + Cámara Alta |
| Dumping masivo | Media | Alto | Vesting progresivo + penalidades |
| Baja adopción | Alta | Medio | Grants + tier de recompensas |
| Bug en contratos | Baja | Crítico | Auditorías múltiples + bug bounty |

---

## Apéndice A: Fórmulas Matemáticas Completas

### A.1 Bonding Curve Completa
```
P(S) = P_0 * e^(k * (S - S_0) / S_max)  para S ≥ S_0
```

### A.2 Costo de Compra (de S_1 a S_2 tokens)
```
Costo(S_1, S_2) = ∫_{S_1}^{S_2} P(s) ds
                 = P_0 * (S_max / k) * [e^(k*S_2/S_max) - e^(k*S_1/S_max)]
```

### A.3 Retorno de Venta (de S_1 a S_2 tokens)
```
Retorno(S_1, S_2) = ∫_{S_1}^{S_2} P_buy(s) ds
                   = 0.90 * P_0 * (S_max / k) * [e^(k*S_2/S_max) - e^(k*S_1/S_max)]
```

### A.4 Valor Presente de Vesting (DCF)
```
VP = Σ_{t=1}^{L} (f(t) * T * P(S_t)) / (1 + r)^t
```
Donde:
- **VP** = Valor presente del vesting
- **f(t)** = Fracción liberada en tiempo t
- **T** = Total de tokens
- **P(S_t)** = Precio esperado en el momento del desbloqueo
- **r** = Tasa de descuento (costo de oportunidad)
- **L** = Duración total del lock en meses

### A.5 Cálculo del Reserve Ratio Dinámico
```
RR_ajustado = RR_actual * (1 + β * (RR_objetivo - RR_actual) / RR_objetivo)
```
Donde β = 0.5 es el factor de ajuste.

---

## Apéndice B: Simulaciones

### B.1 Escenario Base (12 meses)
- Precio inicial: $0.10
- Suministro circulante: 10M → 25M
- Precio final estimado: $0.10 * e^(4 * 15M / 100M) = $0.10 * e^0.6 ≈ $0.182
- Market cap: ~$4.55M

### B.2 Escenario Alcista
- Adopción alta: 50M circulantes en 12 meses
- Precio: $0.10 * e^(4 * 40M / 100M) = $0.10 * e^1.6 ≈ $0.495
- Market cap: ~$24.75M

### B.3 Retorno para Inversores Gold ($1,000 @ $0.10 = 10,000 XAV)
- Compra: 10,000 XAV @ $0.10
- Escenario base (12m): $0.182 → valor $1,820 (+82%)
- Escenario alcista (12m): $0.495 → valor $4,950 (+395%)
- APY por staking Gold (12.5%): +1,250 XAV → +$227 base / +$618 alcista
- **Total retorno base:** $2,047 (+105%)
- **Total retorno alcista:** $5,568 (+457%)

---

*Este whitepaper es un documento vivo. Se actualizará con cada release de Xavier.*
