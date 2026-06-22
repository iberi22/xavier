# Whitepaper Económico: Xavier Sovereign Mesh

## Introducción
El Xavier Sovereign Mesh representa la evolución del sistema de gobernanza y economía distribuida de Xavier. Este documento detalla el sistema de recompensas, la estructura de inversión con vesting progresivo y los mecanismos de estabilidad económica diseñados para asegurar un crecimiento sostenible y una alineación de incentivos a largo plazo entre todos los participantes.

## 1. Niveles de Inversión y Vesting Progresivo

Para incentivar la permanencia y el compromiso con la red, se establece una estructura de niveles de inversión con cláusulas de permanencia (lock-up) y liberación progresiva de capital.

| Nivel | Monto Mínimo (USD) | Bloqueo Total (Cliff) | 50% Liberado | 100% Liberado |
| :--- | :--- | :--- | :--- | :--- |
| **Bronze** | $1,000 | 2 meses | Mes 2 | Mes 4 |
| **Silver** | $5,000 | 4 meses | Mes 2 | Mes 6 |
| **Gold** | $10,000 | 6 meses | Mes 3 | Mes 9 |
| **Platinum** | $25,000 | 9 meses | Mes 4 | Mes 12 |
| **Diamond** | $50,000 | 12 meses | Mes 6 | Mes 18 |
| **Sovereign** | $100,000 | 18 meses | Mes 8 | Mes 24 |

### Cláusulas de Permanencia
Los fondos invertidos están sujetos a un periodo de bloqueo total durante el cual no pueden ser retirados. Tras este periodo, la liberación ocurre en dos etapas principales (50% y 100%) para mitigar la presión de venta masiva.

## 2. Sistema de Recompensas y APY Progresivo

Los nodos participantes y stakers reciben recompensas en XP/XAV calculadas según su nivel de inversión y contribución a la red. El APY (Anual Percentage Yield) escala significativamente con el nivel de compromiso:

- **Base**: 5%
- **Bronze**: 7.5%
- **Silver**: 10%
- **Gold**: 12.5%
- **Platinum**: 17.5%
- **Diamond**: 25%
- **Sovereign**: 40%

## 3. Economía Estable y Mecanismos de Control

La estabilidad del token XAV se mantiene mediante una combinación de algoritmos de mercado automatizados y políticas fiscales del protocolo.

### Curva de Vinculación (Bonding Curve)
Se implementa una **Bonding Curve exponencial suavizada**. Este mecanismo asegura liquidez inmediata y un descubrimiento de precio determinista basado en la oferta circulante.
- Precio aumenta exponencialmente con el suministro.
- La "suavización" reduce la volatilidad extrema durante grandes compras o ventas.

### Gestión de Reservas y Liquidez
- **Reserve Ratio Objetivo**: El protocolo mantiene un 25% del Market Cap en activos de reserva (ej. ETH/USDC) para respaldar el valor del token.
- **Protocol-Owned Liquidity (POL)**: El 20% del Market Cap es gestionado directamente por el protocolo en pools de liquidez descentralizados para asegurar profundidad de mercado.

### Mecanismos de Seguridad (Circuit Breakers)
Para proteger la economía ante caídas abruptas, se activan "Circuit Breakers" automáticos en tres niveles de caída de precio en un periodo de 24 horas:
1. **Nivel 1 (-15%)**: Suspensión temporal de ventas por 1 hora.
2. **Nivel 2 (-25%)**: Suspensión temporal de ventas por 6 horas y aumento del burn rate.
3. **Nivel 3 (-40%)**: Bloqueo total de transacciones salientes hasta intervención de la gobernanza (DAO).

### Suministro y Deflación
- **Burn Rate**: Se aplica una quema automática del 5% sobre todas las tarifas recolectadas por el uso de la red Mesh.
- **Inflación**: Se establece una inflación anual inicial del 2%, con un modelo decreciente anual hasta alcanzar un estado de equilibrio o deflación neta mediante el burn rate.

## 4. Gobernanza e Inversión
Los inversores en niveles superiores (Platinum, Diamond, Sovereign) obtienen un peso de voto amplificado en el Xavier DAO, permitiéndoles influir directamente en los parámetros de la Bonding Curve y la gestión del Tesoro.

---
*Xavier Sovereign Mesh v1.0 — Junio 2026*
