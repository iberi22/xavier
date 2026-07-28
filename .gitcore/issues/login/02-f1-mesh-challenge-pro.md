# DL-02 — F1 Mesh challenge / namespace / Pro gate

| Campo | Valor |
|-------|--------|
| **% validado** | **95%** |
| **Estado** | done |

## Scope

Challenge Ed25519 one-shot, namespaces `swal/{app}/{instance}`, `pro_gate` (Pro=nodo, never Stripe), bridge `NodeIdentity::from_derived`.

## Aceptación validada

- [x] Challenge sign/verify + replay reject
- [x] Instancias aisladas por namespace
- [x] Pro solo con identidad + heartbeat fresco + Xavier reachable
- [x] Commitment ML-DSA en respuesta challenge

## Tests

| Suite | Resultado |
|-------|-----------|
| `mesh::challenge` | PASS (2) |
| `mesh::namespace` | PASS (3) |
| `mesh::pro_gate` | PASS (5) |
| E2E `e2e_f1_mesh_challenge_namespace_pro_gate` | PASS |

## Paths

`src/mesh/{challenge,namespace,pro_gate,node}.rs`
