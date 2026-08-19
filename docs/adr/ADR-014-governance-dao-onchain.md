# ADR-014: Arquitectura de Gobernanza DAO On-Chain — Polygon Amoy Testnet Stub

*Status: ACCEPTED | Date: 2026-07-28*

---

## Contexto

El ecosistema de Xavier requiere un mecanismo democrático, transparente y criptográficamente verificable para votar y aprobar propuestas comunitarias (Epics, agrupaciones de issues, cambios de parámetros). Anteriormente, existía únicamente un diseño básico de DAO simulada (Mock DAO) en memoria, sin soporte para persistencia descentralizada ni ejecución on-chain.

Para satisfacer la madurez de la red y establecer la infraestructura necesaria de cara a la red principal, este documento propone y aprueba la integración de un "on-chain stub" que interactúa directamente con contratos inteligentes de Solidity desplegados en la red de pruebas (Testnet) Polygon Amoy.

---

## Decisiones de Aceptación (Aceptados)

### 1. Contrato Inteligente XavierDAO (`XavierDAO.sol`)
- **Estado**: **ACEPTADO**
- **Decisión**: Crear e implementar el contrato inteligente de gobernanza descentralizada `XavierDAO.sol` bajo el estándar de Solidity `^0.8.20`.
- **Justificación**: El contrato implementa un flujo robusto de propuesta, votación y ejecución (`propose/vote/execute`). Utiliza identificadores bytes32 para referenciar las propuestas por su ID de clúster, registra los votos emitidos por cada dirección de remitente para evitar votos duplicados, y determina si la propuesta es aprobada para su ejecución conforme a las reglas democráticas.
- **Mapeo al Código**: `mesh/governance/contracts/XavierDAO.sol`

### 2. Cliente de Enlace de Rust con Alloy (`OnchainDaoClient`)
- **Estado**: **ACEPTADO**
- **Decisión**: Implementar enlaces nativos en Rust utilizando la biblioteca moderna `alloy` (v2.1) dentro de `src/mesh/governance/onchain.rs`.
- **Justificación**: Separar la lógica específica de la red EVM en un módulo modular (`onchain.rs`) y exponerla a través del cliente `OnchainDaoClient` permite desacoplar completamente la implementación on-chain de las estructuras de datos de gobernanza local del agente. Proporciona métodos con tipado estático seguro para llamar a `createProposal`, `castVote`, `executeProposal` y consultar estados.
- **Mapeo al Código**: `src/mesh/governance/onchain.rs`

### 3. Red de Pruebas Polygon Amoy (Testnet 80002)
- **Estado**: **ACEPTADO**
- **Decisión**: Configurar el stub y el cliente on-chain con soporte nativo para Polygon Amoy testnet (Chain ID `80002`), consumiendo RPCs de pruebas públicas o dedicadas.
- **Justificación**: Amoy proporciona un entorno de pruebas idéntico a Polygon (capa 2 segura y de bajo costo de gas), perfecto para verificar la integración y la firma de transacciones locales sin arriesgar fondos reales.

---

## Decisiones de Rechazo (Rechazados)

### 1. Despliegue en Red Principal (Mainnet) Directo
- **Estado**: **RECHAZADO**
- **Justificación**: Un despliegue inmediato en producción en la red principal introduce altos riesgos financieros y de seguridad sin antes haber madurado el código en el laboratorio de pruebas de SWAL. Se aprueba únicamente el uso de testnets.

---

## Plan de Fases

```
+------------------------------------------+       +------------------------------------------+
|  Fase 1: Contrato XavierDAO.sol          | ----> |  Fase 2: Cliente Alloy + Mock Integration|
|  - Solidity propose/vote/execute         |       |  - onchain.rs / mod.rs / Pruebas unitarias|
+------------------------------------------+       +------------------------------------------+
```

### Fase 1: Contrato XavierDAO.sol
- Desarrollo del contrato de Solidity.
- Compilación y verificación con `solc` para asegurar conformidad de bytecode y firmas de funciones ABI.

### Fase 2: Cliente Alloy e Integración
- Creación de bindings de alloy eficientes.
- Integración en `DaoGovernanceSystem` para realizar llamadas a la blockchain cuando la configuración de EVM esté presente.
- Creación de pruebas unitarias locales robustas que prueben la interoperabilidad.

---

## Consecuencias

### Positivas (+)
- **Transparencia Criptográfica**: Cada voto de gobernanza y estado de propuesta se registra inmutablemente on-chain.
- **Seguridad Perimetral**: No se comparten claves privadas centralizadamente; los nodos firman sus propias transacciones.
- **Arquitectura Limpia**: Todo el código de alloy y Solidity se agrupa modularmente en la crate sin afectar el rendimiento base de la memoria del RAG.

### Negativas (-)
- **Dependencia de Red**: Requiere acceso a un proveedor RPC de Ethereum para interactuar con la cadena, de lo contrario las transacciones fallarán (lo cual se maneja de forma segura como soft-fail en el código).

---

## Referencias
- [Estrategia de Simplificación XTSP](docs/research/PLAN-DEFINITIVO-XTSP-GPU-LAB.md)
- [ADR-013: Rediseño de Cómputo Distribuido](./ADR-013-compute-node-market-clavis-v2.md)
- [Línea Base de Tokens de SWAL](../../AGENTS.md)
