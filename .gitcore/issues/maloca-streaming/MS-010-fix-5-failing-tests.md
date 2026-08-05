# MS-010 — Estabilización de Tests en Xavier (Wave · maloca-streaming)

| Campo | Valor |
|-------|--------|
| **% validado** | **100%** |
| **Estado** | done |
| **Repos** | `xavier` |
| **Refs** | `gestalt-xavier-readiness-v1.0.md` · `features.json` |
| **Prioridad** | P0 |
| **Esfuerzo** | 1d |

## Scope

Corregir los 5 tests unitarios fallidos en Xavier para lograr un suite de pruebas 100% verde (1,319/1,319 PASS):

1. `test_get_offline_status` / `test_get_offline_status_stopped`: Cambiar puerto `8006` a puerto efímero dinámico (`127.0.0.1:0`).
2. `test_overall_status_prioritization`: Aislamiento hermético de estado de pruebas.
3. `test_reindex_null_embeddings_background`: Limpiar variable de entorno `XAVIER_EMBEDDER` para que resuelva el embedder mockeado.
4. `test_custom_dedup_policies`: Ajustar embedding de prueba `rec4` a `-0.3` para evitar falsos positivos de deduplicación semántica.
5. `test_load_config_json`: Apuntar determinísticamente a `CARGO_MANIFEST_DIR/config/xavier.config.json`.

## Aceptación validada

- [x] `test_get_offline_status` pasando
- [x] `test_overall_status_prioritization` pasando
- [x] `test_reindex_null_embeddings_background` pasando
- [x] `test_custom_dedup_policies` pasando
- [x] `test_load_config_json` pasando
- [x] `cargo test -p xavier` 100% verde
