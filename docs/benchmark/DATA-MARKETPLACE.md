# Xavier Data Commons — Data Marketplace Economic Model

This document outlines the economic parameters, pricing model, and revenue distribution mechanisms for the decentralized Data Marketplace of the Xavier/SWAL ecosystem.

---

## 1. Vision & Core Philosophy

The Data Commons marketplace empowers Xavier nodes and SWAL applications to exchange high-quality technical datasets (including telemetry, system logs, diagnostic metrics, and benchmark results) in a completely secure, privacy-preserving, and decentralized manner.

Our economy operates on three core principles:
1. **Utility-Driven Value**: All transactions are settled in the ecosystem's utility token, **$SWAL**.
2. **Fair Recompense**: Data providers receive the overwhelming majority of transaction fees, incentivizing continuous contribution.
3. **Reputation-Weighted Economy**: System-level trust (via EigenTrust) and staking of $SWAL directly determine pricing, security thresholds, and community governance weight. No credit card payments or central Stripe gates are utilized.

---

## 2. Pricing Tier Structure

Datasets listed on the marketplace are categorized into three pricing tiers:

| Tier | Name | Target Content | Pricing Formula | Base Rate |
| :--- | :--- | :--- | :--- | :--- |
| **Free** | `Free` | Open-source diagnostics, public baselines, and general telemetry. | $P = 0$ | $0.00$ $SWAL |
| **Standard** | `Colaborador` | Verified logs, structured technical telemetry, and diagnostic logs. | $P = \text{Size} \times R_{\text{base}} \times (1.0 + \text{Reputation}^+)$ | $0.10$ $SWAL / record |
| **Premium** | `Colaborador+` | High-quality annotated datasets, security telemetry, and gold benchmarks. | $P = \text{Size} \times R_{\text{base}} \times (1.0 + 1.5 \times \text{Reputation}^+)$ | $0.25$ $SWAL / record |

*Note: $\text{Reputation}^+$ refers to $\max(0.0, \text{Reputation})$, where Reputation is in the range $[-1.0, 1.0]$. This ensures that only positive reputation provides a pricing boost, while low or negative reputation does not artificially inflate prices.*

---

## 3. $SWAL Staking & Reputation Boost

Providers can stake $SWAL tokens to signal commitment and boost their provider reputation, allowing them to charge a premium reflecting higher security guarantees.

### Staking Formula
The reputation boost ($\Delta_{\text{rep}}$) is calculated linearly based on the staked amount:

$$\Delta_{\text{rep}} = \min\left(0.50, \frac{\text{Staked } \$SWAL}{2000}\right)$$

This means:
- Staking **1000 $SWAL** yields the maximum boost of **+0.50** to the provider's reputation.
- Staking **500 $SWAL** yields a boost of **+0.25**.
- Staking is capped; amounts above **2000 $SWAL** do not increase the reputation boost beyond **+0.50**.

The final boosted reputation is calculated and clamped within $[-1.0, 1.0]$:

$$\text{Reputation}_{\text{boosted}} = \text{clamp}\left(-1.0, 1.0, \text{Reputation}_{\text{base}} + \Delta_{\text{rep}}\right)$$

---

## 4. Revenue Share Distribution

To ensure long-term platform viability and continuous funding of ecosystem rewards, a 90/10 revenue share split is applied to all dataset access transactions:

- **Provider Share**: **90%** of the transaction price.
- **Platform Share**: **10%** of the transaction price.

### Rules and Edge-Case Handling:
- For any transaction where the price $P > 0$, the platform receives at least **1 token** to avoid rounding errors resulting in zero platform fees on small payments.
- If $P = 0$, both shares are $0$.
- The sum of Provider Share and Platform Share always equals the original transaction price: $P_{\text{provider}} + P_{\text{platform}} = P$.

---

## 5. Concrete Numeric Examples

### Example A: Standard Telemetry Dataset (`Colaborador` Tier)
- **Dataset Size**: 100 records
- **Provider Base Reputation**: 0.0
- **Staked $SWAL**: 0 tokens ($\Delta_{\text{rep}} = 0.0$)
- **Reputation**: 0.0

$$\text{Price} = 100 \times 0.10 \times (1.0 + 0.0) = 10 \text{ \$SWAL}$$

**Revenue Share**:
- **Platform (10%)**: $10 \times 0.10 = 1 \text{ \$SWAL}$
- **Provider (90%)**: $10 - 1 = 9 \text{ \$SWAL}$

---

### Example B: Premium Security Audit Dataset (`Colaborador+` Tier) with Staking Boost
- **Dataset Size**: 100 records
- **Provider Base Reputation**: 0.5
- **Staked $SWAL**: 1000 tokens ($\Delta_{\text{rep}} = +0.50$)
- **Boosted Reputation**: $0.5 + 0.5 = 1.0$

$$\text{Price} = 100 \times 0.25 \times (1.0 + 1.5 \times 1.0) = 25 \times 2.5 = 62.5 \approx 63 \text{ \$SWAL}$$

**Revenue Share**:
- **Platform (10%)**: $\text{round}(63 \times 0.10) = \text{round}(6.3) = 6 \text{ \$SWAL}$
- **Provider (90%)**: $63 - 6 = 57 \text{ \$SWAL}$
