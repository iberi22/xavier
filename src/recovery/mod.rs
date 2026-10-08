//! Multi-path recovery for the keys that protect the memory store.
//!
//! # What is actually at stake (verified, 2026-10-04)
//!
//! The task framing said "recover the master key". That is the wrong target.
//! Reading the code and decrypting a real record from `data/vec-store.sqlite3`
//! shows the real chain of custody:
//!
//! ```text
//! record.content  --AES-256-GCM(dek)-->  ciphertext
//! record.encrypted_dek = "XRK1" || nonce12 || AES-256-GCM(dek, RECORD_KEY)
//! RECORD_KEY  <- $XAVIER_RECORD_KEY  (env)          <- not backed up anywhere
//!             <- <data_dir>/node/record.key (0600)  <- NOT in the backup script
//!             <- otherwise: a FRESH random key is minted and every row becomes
//!                permanently unreadable, silently, with only a warning
//! ```
//!
//! This chain describes **XRK1** records. **XDK2** records instead use the
//! default-space keystore (`spaces/default/keystore.json`), unlocked by the
//! master key. A seal of `record.key` therefore does not recover XDK2 rows.
//! The status report surfaces this gap when that default keystore exists.
//!
//! `master.key` also protects `auth2` (SQLCipher DB key + JWT) and the `secrets`
//! vault. This module seals `record.key`; its master KCV is an integrity marker,
//! not a backup of the master key.
//!
//! # Invariant: a recovery copy is never encrypted with the key it protects
//!
//! Every blob here is sealed under an Argon2id key derived from a *passphrase* or
//! from a *mnemonic-encoded random key*. `KeystoreGuard` asserts at compile time
//! that the record key is not used as its own wrapping key, because recovering a
//! key must never require that key.
//!
//! # Honest limits of this design
//!
//! Two paths, not four. A scheme with four weak paths is worse than two strong
//! ones:
//!
//! 1. **BIP39 mnemonic** ([`MnemonicPath`]) — 24 words carry 256 bits of *random*
//!    key material ([`MnemonicPath::generate`]), not a derivation. The paper is
//!    the weak link, not the entropy.
//! 2. **Passphrase-sealed local bootstrap** ([`PassphrasePath`]) — survives a lost
//!    disk when the file is copied to media outside the host.
//!
//! What this deliberately does **not** do:
//!
//! - **TPM as a recovery path.** A TPM cannot reveal a key it never stored, and
//!   `Esys_TR_Clear` or a PCR change after a NixOS kernel bump destroys the
//!   sealed object — it works *against* recovery. A TPM is fine as an at-rest
//!   wrapper, never as a path.
//! - **Drive escrow as "protection".** Ciphertext and key in the same Drive
//!   account: an attacker who takes the account has both and needs only the
//!   passphrase. It covers disk loss, not account compromise, and is labelled as
//!   such everywhere it appears.
//! - **OTP / Google recovery link.** Proves identity, carries no key material.
//!   Without a server holding the key it unlocks nothing. See
//!   [`otp_recovery_is_theater`].
//! - **Deriving the master key from the mnemonic.** The mnemonic encodes a random
//!   escrow key; the record key is *wrapped* by it. Binding the vault to the
//!   words themselves would make the paper the single point of failure.
//!
//! # Cómo se enchufaría Shamir
//!
//! Shamir Secret Sharing (`src/node_identity/shamir.rs`) podría integrarse repartiendo
//! la clave de recuperación de 32 bytes que hoy codifican las 24 palabras en N partes
//! con un umbral K. Un comando `unseal --shares` reconstruiría esa clave de 32 bytes
//! y continuaría por el mismo flujo de apertura de [`crate::recovery::mnemonic::MnemonicSeal`]
//! y validación de KCV.
//!
//! Riesgos y consideraciones operativas:
//! - Dónde vive cada parte: custodios independientes y almacenamiento desacoplado.
//! - Co-ubicación: evitar estrictamente que el archivo de sello resida en el mismo sitio
//!   o soporte que K partes del secreto, para no invalidar el umbral.
//!
//! # Non-destructive restore
//!
//! [`RecoveryStore::restore_into`] never overwrites: it writes to a sibling temp
//! file, validates the key-check value, and only then renames. A wrong key aborts
//! with the existing file intact. This matters because
//! [`crate::memory::sqlite_vec_store::at_rest::resolve_record_key_with_source`]
//! *generates a new key when none is found* — a restore that clobbers first and
//! checks later can permanently orphan every row.

pub mod kcv;
pub mod manifest;
pub mod mnemonic;
pub mod passphrase;
pub mod store;

pub use kcv::{compute_kcv, KcvError};
pub use manifest::{RecoveryManifest, RecoveryStatus, RECOVERY_FORMAT_VERSION};
pub use mnemonic::{MnemonicPath, MNEMONIC_WORD_COUNT};
pub use passphrase::PassphrasePath;
pub use store::{
    KcvState, KeySourceReport, RecoveryStore, RestoreOutcome, SealHealth, MNEMONIC_SEAL_FILE,
    PASSPHRASE_SEAL_FILE,
};
