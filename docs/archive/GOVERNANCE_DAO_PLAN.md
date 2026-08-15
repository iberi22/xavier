# Governance DAO On-chain — Plan de implementación (Claude Code Audit)

**Issue: #166 | Objetivo: 5% → 60% maturity**

## Hallazgos clave del audit

1. **Mock actual**: 148 líneas, 1 archivo (`src/mesh/governance.rs`)
2. **NO está detrás de feature gate** — governance compila siempre, aunque depende de rand
3. **Feature `mesh`** existe en Cargo.toml: `mesh = ["dep:libp2p"]`
4. **Interfaz pública**: `new()`, `submit_proposal()`, `cast_vote()`, `evaluate_consensus()` (privado)
5. **Test coverage**: 2 tests (consensus + rejection)

## Plan de 5 pasos (Claude Code)

### Paso 1: EVM config struct + constructor alternativo (~60-80 líneas)
- Añadir `EvmDaoConfig` struct (rpc_url, contract_address, chain_id, private_key)
- Añadir `evm_config: Option<EvmDaoConfig>` a `DaoGovernanceSystem`
- Constructor `DaoGovernanceSystem::with_evm(config)` — no rompe `new()`

### Paso 2: submit_proposal → on-chain (~80-100 líneas)
- async submit_proposal que llama `createProposal(bytes32, string, string)` via alloy
- Feature-gated detrás de `#[cfg(feature = "dao-evm")]`

### Paso 3: cast_vote → on-chain (~60-80 líneas)
- async cast_vote que llama `castVote(bytes32, bool)` via alloy
- Mantiene sync API para no-EVM

### Paso 4: sync_from_chain + event listener (~80-100 líneas)
- Polling periódico de estado on-chain
- Sincroniza `active_proposals` con el contrato

### Paso 5: Tests EVM mock (~80-100 líneas)
- governance_dao_submit_vote_evm
- governance_dao_consensus_evm
- governance_dao_sync_from_chain

## Dependencias nuevas
- `alloy` = { version = "0.12", optional = true }
- Feature `dao-evm = ["dep:alloy"]`

## Constraints
- NO romper `new()` / `submit_proposal()` / `cast_vote()` existentes
- NO añadir deps pesadas (alloy es ~3MB compilado)
- Mantener feature gate: mock sin mesh, EVM con dao-evm
- cargo check --lib debe pasar sin dao-evm
