# ADR-038 — Key recovery protects `record.key`, not `master.key`

- **Status**: Accepted
- **Date**: 2026-10-04
- **Scope**: `src/recovery/`, `src/drive/`
- **Supersedes**: nothing. Contradicts the framing of the original request, which
  asked for multi-path recovery of `master.key`.

## Context

The request was to build several independent recovery paths for
`~/.xavier/master.key`, on the premise that losing it makes the memory store
permanently unreadable.

Two facts, both verified against production data rather than inferred from the
brief, invalidate that premise.

**1. `master.key` is an encrypted blob, and its wrapping key is re-derivable.**
The file is 60 bytes and not printable:

```
53b9549a598d03a9ccbf00d24bd182557767c6101d3ae6906a4a7f88e45b8697 71 0fd80e98a67367b9cb2a1dbf21155320a013f9ac9706740bf6a4c7
└──────────────── ciphertext 32 B ────────────────┘ └12┘ └──────── tag 16 ───────┘
```

That is `master.key = AES-256-GCM(32-byte key, K)` serialized as
`nonce(12) || ciphertext(32) || tag(16)` (`src/keystore/mod.rs:309-313`). `K` is
derived deterministically from the host name plus `/etc/machine-id`
(`src/keystore/mod.rs:183-218`, domain `xavier-fallback-wrapping-key-v2`). We
unwrapped the production blob with exactly that derivation. So `master.key` is
recoverable on the same machine by a reinstall, and unrecoverable on a different
machine only because `machine-id` differs.

**2. `master.key` does not protect the memory store.**
`src/memory/sqlite_vec_store/at_rest.rs` wraps every record DEK with the **node
record key**, resolved from `$XAVIER_RECORD_KEY` or `<data_dir>/node/record.key`
(`at_rest.rs:61-90`), not from `master.key`. Decrypting a real `XRK1` record from
`data/vec-store.sqlite3`:

| key | result |
|---|---|
| `data/node/record.key` | **decrypts** — plaintext recovered |
| `master.key` (unwrapped) | `InvalidTag` |

All 22,410 rows are `XRK1`; none are legacy plaintext. And when no record key
resolves, `resolve_record_key_with_source` **generates a fresh random key**
(`at_rest.rs:84-88`) and the node keeps running with a warning — so every row
silently becomes unreadable. `record.key` is also absent from
`xavier-backup-encrypted.sh`, which copies `master.key` but not
`data/node/record.key`.

Losing `master.key` costs a re-consent and a re-`auth2`. Losing `record.key`
costs the entire memory store.

## Decision

Recovery protects **`record.key`** as the primary asset. Two paths, both strong:

1. **BIP39 mnemonic** (`src/recovery/mnemonic.rs`) — 24 Spanish words *encode* a
   random 256-bit recovery key, which wraps the record key.
2. **Passphrase seal** (`src/recovery/passphrase.rs`) — Argon2id + AES-256-GCM,
   written to media outside the host.

`master.key` shares the same escrow as a secondary goal, and the Drive refresh
token is sealed under it (`src/drive/credentials.rs`) precisely because that key
is re-derivable: a credential should cost a re-consent, not data.

### Rejected alternatives

- **TPM as a recovery path.** A TPM cannot reveal a key it never stored, and a
  sealed object dies with `Esys_TR_Clear` or a PCR change after a NixOS kernel
  bump. It works *against* recovery. Usable as an at-rest wrapper, never as a
  path.
- **Drive escrow as "protection".** Ciphertext and key in the same Drive
  account: an attacker with the account has both and needs only the passphrase.
  It covers disk loss, not account compromise. Documented as such, not sold as
  protection.
- **OTP / Google recovery link.** Proves identity, carries no key material.
  Without a server holding the key it unlocks nothing. Theater.
- **Deriving the master key from the mnemonic.** Makes the paper the vault's only
  secret and prevents rotation. The mnemonic encodes a random key instead.
- **Four paths.** Four weak paths are worse than two strong ones.

## Consequences

- `RecoveryStatus::is_recoverable` returns `false` until the rclone **crypt
  passphrase** is accounted for. Recovering the key alone cannot read the
  snapshots, so reporting otherwise is the false comfort this design removes.
- Restore is non-destructive by construction (`src/recovery/store.rs`): stage to a
  sibling file, verify the KCV, then rename. There is no code path that writes
  over an existing key file.
- `HMAC(key, "xavier-kcv-v1")` distinguishes "wrong key" from "empty database"
  without touching a row.
- Two AES-GCM AEADs (`at_rest.rs`, `keystore`) and no MPC policy mean a wrong key
  surfaces as an error, never silent garbage — so **no hygiene, dedup, or prune
  job may delete rows it cannot decrypt.** Not enforced here; flagged as open.
- The plaintext `master.key` files on `gdrive:SWAL/backups/` are out of scope
  here and remain unaddressed.

## Do NOT merge with `src/security/recovery.rs`

`src/security/recovery.rs` (154 lines) recovers the **auth seed phrase**: it
generates a 12-word Spanish BIP39 mnemonic, stores an **Argon2id hash** of it, and
verifies backup codes. It never holds or unwraps key material — the phrase is
hashed, and the hash is what is stored.

`src/recovery/` (this ADR) recovers **encryption keys**: it wraps actual 32-byte
keys and stores **KCVs of those keys**.

They look similar and are not interchangeable:

| | `security::recovery` | `recovery/` (this ADR) |
|---|---|---|
| recovers | a login seed phrase | encryption key material |
| stores | Argon2id **hash** of the phrase | **ciphertext** wrapping a key + KCVs |
| failure mode | "wrong phrase" | "wrong key" → vault unreadable |
| entry point | `xavier auth …` | `xavier recovery …` |

Merging them would put key-unwrapping logic next to hash-verification logic under
one name, which is how a future change ends up logging or weakening the wrong one.

## Not a duplicate of the Fase 0 shell work

The shell path already in place is kept and not re-implemented:

- `~/.xavier-offline/record.key` (0600) — the offline copy.
- `xavier-backup-encrypted.sh` copies `record.key` into each snapshot.
- `xavier-backup-verify.sh` "Puerta 5" warns when no recovery path exists.
- `record.key` never reaches the remote (`--exclude '*.key'`).

`src/recovery/` adds what the shell cannot: a **key-check value** to prove a
restored key is the right one, and a **non-destructive install**. The CLI
(`xavier recovery …`) reads the same `~/.xavier` locations, so the two coexist.

## Related

- Drive scopes and loopback-vs-device-code: `src/drive/mod.rs` module docs.