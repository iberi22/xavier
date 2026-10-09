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
