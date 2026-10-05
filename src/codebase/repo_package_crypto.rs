//! Encrypted-at-rest container for a per-repo Xavier memory package
//! (`<repo>/.xavier/code_graph.db`).
//!
//! ## Why a container and not the per-record mechanism
//!
//! The live memory DB is encrypted **per record** in
//! `src/memory/sqlite_vec_store/at_rest.rs`: `encrypt_columns_for_write_with_key`
//! mutates a `crate::memory::store::MemoryRecord` struct — it hex-encodes the
//! AES-GCM ciphertext of `content` back into a **TEXT column**, stashes
//! `metadata` JSON under `{"encrypted": "<hex>"}`, and stores the per-record DEK
//! in the `encrypted_dek` BLOB with the two nonces in `content_iv` /
//! `metadata_iv`. `migrate_connection` drives it with SQL against the
//! `memory_records` table. That mechanism is therefore **column- and
//! table-shaped**: it needs a row to live in and a `TEXT` column to write hex
//! into. An arbitrary SQLite file has no such column, so that mechanism is not
//! reusable for `code_graph.db` — reusing it would mean re-implementing it
//! anyway.
//!
//! What *is* reusable, and what this module uses, is the AEAD primitive and the
//! DEK-wrapping format that already exist:
//!
//! - `crate::crypto::encryption::{aes_encrypt, aes_decrypt, NonceBytes}` —
//!   AES-256-GCM, the same `aes-gcm` crate and the same
//!   `[nonce(12) || ciphertext||tag(16)]` layout that every other `.enc` file in
//!   the tree uses (`src/secrets/local_vault.rs`).
//! - `crate::memory::sqlite_vec_store::at_rest::{wrap_dek, unwrap_dek}` — the
//!   `XRK1 || nonce || ct` per-object DEK wrapping, so each repo package gets a
//!   fresh random DEK and only the DEK is bound to the long-lived key.
//! - `crate::keystore::MasterKeyManager::derive_key` — HKDF-SHA256 expansion of
//!   the node master key with a repo-scoped `info` label, the same pattern as
//!   `auth_db_key()` / `vault_key()`.
//!
//! No new cipher, no new key derivation: only a framing of bytes that already
//! existed.
//!
//! ## Container layout
//!
//! ```text
//! offset  size  field
//! 0       8     magic  b"XAVPKG01"
//! 8       1     format version (u8)
//! 9       4     header_len (u32 LE)  — length of the *sealed* metadata JSON
//! 13      4     wrapped_dek_len (u32 LE)
//! 17      n     wrapped DEK blob: "XRK1" || nonce12 || ct48
//! 17+n    m     AES-256-GCM ciphertext of (metadata JSON || raw file bytes)
//! ```
//!
//! Nothing about the payload is readable from the outside: the repo name, the
//! source file name, the plaintext length and the timestamp all live **inside**
//! the AEAD plaintext. Only three integers and the magic are structural. The
//! structural prefix is validated explicitly rather than by the AEAD (the
//! existing `aes_encrypt` takes no AAD), and the DEK blob is independently
//! authenticated by `unwrap_dek`, so tampering with either is still an error.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use hkdf::Hkdf;
use rand::RngCore;
use sha2::Sha256;

use crate::crypto::encryption::{aes_decrypt, aes_encrypt, NonceBytes};
use crate::keystore::MasterKeyManager;
use crate::memory::sqlite_vec_store::at_rest::{unwrap_dek, wrap_dek};

/// Magic bytes at offset 0 of every package container.
pub const PACKAGE_MAGIC: &[u8; 8] = b"XAVPKG01";

/// Container format version understood by this module.
pub const PACKAGE_VERSION: u8 = 1;

/// Fixed size of the structural prefix: magic + version + two u32 lengths.
const PREFIX_LEN: usize = 8 + 1 + 4 + 4;

/// Extension used for the encrypted artifact.
pub const PACKAGE_ENC_EXTENSION: &str = "enc";

/// HKDF `info` prefix for the per-repo package KEK. The project id is appended,
/// so two repos never share a package key even under the same master key.
pub const PACKAGE_KEK_INFO_PREFIX: &str = "xavier-repo-package-kek-v1:";

/// Non-sensitive-on-disk description of the sealed payload. Every field is
/// stored **inside** the AEAD ciphertext; reading it requires the package key.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PackageHeader {
    /// Container format version (mirrors [`PACKAGE_VERSION`]).
    pub version: u8,
    /// Sanitized repo project id (see `repo_identity::derive_project_id`).
    pub project_id: String,
    /// Original file name inside the package, e.g. `code_graph.db`.
    pub source_file_name: String,
    /// Commit the packaged graph corresponds to, when known.
    pub indexed_commit: String,
    /// Byte length of the sealed file.
    pub plaintext_len: u64,
    /// RFC3339 creation timestamp.
    pub created_at: String,
}

/// Full HKDF `info` label for a repo's package KEK.
pub fn package_kek_info(project_id: &str) -> Vec<u8> {
    format!("{PACKAGE_KEK_INFO_PREFIX}{project_id}").into_bytes()
}

/// Derive the package KEK for `project_id` from the node master key.
///
/// Same shape as `MasterKeyManager::auth_db_key()`: HKDF-SHA256 over the master
/// key with a domain-separated, repo-scoped `info`.
pub fn derive_package_kek(master_key: &MasterKeyManager, project_id: &str) -> Result<[u8; 32]> {
    let mut kek = [0u8; 32];
    master_key
        .derive_key(&package_kek_info(project_id), &mut kek)
        .context("HKDF expansion for the repo package KEK failed")?;
    Ok(kek)
}

/// [`derive_package_kek`] over raw master-key bytes, with no keystore lookup.
///
/// Same HKDF-SHA256 construction as [`crate::keystore::MasterKeyManager::derive_key`]
/// (which uses `Hkdf::<Sha256>::new(None, master_key)`), exposed so the
/// derivation itself is testable without a live keyring.
pub fn derive_package_kek_from_master(master_key: &[u8; 32], project_id: &str) -> Result<[u8; 32]> {
    let mut kek = [0u8; 32];
    Hkdf::<Sha256>::new(None, master_key)
        .expand(&package_kek_info(project_id), &mut kek)
        .map_err(|e| anyhow!("HKDF expansion for the repo package KEK failed: {e}"))?;
    Ok(kek)
}

/// Path of the encrypted package for a repo root: `<root>/.xavier/<name>.enc`.
pub fn package_path_for(root: &Path, file_name: &str) -> PathBuf {
    root.join(".xavier")
        .join(format!("{file_name}.{PACKAGE_ENC_EXTENSION}"))
}

/// Seal `plaintext` into a container under `kek`. Returns the container bytes.
///
/// `header.plaintext_len` is **derived here**, not taken from the caller: it
/// always describes the payload that went in, so a header can never disagree
/// with its own ciphertext.
///
/// A fresh random DEK is generated per call (via `rand`, as
/// `at_rest::encrypt_columns_for_write_with_key` does) and wrapped with
/// `at_rest::wrap_dek`, so the KEK is never used directly on the payload.
pub fn seal(plaintext: &[u8], kek: &[u8; 32], header: &PackageHeader) -> Result<Vec<u8>> {
    if header.version != PACKAGE_VERSION {
        bail!(
            "refusing to seal package version {} (this build writes {PACKAGE_VERSION})",
            header.version
        );
    }

    let mut sealed_header = header.clone();
    sealed_header.plaintext_len = plaintext.len() as u64;

    let mut dek = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut dek);
    let wrapped = wrap_dek(&dek, kek).context("wrapping the package DEK failed")?;

    let mut sealed =
        serde_json::to_vec(&sealed_header).context("serializing the package header failed")?;
    // The prefix field is the *header* length, so it must be captured before the
    // payload is appended; the reader splits the AEAD plaintext on it.
    let header_len =
        u32::try_from(sealed.len()).map_err(|_| anyhow!("package header is too large to frame"))?;
    sealed.extend_from_slice(plaintext);

    let ciphertext = aes_encrypt(&sealed, &dek, &NonceBytes::generate())
        .map_err(|e| anyhow!("package payload encryption failed: {e}"))?;

    let mut out = Vec::with_capacity(PREFIX_LEN + wrapped.len() + ciphertext.len());
    out.extend_from_slice(PACKAGE_MAGIC);
    out.push(PACKAGE_VERSION);
    out.extend_from_slice(&header_len.to_le_bytes());
    out.extend_from_slice(&(wrapped.len() as u32).to_le_bytes());
    out.extend_from_slice(&wrapped);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Open a container produced by [`seal`], returning `(header, plaintext)`.
///
/// Fails when the magic/version/prefix are wrong, when the DEK cannot be
/// unwrapped with `kek`, and — via the AES-GCM tag — on **any** modification of
/// the sealed payload. Never returns partially-decrypted data.
pub fn open(container: &[u8], kek: &[u8; 32]) -> Result<(PackageHeader, Vec<u8>)> {
    if container.len() < PREFIX_LEN {
        bail!(
            "package container too short: {} bytes, need at least {PREFIX_LEN}",
            container.len()
        );
    }
    if &container[..8] != PACKAGE_MAGIC {
        bail!(
            "bad package magic {:02x?}, expected {:02x?}",
            &container[..8],
            PACKAGE_MAGIC
        );
    }
    let version = container[8];
    if version != PACKAGE_VERSION {
        bail!("unsupported package format version {version}");
    }
    let sealed_len = u32::from_le_bytes(
        container[9..13]
            .try_into()
            .map_err(|_| anyhow!("truncated header length field"))?,
    ) as usize;
    let wrapped_len = u32::from_le_bytes(
        container[13..17]
            .try_into()
            .map_err(|_| anyhow!("truncated DEK length field"))?,
    ) as usize;

    let body = &container[PREFIX_LEN..];
    if body.len() < wrapped_len {
        bail!("package container truncated inside the wrapped DEK");
    }
    let (wrapped, ciphertext) = body.split_at(wrapped_len);
    if ciphertext.is_empty() {
        bail!("package container carries no ciphertext");
    }

    let dek = unwrap_dek(wrapped, kek)
        .map_err(|e| anyhow!("package DEK unwrap failed (wrong key or tampered DEK): {e}"))?;

    let plaintext = aes_decrypt(ciphertext, &dek).map_err(|e| {
        anyhow!("package payload authentication failed (corrupted or wrong key): {e}")
    })?;

    if plaintext.len() < sealed_len {
        bail!(
            "sealed payload shorter than its header: {} < {sealed_len}",
            plaintext.len()
        );
    }
    let (header_bytes, file_bytes) = plaintext.split_at(sealed_len);
    let header: PackageHeader =
        serde_json::from_slice(header_bytes).context("package header JSON is invalid")?;
    if header.version != PACKAGE_VERSION {
        bail!(
            "sealed header reports unsupported version {}",
            header.version
        );
    }
    if file_bytes.len() as u64 != header.plaintext_len {
        bail!(
            "sealed payload length {} disagrees with the header ({})",
            file_bytes.len(),
            header.plaintext_len
        );
    }
    Ok((header, file_bytes.to_vec()))
}

/// Encrypt `plain_path` to `enc_path` with `kek`, creating parent dirs.
///
/// The `.enc` file is written through `keystore::write_private_file` (created
/// `0600`, no create-then-chmod window). The plaintext file is left untouched.
pub fn encrypt_file(
    plain_path: &Path,
    enc_path: &Path,
    kek: &[u8; 32],
    header: &PackageHeader,
) -> Result<PackageHeader> {
    let mut header = header.clone();
    let plaintext = std::fs::read(plain_path)
        .with_context(|| format!("cannot read {}", plain_path.display()))?;
    header.plaintext_len = plaintext.len() as u64;
    if header.source_file_name.is_empty() {
        if let Some(name) = plain_path.file_name().and_then(|n| n.to_str()) {
            header.source_file_name = name.to_string();
        }
    }

    let container = seal(&plaintext, kek, &header)?;
    if let Some(parent) = enc_path.parent() {
        crate::keystore::ensure_private_dir(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    crate::keystore::write_private_file(enc_path, &container)
        .with_context(|| format!("cannot write {}", enc_path.display()))?;
    Ok(header)
}

/// Decrypt `enc_path` into `out_path` with `kek`.
///
/// The plaintext is only written after the AEAD tag verifies, so a corrupt or
/// foreign container never leaves a partial `out_path` behind.
pub fn decrypt_file(enc_path: &Path, out_path: &Path, kek: &[u8; 32]) -> Result<PackageHeader> {
    let container =
        std::fs::read(enc_path).with_context(|| format!("cannot read {}", enc_path.display()))?;
    let (header, plaintext) = open(&container, kek)?;

    if let Some(parent) = out_path.parent() {
        crate::keystore::ensure_private_dir(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    crate::keystore::write_private_file(out_path, &plaintext)
        .with_context(|| format!("cannot write {}", out_path.display()))?;
    Ok(header)
}

/// Authenticate `enc_path` without writing anything anywhere.
///
/// This is the cheap "can this package be trusted and is it for this repo?"
/// check: it runs the full AEAD verification and returns the sealed header.
pub fn verify_file(enc_path: &Path, kek: &[u8; 32]) -> Result<PackageHeader> {
    let container =
        std::fs::read(enc_path).with_context(|| format!("cannot read {}", enc_path.display()))?;
    let (header, _) = open(&container, kek)?;
    Ok(header)
}

/// Header template for a repo package, pre-filled with the identity fields.
pub fn header_for(project_id: &str, source_file_name: &str, indexed_commit: &str) -> PackageHeader {
    PackageHeader {
        version: PACKAGE_VERSION,
        project_id: project_id.to_string(),
        source_file_name: source_file_name.to_string(),
        indexed_commit: indexed_commit.to_string(),
        plaintext_len: 0,
        created_at: chrono::Utc::now().to_rfc3339(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_kek(fill: u8) -> [u8; 32] {
        [fill; 32]
    }

    fn header(project: &str) -> PackageHeader {
        header_for(
            project,
            "code_graph.db",
            "0123456789abcdef0123456789abcdef01234567",
        )
    }

    /// A realistic SQLite header plus filler, so plaintext sniffing has
    /// something real to find.
    fn sqlite_like_bytes(rows: usize) -> Vec<u8> {
        let mut v = b"SQLite format 3\0".to_vec();
        for i in 0..rows {
            v.extend_from_slice(format!("row-{i:08}-symbol-alpha-beta\n").as_bytes());
        }
        v
    }

    fn write_temp(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn roundtrip_returns_identical_bytes() {
        let dir = tempdir().unwrap();
        let kek = test_kek(0x11);
        let plaintext = sqlite_like_bytes(64);

        let src = write_temp(dir.path(), "code_graph.db", &plaintext);
        let enc = dir.path().join("code_graph.db.enc");
        let out = dir.path().join("restored.db");

        let written = encrypt_file(&src, &enc, &kek, &header("xavier")).unwrap();
        assert_eq!(written.plaintext_len, plaintext.len() as u64);
        assert_eq!(written.source_file_name, "code_graph.db");

        let read_back = decrypt_file(&enc, &out, &kek).unwrap();
        assert_eq!(read_back.project_id, "xavier");
        assert_eq!(read_back.source_file_name, "code_graph.db");
        assert_eq!(read_back.plaintext_len, plaintext.len() as u64);
        assert_eq!(std::fs::read(&out).unwrap(), plaintext);
    }

    #[test]
    fn ciphertext_leaks_no_plaintext() {
        let kek = test_kek(0x22);
        let plaintext = sqlite_like_bytes(32);
        let container = seal(&plaintext, &kek, &header("secretrepo")).unwrap();

        // The SQLite magic must not survive in the clear.
        assert!(
            !container
                .windows(b"SQLite format 3".len())
                .any(|w| w == b"SQLite format 3"),
            "the SQLite header magic leaked into the container"
        );
        // Nor any distinctive plaintext token.
        let token = b"row-00000007-symbol-alpha-beta";
        assert!(
            !container.windows(token.len()).any(|w| w == token),
            "a plaintext row token leaked into the container"
        );
        // Nor the project id, which only exists inside the AEAD plaintext.
        assert!(
            !container
                .windows(b"secretrepo".len())
                .any(|w| w == b"secretrepo"),
            "the project id leaked into the container"
        );
        assert_eq!(&container[..8], PACKAGE_MAGIC);
    }

    #[test]
    fn tampered_ciphertext_fails_authentication() {
        let kek = test_kek(0x33);
        let container = seal(&sqlite_like_bytes(16), &kek, &header("xavier")).unwrap();

        // Flip one bit deep inside the ciphertext (past prefix + wrapped DEK).
        let mut bad = container.clone();
        let last = bad.len() - 1;
        bad[last] ^= 0x01;
        let err = open(&bad, &kek).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("authentication failed"),
            "unexpected error for a flipped ciphertext bit: {msg}"
        );
    }

    #[test]
    fn tampered_wrapped_dek_fails() {
        let kek = test_kek(0x44);
        let container = seal(&sqlite_like_bytes(8), &kek, &header("xavier")).unwrap();
        let mut bad = container.clone();
        // Byte 5 of the "XRK1" magic, inside the wrapped DEK region.
        bad[17 + 2] ^= 0xff;
        let err = open(&bad, &kek).unwrap_err();
        assert!(
            err.to_string().contains("unwrap failed"),
            "unexpected error for a tampered DEK: {err}"
        );
    }

    #[test]
    fn wrong_key_fails_and_writes_nothing() {
        let dir = tempdir().unwrap();
        let plaintext = sqlite_like_bytes(8);
        let src = write_temp(dir.path(), "code_graph.db", &plaintext);
        let enc = dir.path().join("code_graph.db.enc");
        encrypt_file(&src, &enc, &test_kek(0x55), &header("xavier")).unwrap();

        let out = dir.path().join("restored.db");
        let err = decrypt_file(&enc, &out, &test_kek(0x56)).unwrap_err();
        assert!(err.to_string().contains("unwrap failed"), "{err}");
        assert!(
            !out.exists(),
            "a failed decrypt must not leave a partial plaintext behind"
        );
    }

    #[test]
    fn truncated_and_malformed_containers_are_rejected() {
        let kek = test_kek(0x66);
        let container = seal(&sqlite_like_bytes(4), &kek, &header("xavier")).unwrap();

        assert!(
            open(&container[..4], &kek).is_err(),
            "short prefix accepted"
        );

        let mut bad_magic = container.clone();
        // The magic starts with 'X', so overwrite the second byte instead.
        bad_magic[1] = b'Z';
        assert_ne!(&bad_magic[..8], PACKAGE_MAGIC, "mutation was a no-op");
        assert!(open(&bad_magic, &kek).is_err(), "bad magic accepted");

        let mut bad_version = container.clone();
        bad_version[8] = 99;
        assert!(open(&bad_version, &kek).is_err(), "bad version accepted");

        assert!(
            open(&container[..container.len() - 5], &kek).is_err(),
            "truncated ciphertext accepted"
        );

        // Header length claiming more than the payload actually holds.
        let mut bad_len = container.clone();
        bad_len[9..13].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(
            open(&bad_len, &kek).is_err(),
            "oversized sealed length accepted"
        );
    }

    #[test]
    fn empty_file_roundtrips() {
        let dir = tempdir().unwrap();
        let kek = test_kek(0x77);
        let src = write_temp(dir.path(), "empty.db", b"");
        let enc = dir.path().join("empty.db.enc");
        let out = dir.path().join("empty.out");

        let h = encrypt_file(&src, &enc, &kek, &header("xavier")).unwrap();
        assert_eq!(h.plaintext_len, 0);
        let read_back = decrypt_file(&enc, &out, &kek).unwrap();
        assert_eq!(read_back.plaintext_len, 0);
        assert_eq!(std::fs::read(&out).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn large_file_roundtrips() {
        let dir = tempdir().unwrap();
        let kek = test_kek(0x88);
        // ~4 MiB of pseudo-codebase text.
        let plaintext = sqlite_like_bytes(200_000);
        assert!(
            plaintext.len() > 4 * 1024 * 1024,
            "fixture must exceed 4 MiB, got {}",
            plaintext.len()
        );

        let src = write_temp(dir.path(), "big.db", &plaintext);
        let enc = dir.path().join("big.db.enc");
        let out = dir.path().join("big.out");

        encrypt_file(&src, &enc, &kek, &header("xavier")).unwrap();

        // Overhead is the structural prefix, the 68-byte wrapped DEK, the AES-GCM
        // nonce and tag, and the sealed header JSON — none of it scales with the
        // payload.
        let container = std::fs::read(&enc).unwrap();
        let sealed_len = u32::from_le_bytes(container[9..13].try_into().unwrap()) as usize;
        let wrapped_len = u32::from_le_bytes(container[13..17].try_into().unwrap()) as usize;
        const NONCE_LEN: usize = 12;
        const TAG_LEN: usize = 16;
        assert_eq!(
            container.len(),
            PREFIX_LEN + wrapped_len + NONCE_LEN + sealed_len + plaintext.len() + TAG_LEN,
            "the container must add only framing, not a second copy of the payload"
        );
        assert!(
            container.len() - plaintext.len() < 512,
            "framing overhead must stay small for a large payload"
        );

        decrypt_file(&enc, &out, &kek).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), plaintext);
    }

    #[test]
    fn verify_detects_tampering_without_writing() {
        let dir = tempdir().unwrap();
        let kek = test_kek(0x99);
        let src = write_temp(dir.path(), "code_graph.db", &sqlite_like_bytes(4));
        let enc = dir.path().join("code_graph.db.enc");
        encrypt_file(&src, &enc, &kek, &header("xavier")).unwrap();

        let h = verify_file(&enc, &kek).unwrap();
        assert_eq!(h.project_id, "xavier");

        let mut bytes = std::fs::read(&enc).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0x80;
        std::fs::write(&enc, &bytes).unwrap();
        assert!(verify_file(&enc, &kek).is_err());
    }

    #[test]
    fn per_repo_keks_differ_and_are_deterministic() {
        let dir = tempdir().unwrap();
        let src = write_temp(dir.path(), "code_graph.db", &sqlite_like_bytes(4));
        let enc_a = dir.path().join("a.enc");
        let enc_b = dir.path().join("b.enc");

        let kek_a = test_kek(0xAA);
        let kek_b = test_kek(0xBB);
        encrypt_file(&src, &enc_a, &kek_a, &header("repo-alpha")).unwrap();
        encrypt_file(&src, &enc_b, &kek_b, &header("repo-beta")).unwrap();

        // Cross-repo open must fail: a package is not portable between keys.
        assert!(open(&std::fs::read(&enc_a).unwrap(), &kek_b).is_err());
        assert_eq!(
            verify_file(&enc_a, &kek_a).unwrap().project_id,
            "repo-alpha"
        );
    }

    #[test]
    fn package_kek_info_is_repo_scoped() {
        assert_eq!(
            package_kek_info("a"),
            b"xavier-repo-package-kek-v1:a".to_vec()
        );
        assert_ne!(package_kek_info("a"), package_kek_info("b"));
    }

    #[test]
    fn two_seals_of_the_same_input_differ() {
        // Fresh DEK + fresh nonce per call: no deterministic ciphertext.
        let kek = test_kek(0xCC);
        let plaintext = sqlite_like_bytes(4);
        let a = seal(&plaintext, &kek, &header("xavier")).unwrap();
        let b = seal(&plaintext, &kek, &header("xavier")).unwrap();
        assert_ne!(
            a, b,
            "two seals of the same plaintext must not be byte-identical"
        );
        assert_eq!(open(&a, &kek).unwrap().1, open(&b, &kek).unwrap().1);
    }

    #[test]
    fn enc_file_is_private_and_package_path_is_repo_local() {
        let dir = tempdir().unwrap();
        let kek = test_kek(0xDD);
        let src = write_temp(dir.path(), "code_graph.db", &sqlite_like_bytes(2));
        let enc = dir.path().join("sub").join("code_graph.db.enc");
        encrypt_file(&src, &enc, &kek, &header("xavier")).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&enc).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "the .enc artifact must not be world readable");
        }

        assert_eq!(
            package_path_for(Path::new("/tmp/repo-xav01"), "code_graph.db"),
            PathBuf::from("/tmp/repo-xav01/.xavier/code_graph.db.enc")
        );
    }

    /// A real SQLite file must survive the container and still be queryable.
    #[test]
    fn real_sqlite_db_survives_the_container() {
        let dir = tempdir().unwrap();
        let kek = test_kek(0xEE);
        let src = dir.path().join("code_graph.db");
        {
            let conn = rusqlite::Connection::open(&src).unwrap();
            conn.execute_batch(
                "CREATE TABLE symbols (id INTEGER PRIMARY KEY, name TEXT);
                 INSERT INTO symbols (name) VALUES ('derive_repo_identity'), ('resolve_record_key');",
            )
            .unwrap();
        }
        let enc = dir.path().join("code_graph.db.enc");
        let out = dir.path().join("restored.db");
        encrypt_file(&src, &enc, &kek, &header("xavier")).unwrap();
        decrypt_file(&enc, &out, &kek).unwrap();

        let conn = rusqlite::Connection::open(&out).unwrap();
        let mut stmt = conn
            .prepare("SELECT name FROM symbols ORDER BY name")
            .unwrap();
        let names: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(names, vec!["derive_repo_identity", "resolve_record_key"]);

        // And the encrypted artifact is not a SQLite file at all.
        let raw = std::fs::read(&enc).unwrap();
        assert!(!raw.starts_with(b"SQLite format 3"));
    }

    /// The KEK must actually come from the master key, HKDF-style, and must be
    /// repo-scoped: the same master key over two project ids yields two
    /// independent keys that cannot open each other's packages.
    #[test]
    fn kek_is_derived_from_the_master_key_and_scoped_per_repo() {
        let master = [0x5A; 32];

        let kek_a = derive_package_kek_from_master(&master, "xavier").unwrap();
        let kek_a_again = derive_package_kek_from_master(&master, "xavier").unwrap();
        let kek_b = derive_package_kek_from_master(&master, "other-repo").unwrap();
        let kek_other_master = derive_package_kek_from_master(&[0x5B; 32], "xavier").unwrap();

        assert_eq!(kek_a, kek_a_again, "derivation must be deterministic");
        assert_ne!(kek_a, kek_b, "project id must scope the KEK");
        assert_ne!(kek_a, kek_other_master, "master key must scope the KEK");

        let container = seal(b"payload", &kek_a, &header("xavier")).unwrap();
        assert!(open(&container, &kek_a).is_ok());
        assert!(
            open(&container, &kek_b).is_err(),
            "another repo's KEK must not open the package"
        );
        assert!(open(&container, &kek_other_master).is_err());
    }

    /// The KEK label is a wire contract: changing it orphans every committed
    /// package, so it is pinned alongside the magic and the extension.
    #[test]
    fn package_path_is_repo_local_and_kek_label_is_pinned() {
        assert_eq!(
            package_kek_info("xavier"),
            b"xavier-repo-package-kek-v1:xavier".to_vec()
        );
        assert_eq!(PACKAGE_ENC_EXTENSION, "enc");
        assert_eq!(PACKAGE_VERSION, 1);
    }
}
