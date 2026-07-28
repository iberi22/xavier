# SESSION REPORT — Decentralized Login (2026-07-28)

## Qué se hizo (Xavier)

1. **F0** `src/node_identity/` — BIP39-24, Shamir 2-of-3, vault Argon2id+AES-GCM, check-codes, derive, persist, hybrid_pack  
2. **CLI** `xavier node create|recover|status|anchor|anchor-pack`  
3. **F1** `mesh/{challenge,namespace,pro_gate}.rs` + vault→`NodeIdentity::from_derived`  
4. **F2** `polygon_anchor/` — ABI, dry-run, live-prepared, broadcast (`dao-evm`)  
5. **F3** hybrid pack + ADRs ML-KEM / bio-ZKP (research)  
6. Docs GitCore + FEATURE **95%** honest + SRS REQ-008  

## Fuera de este commit (otros repos)

- `maloca/packages/@swal/node` device-key + heartbeat-loop  
- `edge-mesh` xavier-bridge / hybrid-pack / features F-019…F-023  
- `docs/SWAL/*` monorepo (copias en `.gitcore/docs/`)  
- `worldexams` heartbeat mirror  

## Qué falta

| Item | Dueño |
|------|--------|
| Deploy `SwalIdentityRegistry` Amoy + smoke broadcast | Ops (key funded) |
| UI Maloca WebAuthn onboarding | Producto |
| Fase 4 fuzzy/ZKP spike | Research `F-023` |
| SLIP39 mnemonic shares | OOS (Shamir cumple) |

## Sanitize

- No `.env` / tokens en commit  
- Solo rama `main` local tras limpieza  
