# ADR-043 — Master key initialization fails closed on ambiguous state

| Campo | Valor |
|-------|--------|
| **ID** | ADR-043 |
| **Estado** | Propuesto |
| **Fecha** | 2026-10-08 |
| **Relacionados** | ADR-038 |

## Contexto

`MasterKeyManager::load_or_init` reads the master key from the OS keyring and from
an encrypted fallback file (`~/.xavier/master.key`). Minting a new key while older
key material still exists would leave existing data unreadable, so initialization
has to decide explicitly what to do when the two sources disagree or are unusable.

## Opciones que compiten

| Opción | Descripción | Coste/complejidad esperada |
|--------|-------------|----------------------------|
| A — Fail closed on ambiguous state (adopted) | When no key is reachable anywhere (the keyring answers "no entry" *or* errors) and the data dir already holds `node/record.key` or a `vec-store.sqlite3*` / `memory-store.sqlite3*` file, initialization refuses to mint and names the recovery path. | Low: one guard plus one predicate over the data dir; one explicit behaviour per state. |
| B — Mint whenever the keyring has no entry (status quo) | Any keyring miss or error on a node that already holds data silently produces a second master key. | Zero code cost; unbounded data cost: the existing store stays unreadable and start-up reports success. |
| C — Always fail closed on a keyring error | Treat "keyring unreachable" as fatal even when the data dir is empty. | Low code cost, but a locked or absent keyring blocks a fresh install that has nothing to lose; the encrypted fallback file exists precisely for that case. |
| D — Overwrite the fallback file when minting | Write with `write_private_file` (create + truncate) instead of create-new. | Removes the lost-create race handling and lets a second writer destroy the only copy of the previous key. Rejected: the mint path creates the file it just failed to read, it never rewrites bytes it did not produce. |

## Simulación (OBLIGATORIO)

**Recorded decision: accept on deterministic gates instead of a simulation report**, per the ADR-041
precedent (its `## Simulation (REQUIRED)` records the same waiver): the states this ADR decides
over are enumerable and each one has an executable test, so a Monte-Carlo composite over modelled
costs would add no evidence. The engine (`periferia/swal-sim/adr_sim.py`) lives in a separate repo
and has no model for this decision. **Runs / seed: not run — waived, not pending.** Runnable
inputs if a run is preferred later: `python3 adr_sim.py --model adr_043_master_key_init --runs
5000 --seed 42 --outdir reports` → `periferia/swal-sim/reports/ADR-043-adr_043_master_key_init.md`
(+ `.json`). **Honesty note:** the waiver rests on the executable tests listed under *Verificación
posterior*, not on a measured score.

## Supuestos y su anclaje

| Parámetro | Valor usado | Fuente |
|-----------|-------------|--------|
| Fallback file location | `~/.xavier/master.key`, an AES-256-GCM blob under a machine-derived wrapping key | `src/keystore/mod.rs` (`get_fallback_path`, `derive_fallback_encryption_key`); ADR-038 |
| Primary key source | the OS keyring entry; the encrypted file is the fallback | `src/keystore/mod.rs` (`load_from_keyring`, `save_to_keyring`) |
| "Existing node data" | `node/record.key` present, or any `vec-store.sqlite3*` / `memory-store.sqlite3*` entry in the data dir | `src/keystore/mod.rs` (`data_dir_has_existing_material`) |
| Data dir | two ancestors above `at_rest::record_key_path()` (which is `<data_dir>/node/record.key`) | `src/keystore/mod.rs` (`node_data_dir`); `src/memory/sqlite_vec_store/at_rest.rs` (`record_key_path`) |
| Unreadable data dir | counted as "has material": it cannot prove it is empty | `src/keystore/mod.rs`, the `read_dir` error branch of `data_dir_has_existing_material` |
| Keyring error vs. "no entry" | an error is not evidence of absence, so it cannot license a mint on a data dir with material | **ASSUMPTION** — the code splits `keyring::Error::NoEntry` (absence) from any other error (unreachable); this ADR makes that split normative |
| Keyring error on an empty data dir | a mint into the encrypted file is still allowed, with a warning | `src/keystore/mod.rs` (`load_or_init_at`: "Keyring unavailable … minting a new master key") |
| Mint writes are create-new | `create_new(true)` + `O_NOFOLLOW`, mode `0600`; a file that appears first is adopted, never overwritten | `src/keystore/mod.rs` (`create_private_file_new`; test `init_mint_path_does_not_overwrite_a_file_that_appears_first`) |
| Restore requires a KCV | a restore with no KCV is rejected (`kcv_required`); `--key-file` (or `-`) carries the key and `--kcv` overrides the stored value | `src/recovery/store.rs` (`restore_into`, `restore_record_key`); `src/cli/commands/enums.rs` (`RecoveryCommand::Restore`) |
| A new master key does not rescue existing rows | per-record DEKs are wrapped with the node record key (`XRK1`), not with the master key | ADR-038; `src/memory/sqlite_vec_store/at_rest.rs` |

## Decision

- Keyring hit and fallback file decrypts to the same key: use it.
- Keyring hit and fallback file decrypts to a different key: fail with an error.
- Keyring hit and fallback file unusable: use the keyring key, log a warning, never
  write or delete the file.
- Keyring miss and fallback usable: adopt the file key and copy it to the keyring.
- Keyring miss and fallback unusable or unreadable: fail with an error.
- No fallback file and no keyring key: a new key is minted only when no key material
  exists anywhere, whether the keyring answers "no entry", errors or is unreachable.
  If the data dir holds a `node/record.key` or a store database (vec-store or
  memory-store sqlite), initialization fails and tells the operator to unlock the
  keyring or restore `master.key` with `xavier recovery restore`. The new file is
  created with create-new, mode 0600, and never overwritten.
- Legacy wrapped fallback files are re-wrapped in place with the same key.
- Unseal and restore require a stored key check value (KCV); a recovery directory
  without one is refused.

## Consecuencias

- A stale fallback file no longer blocks start-up when the keyring has the key.
- A missing or unreachable keyring entry on a node with data (for example after a
  host identity change) stops start-up instead of creating a second key; this favours data safety over availability.
- Restoring from a recovery directory needs the KCV that `seal` writes.

## Verificación posterior

Every test named below exists in this tree; they run with `cargo test -p xavier --lib`.

**Fail-closed guard on existing node state** (`src/keystore/mod.rs`):
`init_keyring_error_with_record_key_fails_closed`,
`init_no_entry_with_record_key_fails_closed`,
`init_no_entry_with_store_database_fails_closed`,
`init_keyring_error_with_store_database_fails_closed`,
`init_keyring_error_with_unlisted_data_dir_state_fails_closed` — each asserts that the create
call count is 0, nothing is saved to the keyring, no file is written, and the error names
`xavier recovery restore`.

**Mint path** (`src/keystore/mod.rs`): `init_no_file_and_no_entry_mints_private_file` (0600),
`init_keyring_error_mints_only_on_empty_data_dir`,
`create_private_file_new_never_truncates_or_follows` (create-new + `O_NOFOLLOW`),
`init_mint_path_does_not_overwrite_a_file_that_appears_first`,
`init_lost_create_race_adopts_other_writers_key`.

**Sources that disagree** (`src/keystore/mod.rs`):
`init_keyring_and_file_mismatch_fails_untouched`,
`init_keyring_hit_with_undecryptable_file_uses_keyring`,
`init_keyring_miss_adopts_decryptable_file`,
`init_keyring_miss_undecryptable_file_fails_untouched`,
`init_path_that_is_a_directory_fails_untouched`,
`init_legacy_file_is_rewrapped_with_same_key`.

**Recovery path** (`src/recovery/store.rs`, `src/recovery/kcv.rs`,
`src/cli/commands/recovery.rs`): `restore_requires_kcv_before_installing`,
`absent_kcv_is_its_own_error`, `wrong_key_is_rejected_without_touching_the_target`,
`malformed_candidate_is_rejected`, `restore_refuses_to_overwrite_an_existing_key`,
`failed_restore_leaves_no_staging_file`, `restore_into_refuses_dangling_symlink_target`,
`restore_never_clobbers_an_existing_key`.

Observable metric: start-up on a node that holds a store, with an unreachable keyring, fails with
an error naming the recovery path instead of minting a second key; the complementary metric is
that a keyring error on an *empty* data dir still starts. Review date: the next wave verification
pass, or any change to the keyring backend — if the "no entry" versus error split changes, this
ADR needs a re-read.
