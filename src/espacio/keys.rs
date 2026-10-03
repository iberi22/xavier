//! Per-space key model (WP-13j).
//!
//! Every space owns one random 256-bit data key (DEK, `OsRng`). The DEK never
//! touches disk in clear: `{space_dir}/keystore.json` holds only wrapped
//! copies, each an AES-256-GCM envelope under a different key-encryption key
//! (KEK):
//!
//! | wrapper    | KEK                                                              |
//! |------------|------------------------------------------------------------------|
//! | `node`     | HKDF-SHA256 from the node master key, info `xavier-space-wrap-v1:{space_id}` (via [`NodeKek`]) |
//! | `password` | Argon2id(password, random 16-byte salt), params of `derive_vault_key` (m=64 MiB, t=3, p=1) |
//! | `recovery` | HKDF-SHA256(random 32-byte recovery code, info `xavier-space-recovery-v1:{space_id}`) |
//!
//! Modes: [`UnlockMode::NodeUnlock`] (default) stores `node` + `recovery`
//! (+ `password` when one is set) and unlocks itself at boot.
//! [`UnlockMode::PasswordRequired`] stores `password` + `recovery` only: the
//! space stays locked after a restart until [`KeyRing::unlock_with_password`]
//! or [`KeyRing::unlock_with_recovery`].
//!
//! Envelope: `nonce` (12 random bytes) + `ct` (ciphertext || 16-byte GCM tag),
//! both hex in JSON. Associated data (length-prefixed fields, u32 BE):
//! `"xavier-space-keywrap"`, format version, wrapper kind, space id, mode,
//! `encrypt_records` flag. Changing any of these in the file, or moving a
//! keystore to another space, makes the unwrap fail authentication.
//!
//! Records use `xr1:` + hex(nonce || ct) under the DEK with a fresh random
//! 12-byte nonce each and AD = `"xavier-space-record"`, version, space id,
//! record kind, record id (message seq), so rows cannot be swapped inside a
//! space. Locked spaces answer [`KeysError::Locked`]; there is no plaintext
//! fallback for a space whose `encrypt_records` flag is on.
//!
//! Fail closed: the encryption decision is also persisted outside the
//! keystore (`space.json` `encryption` block, a `meta` row in the space
//! database, and the `xr1:` prefix of stored rows). A space that any of them
//! marks as encrypted but whose keystore is missing answers
//! [`KeysError::KeystoreMissing`]; it never degrades to plaintext, and a
//! stored `xr1:` row is never returned as content.
//!
//! Nothing here logs or formats secrets; secret types redact `Debug`.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{anyhow, Result};
use argon2::{Algorithm, Argon2, Params, Version};
use hkdf::Hkdf;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

use super::store::{validate_space_id, write_atomic, RecordCodec};

/// File name inside a space directory.
pub const KEYSTORE_FILE: &str = "keystore.json";

const FORMAT_VERSION: u32 = 1;
/// Prefix of every encrypted stored record.
pub const RECORD_PREFIX: &str = "xr1:";

/// Whether `stored` has the shape of an encrypted record: prefix plus hex of
/// at least nonce + tag. Such a row is never served as plaintext content.
pub fn looks_encrypted(stored: &str) -> bool {
    stored.strip_prefix(RECORD_PREFIX).is_some_and(|b| {
        b.len() >= 2 * (NONCE_LEN + 16)
            && b.len() % 2 == 0
            && b.bytes().all(|c| c.is_ascii_hexdigit())
    })
}
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const SALT_LEN: usize = 16;
const MIN_PASSWORD_CHARS: usize = 8;

// Same parameters as `derive_vault_key` in `node_identity/vault.rs`.
const ARGON2_M_KIB: u32 = 64 * 1024;
const ARGON2_T: u32 = 3;
const ARGON2_P: u32 = 1;

/// Key errors. Unlock failures are deliberately uniform (wrong password,
/// wrong code and tampered file are indistinguishable to the caller).
#[derive(Debug, thiserror::Error)]
pub enum KeysError {
    /// Space is locked (map to HTTP 423).
    #[error("space {0} is locked")]
    Locked(String),
    #[error("unlock failed")]
    AuthFailed,
    #[error("space {0} has no such unlock method")]
    NoWrapper(String),
    #[error("space {0} has no keystore")]
    NoKeystore(String),
    #[error("keystore already exists for space {0}")]
    AlreadyExists(String),
    /// The space is known to be encrypted but its keystore is gone.
    #[error("space {0} is encrypted but its keystore is missing; refusing plaintext access")]
    KeystoreMissing(String),
    #[error("keystore corrupt: {0}")]
    Corrupt(String),
    #[error("invalid password: {0}")]
    WeakPassword(String),
    #[error("invalid recovery code")]
    BadRecoveryCode,
}

// ---- secret hygiene ----

/// Heap secret wiped on drop (`zeroize`). `Debug` is redacted.
pub struct Secret(Zeroizing<Vec<u8>>);

impl Secret {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(Zeroizing::new(bytes))
    }
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// Handle to an unlocked space data key (cheap to clone, wiped when the last
/// clone drops).
#[derive(Clone)]
pub struct KeyHandle(Arc<Secret>);

impl KeyHandle {
    fn new(dek: Secret) -> Self {
        Self(Arc::new(dek))
    }
    fn bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
    fn same_key(&self, other: &KeyHandle) -> bool {
        self.bytes().ct_eq(other.bytes()).into()
    }
}

/// Space id of the default workspace keystore (`spaces/default/keystore.json`).
pub const DEFAULT_SPACE_ID: &str = "default";

impl KeyHandle {
    /// Seal `plain` as an `xr1:` record string under this key. AD binds the
    /// space id, the record kind and the record id; fresh random nonce each call.
    /// Used by callers that hold a handle but no [`KeyRing`] codec (default
    /// store rows).
    pub fn seal_record(
        &self,
        space_id: &str,
        kind: &str,
        record_id: &str,
        plain: &str,
    ) -> Result<String> {
        let ad = record_ad(space_id, kind, record_id);
        let (nonce, ct) = aead_seal(self.bytes(), plain.as_bytes(), &ad)?;
        let mut raw = nonce.to_vec();
        raw.extend_from_slice(&ct);
        Ok(format!("{RECORD_PREFIX}{}", hex_encode(&raw)))
    }

    /// Inverse of [`KeyHandle::seal_record`]. Anything that is not a valid
    /// `xr1:` record for this exact (space, kind, record id) is an error;
    /// there is no plaintext fallback.
    pub fn open_record(
        &self,
        space_id: &str,
        kind: &str,
        record_id: &str,
        stored: &str,
    ) -> Result<String> {
        let body = stored
            .strip_prefix(RECORD_PREFIX)
            .ok_or_else(|| anyhow!("record is not encrypted (refusing plaintext)"))?;
        let raw = hex_decode(body).ok_or_else(|| anyhow!("record is corrupt"))?;
        if raw.len() < NONCE_LEN {
            return Err(anyhow!("record is corrupt"));
        }
        let (nonce, ct) = raw.split_at(NONCE_LEN);
        let ad = record_ad(space_id, kind, record_id);
        let pt = aead_open(self.bytes(), nonce, ct, &ad)
            .map_err(|_| anyhow!("record failed authentication"))?;
        String::from_utf8(pt.as_bytes().to_vec()).map_err(|_| anyhow!("record is not valid UTF-8"))
    }

    /// Keyed digest (HMAC-SHA256 under a subkey derived from this key) used
    /// for tamper-evident verification without storing plain hashes of
    /// private data. Returns lowercase hex.
    pub fn verify_mac(&self, label: &str, data: &[u8]) -> String {
        let mut sk = Sha256::new();
        sk.update(b"xavier-record-verify-v1");
        sk.update(self.bytes());
        let sub = sk.finalize();
        let mut ipad = [0x36u8; 64];
        let mut opad = [0x5cu8; 64];
        for (i, b) in sub.iter().enumerate() {
            ipad[i] ^= b;
            opad[i] ^= b;
        }
        let mut inner = Sha256::new();
        inner.update(ipad);
        inner.update((label.len() as u32).to_be_bytes());
        inner.update(label.as_bytes());
        inner.update(data);
        let mut outer = Sha256::new();
        outer.update(opad);
        outer.update(inner.finalize());
        hex_encode(&outer.finalize())
    }
}

impl fmt::Debug for KeyHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("KeyHandle(<redacted>)")
    }
}

/// Recovery code, handed to the caller exactly once. Never stored in clear.
/// Format `XRC1-` + 68 hex chars in groups of 8 (32 bytes + 2 check bytes).
pub struct RecoveryCode(Secret);

impl RecoveryCode {
    /// The display string to show the user once.
    pub fn expose(&self) -> &str {
        std::str::from_utf8(self.0.as_bytes()).unwrap_or_default()
    }
}

impl fmt::Debug for RecoveryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RecoveryCode(<redacted>)")
    }
}

// ---- hex ----

fn hex_encode(b: &[u8]) -> String {
    const H: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(b.len() * 2);
    for x in b {
        s.push(H[(x >> 4) as usize] as char);
        s.push(H[(x & 15) as usize] as char);
    }
    s
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.is_ascii() {
        return None;
    }
    s.as_bytes()
        .chunks(2)
        .map(|p| {
            let hi = (p[0] as char).to_digit(16)?;
            let lo = (p[1] as char).to_digit(16)?;
            Some((hi * 16 + lo) as u8)
        })
        .collect()
}

// ---- recovery code encoding ----

fn recovery_check(code: &[u8]) -> [u8; 2] {
    let mut h = Sha256::new();
    h.update(b"xavier-recovery-check-v1");
    h.update(code);
    let d = h.finalize();
    [d[0], d[1]]
}

fn recovery_display(code: &[u8; KEY_LEN]) -> RecoveryCode {
    let mut raw = Zeroizing::new(code.to_vec());
    raw.extend_from_slice(&recovery_check(code));
    let hex = Zeroizing::new(hex_encode(&raw).to_uppercase());
    let mut s = String::from("XRC1");
    for chunk in hex.as_bytes().chunks(8) {
        s.push('-');
        s.push_str(std::str::from_utf8(chunk).unwrap_or_default());
    }
    RecoveryCode(Secret::new(s.into_bytes()))
}

fn recovery_parse(input: &str) -> Result<Secret> {
    let norm: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect::<String>()
        .to_ascii_lowercase();
    let body = norm
        .strip_prefix("xrc1")
        .ok_or(KeysError::BadRecoveryCode)?;
    let raw = hex_decode(body).ok_or(KeysError::BadRecoveryCode)?;
    let raw = Secret::new(raw);
    if raw.as_bytes().len() != KEY_LEN + 2 {
        return Err(KeysError::BadRecoveryCode.into());
    }
    let (code, chk) = raw.as_bytes().split_at(KEY_LEN);
    if recovery_check(code) != [chk[0], chk[1]] {
        return Err(KeysError::BadRecoveryCode.into());
    }
    Ok(Secret::new(code.to_vec()))
}

// ---- node KEK provider ----

/// Source of the node-bound KEK. Injected so tests never need the real
/// master key (no global state).
pub trait NodeKek: Send + Sync {
    /// 32-byte KEK for `space_id`.
    fn space_kek(&self, space_id: &str) -> Result<Secret>;
}

fn node_info(space_id: &str) -> Vec<u8> {
    format!("xavier-space-wrap-v1:{space_id}").into_bytes()
}

/// Production provider: HKDF from the node master key.
pub struct MasterNodeKek(crate::keystore::MasterKeyManager);

impl MasterNodeKek {
    /// Loads (or initializes) the node master key. Touches the OS keyring /
    /// the master-key fallback file: never call from tests.
    pub fn load_or_init() -> Result<Self> {
        Ok(Self(crate::keystore::MasterKeyManager::load_or_init()?))
    }
}

impl NodeKek for MasterNodeKek {
    fn space_kek(&self, space_id: &str) -> Result<Secret> {
        let mut out = vec![0u8; KEY_LEN];
        self.0.derive_key(&node_info(space_id), &mut out)?;
        Ok(Secret::new(out))
    }
}

/// Provider over an injected 32-byte master key, same HKDF derivation as
/// [`MasterNodeKek`]. For tests and tools.
pub struct StaticNodeKek(Secret);

impl StaticNodeKek {
    pub fn new(master: [u8; KEY_LEN]) -> Self {
        Self(Secret::new(master.to_vec()))
    }
}

impl NodeKek for StaticNodeKek {
    fn space_kek(&self, space_id: &str) -> Result<Secret> {
        let hk = Hkdf::<Sha256>::new(None, self.0.as_bytes());
        let mut out = vec![0u8; KEY_LEN];
        hk.expand(&node_info(space_id), &mut out)
            .map_err(|e| anyhow!("HKDF expansion failed: {e}"))?;
        Ok(Secret::new(out))
    }
}

// ---- keystore file ----

/// How a space unlocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnlockMode {
    NodeUnlock,
    PasswordRequired,
}

impl UnlockMode {
    fn as_str(self) -> &'static str {
        match self {
            UnlockMode::NodeUnlock => "node_unlock",
            UnlockMode::PasswordRequired => "password_required",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum WrapperKind {
    Node,
    Password,
    Recovery,
}

impl WrapperKind {
    fn as_str(self) -> &'static str {
        match self {
            WrapperKind::Node => "node",
            WrapperKind::Password => "password",
            WrapperKind::Recovery => "recovery",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WrapperRecord {
    kind: WrapperKind,
    /// Argon2id salt (password wrapper only), hex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    salt: Option<String>,
    nonce: String,
    ct: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct KeystoreFile {
    version: u32,
    space_id: String,
    mode: UnlockMode,
    encrypt_records: bool,
    wrappers: Vec<WrapperRecord>,
}

fn push_field(out: &mut Vec<u8>, f: &[u8]) {
    out.extend_from_slice(&(f.len() as u32).to_be_bytes());
    out.extend_from_slice(f);
}

fn wrap_ad(file: &KeystoreFile, kind: WrapperKind) -> Vec<u8> {
    wrap_ad_parts(&file.space_id, kind, file.mode, file.encrypt_records)
}

fn wrap_ad_parts(space_id: &str, kind: WrapperKind, mode: UnlockMode, encrypt: bool) -> Vec<u8> {
    let mut ad = Vec::new();
    push_field(&mut ad, b"xavier-space-keywrap");
    push_field(&mut ad, &FORMAT_VERSION.to_be_bytes());
    push_field(&mut ad, kind.as_str().as_bytes());
    push_field(&mut ad, space_id.as_bytes());
    push_field(&mut ad, mode.as_str().as_bytes());
    push_field(&mut ad, &[encrypt as u8]);
    ad
}

fn record_ad(space_id: &str, kind: &str, record_id: &str) -> Vec<u8> {
    let mut ad = Vec::new();
    push_field(&mut ad, b"xavier-space-record");
    push_field(&mut ad, &FORMAT_VERSION.to_be_bytes());
    push_field(&mut ad, space_id.as_bytes());
    push_field(&mut ad, kind.as_bytes());
    push_field(&mut ad, record_id.as_bytes());
    ad
}

fn aead_seal(key: &[u8], plain: &[u8], ad: &[u8]) -> Result<([u8; NONCE_LEN], Vec<u8>)> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| anyhow!("AES key init: {e}"))?;
    let mut nonce = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plain,
                aad: ad,
            },
        )
        .map_err(|_| anyhow!("AES-GCM encrypt failed"))?;
    Ok((nonce, ct))
}

fn aead_open(key: &[u8], nonce: &[u8], ct: &[u8], ad: &[u8]) -> Result<Secret> {
    if nonce.len() != NONCE_LEN {
        return Err(KeysError::AuthFailed.into());
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| anyhow!("AES key init: {e}"))?;
    let pt = cipher
        .decrypt(Nonce::from_slice(nonce), Payload { msg: ct, aad: ad })
        .map_err(|_| KeysError::AuthFailed)?;
    Ok(Secret::new(pt))
}

fn password_kek(password: &str, salt: &[u8]) -> Result<Secret> {
    let params = Params::new(ARGON2_M_KIB, ARGON2_T, ARGON2_P, Some(KEY_LEN))
        .map_err(|e| anyhow!("argon2 params: {e}"))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out = vec![0u8; KEY_LEN];
    argon
        .hash_password_into(password.as_bytes(), salt, &mut out)
        .map_err(|e| anyhow!("argon2 hash: {e}"))?;
    Ok(Secret::new(out))
}

fn recovery_kek(code: &[u8], space_id: &str) -> Result<Secret> {
    let hk = Hkdf::<Sha256>::new(None, code);
    let mut out = vec![0u8; KEY_LEN];
    hk.expand(
        format!("xavier-space-recovery-v1:{space_id}").as_bytes(),
        &mut out,
    )
    .map_err(|e| anyhow!("HKDF expansion failed: {e}"))?;
    Ok(Secret::new(out))
}

fn check_password(pw: &str) -> Result<()> {
    if pw.chars().count() < MIN_PASSWORD_CHARS {
        return Err(KeysError::WeakPassword(format!(
            "at least {MIN_PASSWORD_CHARS} characters required"
        ))
        .into());
    }
    Ok(())
}

fn make_wrapper(
    file_meta: (&str, UnlockMode, bool),
    kind: WrapperKind,
    kek: &Secret,
    salt: Option<&[u8]>,
    dek: &Secret,
) -> Result<WrapperRecord> {
    let ad = wrap_ad_parts(file_meta.0, kind, file_meta.1, file_meta.2);
    let (nonce, ct) = aead_seal(kek.as_bytes(), dek.as_bytes(), &ad)?;
    Ok(WrapperRecord {
        kind,
        salt: salt.map(hex_encode),
        nonce: hex_encode(&nonce),
        ct: hex_encode(&ct),
    })
}

fn open_wrapper(file: &KeystoreFile, w: &WrapperRecord, kek: &Secret) -> Result<KeyHandle> {
    let nonce = hex_decode(&w.nonce).ok_or(KeysError::AuthFailed)?;
    let ct = hex_decode(&w.ct).ok_or(KeysError::AuthFailed)?;
    let dek = aead_open(kek.as_bytes(), &nonce, &ct, &wrap_ad(file, w.kind))?;
    if dek.as_bytes().len() != KEY_LEN {
        return Err(KeysError::AuthFailed.into());
    }
    Ok(KeyHandle::new(dek))
}

fn read_keystore(dir: &Path, space_id: &str) -> Result<Option<KeystoreFile>> {
    let path = dir.join(KEYSTORE_FILE);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(KeysError::Corrupt(e.to_string()).into()),
    };
    let file: KeystoreFile =
        serde_json::from_slice(&bytes).map_err(|e| KeysError::Corrupt(e.to_string()))?;
    if file.version != FORMAT_VERSION {
        return Err(KeysError::Corrupt(format!("unsupported version {}", file.version)).into());
    }
    if file.space_id != space_id {
        return Err(KeysError::Corrupt("space id mismatch".into()).into());
    }
    Ok(Some(file))
}

fn write_keystore(dir: &Path, file: &KeystoreFile) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(file)?;
    write_atomic(&dir.join(KEYSTORE_FILE), &bytes)
}

// ---- key ring ----

struct SpaceKeys {
    encrypt_records: bool,
    dek: Option<KeyHandle>,
}

/// What a codec must do for one space right now.
enum RecordKey {
    /// Legacy plaintext space (no keystore and nothing says it was ever
    /// encrypted), or `encrypt_records = false`: stored as given.
    Plain,
    Key(KeyHandle),
}

/// Key state of all spaces under one `spaces/` directory.
pub struct KeyRing {
    spaces_dir: PathBuf,
    node: Arc<dyn NodeKek>,
    state: Mutex<HashMap<String, SpaceKeys>>,
    /// Spaces known to be encrypted from evidence outside the keystore
    /// (descriptor flag, database meta row, `xr1:` rows). Without a keystore
    /// they are unavailable, never plaintext.
    required: Mutex<HashSet<String>>,
}

impl fmt::Debug for KeyRing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeyRing")
            .field("spaces_dir", &self.spaces_dir)
            .finish_non_exhaustive()
    }
}

impl KeyRing {
    pub fn new(spaces_dir: impl Into<PathBuf>, node: Arc<dyn NodeKek>) -> Arc<Self> {
        Arc::new(Self {
            spaces_dir: spaces_dir.into(),
            node,
            state: Mutex::new(HashMap::new()),
            required: Mutex::new(HashSet::new()),
        })
    }

    /// Record that `space_id` is encrypted according to evidence outside the
    /// keystore. Sticky for the life of the ring.
    pub fn require_encryption(&self, space_id: &str) {
        self.required
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(space_id.to_string());
    }

    fn is_required(&self, space_id: &str) -> bool {
        self.required
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains(space_id)
    }

    /// A keystore without a `space.json` next to it comes from a crashed
    /// create. Move it to `keystore.json.orphan-<ts>` (never delete) so a new
    /// keystore can be written. Returns the new path when something moved.
    pub fn quarantine_orphan_keystore(&self, space_id: &str) -> Result<Option<PathBuf>> {
        let dir = self.dir(space_id)?;
        let ks = dir.join(KEYSTORE_FILE);
        if !ks.exists() || dir.join(super::store::DESCRIPTOR_FILE).exists() {
            return Ok(None);
        }
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dest = dir.join(format!("{KEYSTORE_FILE}.orphan-{ts}"));
        std::fs::rename(&ks, &dest)?;
        self.forget(space_id);
        tracing::warn!("espacio: orphan keystore of space {space_id} moved aside (not deleted)");
        Ok(Some(dest))
    }

    fn state(&self) -> MutexGuard<'_, HashMap<String, SpaceKeys>> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn dir(&self, space_id: &str) -> Result<PathBuf> {
        validate_space_id(space_id)?;
        Ok(self.spaces_dir.join(space_id))
    }

    /// Create the keystore of a new space with default options
    /// (`encrypt_records = true`). The caller must display the recovery code
    /// once; it cannot be recovered later.
    pub fn create_keys(
        &self,
        space_id: &str,
        mode: UnlockMode,
        password: Option<&str>,
    ) -> Result<(KeyHandle, RecoveryCode)> {
        self.create_keys_with(space_id, mode, password, true)
    }

    /// [`KeyRing::create_keys`] with an explicit `encrypt_records` flag.
    /// Refuses to overwrite an existing keystore.
    pub fn create_keys_with(
        &self,
        space_id: &str,
        mode: UnlockMode,
        password: Option<&str>,
        encrypt_records: bool,
    ) -> Result<(KeyHandle, RecoveryCode)> {
        let dir = self.dir(space_id)?;
        match (mode, password) {
            (UnlockMode::PasswordRequired, None) => {
                return Err(KeysError::WeakPassword(
                    "password_required mode needs a password".into(),
                )
                .into())
            }
            (_, Some(pw)) => check_password(pw)?,
            _ => {}
        }
        if dir.join(KEYSTORE_FILE).exists() {
            return Err(KeysError::AlreadyExists(space_id.to_string()).into());
        }
        std::fs::create_dir_all(&dir)?;

        let mut dek_bytes = vec![0u8; KEY_LEN];
        OsRng.fill_bytes(&mut dek_bytes);
        let dek = Secret::new(dek_bytes);
        let mut code = [0u8; KEY_LEN];
        OsRng.fill_bytes(&mut code);

        let built = (|| -> Result<Vec<WrapperRecord>> {
            let meta = (space_id, mode, encrypt_records);
            let mut wrappers = Vec::new();
            if mode == UnlockMode::NodeUnlock {
                let kek = self.node.space_kek(space_id)?;
                wrappers.push(make_wrapper(meta, WrapperKind::Node, &kek, None, &dek)?);
            }
            if let Some(pw) = password {
                wrappers.push(self.password_wrapper(meta, pw, &dek)?);
            }
            let kek = recovery_kek(&code, space_id)?;
            wrappers.push(make_wrapper(meta, WrapperKind::Recovery, &kek, None, &dek)?);
            Ok(wrappers)
        })();
        let display = recovery_display(&code);
        code.zeroize();
        let wrappers = built?;

        write_keystore(
            &dir,
            &KeystoreFile {
                version: FORMAT_VERSION,
                space_id: space_id.to_string(),
                mode,
                encrypt_records,
                wrappers,
            },
        )?;
        let handle = KeyHandle::new(dek);
        self.state().insert(
            space_id.to_string(),
            SpaceKeys {
                encrypt_records,
                dek: Some(handle.clone()),
            },
        );
        Ok((handle, display))
    }

    fn password_wrapper(
        &self,
        meta: (&str, UnlockMode, bool),
        password: &str,
        dek: &Secret,
    ) -> Result<WrapperRecord> {
        let mut salt = [0u8; SALT_LEN];
        OsRng.fill_bytes(&mut salt);
        let kek = password_kek(password, &salt)?;
        make_wrapper(meta, WrapperKind::Password, &kek, Some(&salt), dek)
    }

    /// Load the keystore state for a space (if not yet cached). Returns
    /// whether the space has a keystore.
    fn ensure_loaded(&self, space_id: &str) -> Result<bool> {
        if self.state().contains_key(space_id) {
            return Ok(true);
        }
        let dir = self.dir(space_id)?;
        match read_keystore(&dir, space_id)? {
            None if self.is_required(space_id) => {
                Err(KeysError::KeystoreMissing(space_id.to_string()).into())
            }
            None => Ok(false),
            Some(f) => {
                self.state()
                    .entry(space_id.to_string())
                    .or_insert(SpaceKeys {
                        encrypt_records: f.encrypt_records,
                        dek: None,
                    });
                Ok(true)
            }
        }
    }

    fn unlock_with<F>(&self, space_id: &str, kind: WrapperKind, kek_for: F) -> Result<KeyHandle>
    where
        F: FnOnce(&WrapperRecord) -> Result<Secret>,
    {
        let dir = self.dir(space_id)?;
        let file = read_keystore(&dir, space_id)?
            .ok_or_else(|| KeysError::NoKeystore(space_id.to_string()))?;
        let w = file
            .wrappers
            .iter()
            .find(|w| w.kind == kind)
            .ok_or_else(|| KeysError::NoWrapper(space_id.to_string()))?;
        let kek = kek_for(w)?;
        let handle = open_wrapper(&file, w, &kek)?;
        self.state().insert(
            space_id.to_string(),
            SpaceKeys {
                encrypt_records: file.encrypt_records,
                dek: Some(handle.clone()),
            },
        );
        Ok(handle)
    }

    /// Unlock with the node wrapper (`node_unlock` spaces only).
    pub fn unlock_with_node(&self, space_id: &str) -> Result<KeyHandle> {
        self.unlock_with(space_id, WrapperKind::Node, |_| {
            self.node.space_kek(space_id)
        })
    }

    pub fn unlock_with_password(&self, space_id: &str, password: &str) -> Result<KeyHandle> {
        self.unlock_with(space_id, WrapperKind::Password, |w| {
            let salt = w
                .salt
                .as_deref()
                .and_then(hex_decode)
                .ok_or(KeysError::AuthFailed)?;
            if salt.len() != SALT_LEN {
                return Err(KeysError::AuthFailed.into());
            }
            password_kek(password, &salt)
        })
    }

    pub fn unlock_with_recovery(&self, space_id: &str, code: &str) -> Result<KeyHandle> {
        let code = recovery_parse(code)?;
        self.unlock_with(space_id, WrapperKind::Recovery, |_| {
            recovery_kek(code.as_bytes(), space_id)
        })
    }

    /// Replace (or add, in `node_unlock` spaces) the password wrapper. Only
    /// the DEK is re-wrapped, records are untouched. `handle` must be the
    /// currently unlocked key (from any unlock path), which is how a recovery
    /// code resets a forgotten password.
    pub fn change_password(
        &self,
        space_id: &str,
        handle: &KeyHandle,
        new_password: &str,
    ) -> Result<()> {
        check_password(new_password)?;
        let dir = self.dir(space_id)?;
        let current = self.current_dek(space_id)?;
        if !current.same_key(handle) {
            return Err(KeysError::AuthFailed.into());
        }
        let mut file = read_keystore(&dir, space_id)?
            .ok_or_else(|| KeysError::NoKeystore(space_id.to_string()))?;
        let meta = (space_id, file.mode, file.encrypt_records);
        let dek = Secret::new(current.bytes().to_vec());
        let w = self.password_wrapper(meta, new_password, &dek)?;
        match file
            .wrappers
            .iter_mut()
            .find(|w| w.kind == WrapperKind::Password)
        {
            Some(slot) => *slot = w,
            None => file.wrappers.push(w),
        }
        write_keystore(&dir, &file)
    }

    fn current_dek(&self, space_id: &str) -> Result<KeyHandle> {
        if !self.ensure_loaded(space_id)? {
            return Err(KeysError::NoKeystore(space_id.to_string()).into());
        }
        self.state()
            .get(space_id)
            .and_then(|s| s.dek.clone())
            .ok_or_else(|| KeysError::Locked(space_id.to_string()).into())
    }

    /// Whether the space has a keystore (spaces without one are legacy
    /// plaintext spaces).
    pub fn has_keys(&self, space_id: &str) -> bool {
        self.ensure_loaded(space_id).unwrap_or(true)
    }

    /// Fail-closed: a space with a keystore that is unreadable counts as
    /// locked. Spaces without a keystore are not locked.
    pub fn is_locked(&self, space_id: &str) -> bool {
        match self.ensure_loaded(space_id) {
            Ok(false) => false,
            Ok(true) => self
                .state()
                .get(space_id)
                .map(|s| s.dek.is_none())
                .unwrap_or(true),
            Err(_) => true,
        }
    }

    /// `Err(Locked)` when the space is locked, `Err(KeystoreMissing)` when it
    /// is known to be encrypted but has no keystore.
    pub fn check_unlocked(&self, space_id: &str) -> Result<()> {
        if let Err(e) = self.ensure_loaded(space_id) {
            if matches!(
                e.downcast_ref::<KeysError>(),
                Some(KeysError::KeystoreMissing(_))
            ) {
                return Err(e);
            }
        }
        if self.is_locked(space_id) {
            return Err(KeysError::Locked(space_id.to_string()).into());
        }
        Ok(())
    }

    /// Forget the in-memory key (e.g. after deleting the space).
    pub fn forget(&self, space_id: &str) {
        self.state().remove(space_id);
    }

    /// Remove a keystore created by a failed space creation.
    pub fn discard_keys(&self, space_id: &str) {
        self.forget(space_id);
        if let Ok(dir) = self.dir(space_id) {
            let _ = std::fs::remove_file(dir.join(KEYSTORE_FILE));
        }
    }

    /// Unlock every `node_unlock` space found under the spaces directory.
    /// Failures are logged without secrets and leave the space locked.
    pub fn auto_unlock_all(&self) {
        let Ok(rd) = std::fs::read_dir(&self.spaces_dir) else {
            return;
        };
        for e in rd.flatten() {
            let id = e.file_name().to_string_lossy().to_string();
            if validate_space_id(&id).is_err() || !e.path().join(KEYSTORE_FILE).is_file() {
                continue;
            }
            let mode = read_keystore(&e.path(), &id).map(|f| f.map(|f| f.mode));
            match mode {
                Ok(Some(UnlockMode::NodeUnlock)) => {
                    if self.unlock_with_node(&id).is_err() {
                        tracing::error!("espacio: space {id} could not be unlocked with the node key; left locked");
                    }
                }
                Ok(Some(UnlockMode::PasswordRequired)) => {
                    tracing::info!("espacio: space {id} requires a password; locked");
                }
                Ok(None) => {}
                Err(_) => {
                    tracing::error!("espacio: keystore of space {id} unreadable; left locked");
                }
            }
        }
    }

    fn record_key(&self, space_id: &str) -> Result<RecordKey> {
        if !self.ensure_loaded(space_id)? {
            return Ok(RecordKey::Plain);
        }
        let st = self.state();
        let s = st
            .get(space_id)
            .ok_or_else(|| KeysError::Locked(space_id.to_string()))?;
        if !s.encrypt_records {
            if self.is_required(space_id) {
                // Evidence says encrypted, the keystore says plain.
                return Err(KeysError::Corrupt(
                    "encryption flag contradicts space metadata".into(),
                )
                .into());
            }
            return Ok(RecordKey::Plain);
        }
        match &s.dek {
            Some(h) => Ok(RecordKey::Key(h.clone())),
            None => Err(KeysError::Locked(space_id.to_string()).into()),
        }
    }

    /// Record codec for one record kind of one space.
    pub fn codec(self: &Arc<Self>, space_id: &str, kind: &'static str) -> Arc<dyn RecordCodec> {
        Arc::new(KeyedCodec {
            ring: self.clone(),
            space_id: space_id.to_string(),
            kind,
        })
    }
}

// ---- record codec ----

/// AES-256-GCM record codec bound to a space and a record kind. The key is
/// looked up on every call, so a locked space never serves or stores
/// plaintext.
pub struct KeyedCodec {
    ring: Arc<KeyRing>,
    space_id: String,
    kind: &'static str,
}

impl fmt::Debug for KeyedCodec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KeyedCodec")
            .field("space_id", &self.space_id)
            .field("kind", &self.kind)
            .finish()
    }
}

impl RecordCodec for KeyedCodec {
    fn seal(&self, plain: &str, record_id: &str) -> Result<String> {
        match self.ring.record_key(&self.space_id)? {
            RecordKey::Plain => Ok(plain.to_string()),
            RecordKey::Key(k) => {
                let ad = record_ad(&self.space_id, self.kind, record_id);
                let (nonce, ct) = aead_seal(k.bytes(), plain.as_bytes(), &ad)?;
                let mut raw = nonce.to_vec();
                raw.extend_from_slice(&ct);
                Ok(format!("{RECORD_PREFIX}{}", hex_encode(&raw)))
            }
        }
    }

    fn open(&self, stored: &str, record_id: &str) -> Result<String> {
        match self.ring.record_key(&self.space_id)? {
            RecordKey::Plain => {
                if looks_encrypted(stored) {
                    // Ciphertext is never content.
                    return Err(KeysError::KeystoreMissing(self.space_id.clone()).into());
                }
                Ok(stored.to_string())
            }
            RecordKey::Key(k) => {
                let body = stored
                    .strip_prefix(RECORD_PREFIX)
                    .ok_or_else(|| anyhow!("record is not encrypted (refusing plaintext)"))?;
                let raw = hex_decode(body).ok_or_else(|| anyhow!("record is corrupt"))?;
                if raw.len() < NONCE_LEN {
                    return Err(anyhow!("record is corrupt"));
                }
                let (nonce, ct) = raw.split_at(NONCE_LEN);
                let ad = record_ad(&self.space_id, self.kind, record_id);
                let pt = aead_open(k.bytes(), nonce, ct, &ad)
                    .map_err(|_| anyhow!("record failed authentication"))?;
                String::from_utf8(pt.as_bytes().to_vec())
                    .map_err(|_| anyhow!("record is not valid UTF-8"))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    const PW: &str = "correct horse battery";

    fn ring(dir: &Path) -> Arc<KeyRing> {
        KeyRing::new(dir, Arc::new(StaticNodeKek::new([7u8; 32])))
    }

    fn ks_path(dir: &Path, id: &str) -> PathBuf {
        dir.join(id).join(KEYSTORE_FILE)
    }

    fn read_json(dir: &Path, id: &str) -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(ks_path(dir, id)).unwrap()).unwrap()
    }

    fn write_json(dir: &Path, id: &str, v: &serde_json::Value) {
        std::fs::write(ks_path(dir, id), serde_json::to_vec(v).unwrap()).unwrap();
    }

    fn is_auth_failed(e: &anyhow::Error) -> bool {
        matches!(e.downcast_ref::<KeysError>(), Some(KeysError::AuthFailed))
    }

    #[test]
    fn node_unlock_roundtrip_and_reopen() {
        let tmp = tempfile::tempdir().unwrap();
        let r = ring(tmp.path());
        let (h, _code) = r.create_keys("s1", UnlockMode::NodeUnlock, None).unwrap();
        let r2 = ring(tmp.path());
        assert!(r2.is_locked("s1"));
        let h2 = r2.unlock_with_node("s1").unwrap();
        assert!(h.same_key(&h2));
        assert!(!r2.is_locked("s1"));
    }

    #[test]
    fn wrong_password_fails_right_one_opens() {
        let tmp = tempfile::tempdir().unwrap();
        let r = ring(tmp.path());
        let (h, _) = r
            .create_keys("s1", UnlockMode::PasswordRequired, Some(PW))
            .unwrap();
        let r2 = ring(tmp.path());
        let err = r2
            .unlock_with_password("s1", "not the password")
            .unwrap_err();
        assert!(is_auth_failed(&err));
        assert!(r2.is_locked("s1"));
        let h2 = r2.unlock_with_password("s1", PW).unwrap();
        assert!(h.same_key(&h2));
        // password_required has no node wrapper
        let err = r2.unlock_with_node("s1").unwrap_err();
        assert!(matches!(
            err.downcast_ref::<KeysError>(),
            Some(KeysError::NoWrapper(_))
        ));
    }

    #[test]
    fn password_required_needs_password_and_min_length() {
        let tmp = tempfile::tempdir().unwrap();
        let r = ring(tmp.path());
        assert!(r
            .create_keys("s1", UnlockMode::PasswordRequired, None)
            .is_err());
        assert!(r
            .create_keys("s1", UnlockMode::NodeUnlock, Some("short"))
            .is_err());
        assert!(!ks_path(tmp.path(), "s1").exists());
    }

    #[test]
    fn recovery_code_opens_and_resets_password() {
        let tmp = tempfile::tempdir().unwrap();
        let r = ring(tmp.path());
        let (h, code) = r
            .create_keys("s1", UnlockMode::PasswordRequired, Some(PW))
            .unwrap();
        let code = code.expose().to_string();
        assert!(code.starts_with("XRC1-"));

        let r2 = ring(tmp.path());
        // lowercase and spacing tolerated
        let h2 = r2
            .unlock_with_recovery("s1", &code.to_lowercase().replace('-', " "))
            .unwrap();
        assert!(h.same_key(&h2));
        r2.change_password("s1", &h2, "a brand new password")
            .unwrap();

        let r3 = ring(tmp.path());
        assert!(is_auth_failed(
            &r3.unlock_with_password("s1", PW).unwrap_err()
        ));
        let h3 = r3
            .unlock_with_password("s1", "a brand new password")
            .unwrap();
        assert!(h.same_key(&h3));
        // the code still works after a reset
        assert!(ring(tmp.path()).unlock_with_recovery("s1", &code).is_ok());
    }

    #[test]
    fn bad_recovery_codes_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let r = ring(tmp.path());
        let (_, code) = r.create_keys("s1", UnlockMode::NodeUnlock, None).unwrap();
        let good = code.expose().to_string();
        let r2 = ring(tmp.path());
        assert!(r2.unlock_with_recovery("s1", "").is_err());
        assert!(r2.unlock_with_recovery("s1", "XRC1-00").is_err());
        // flip one hex digit: checksum rejects it
        let mut bad = good.clone().into_bytes();
        let i = 6;
        bad[i] = if bad[i] == b'0' { b'1' } else { b'0' };
        let bad = String::from_utf8(bad).unwrap();
        assert!(matches!(
            r2.unlock_with_recovery("s1", &bad)
                .unwrap_err()
                .downcast_ref::<KeysError>(),
            Some(KeysError::BadRecoveryCode)
        ));
        // a well-formed code for another key fails authentication
        let (_, other) = ring(tmp.path())
            .create_keys("s2", UnlockMode::NodeUnlock, None)
            .unwrap();
        assert!(is_auth_failed(
            &r2.unlock_with_recovery("s1", other.expose()).unwrap_err()
        ));
        assert!(r2.is_locked("s1"));
    }

    fn flip_hex(s: &str) -> String {
        let mut b = s.as_bytes().to_vec();
        b[0] = if b[0] == b'0' { b'1' } else { b'0' };
        String::from_utf8(b).unwrap()
    }

    #[test]
    fn tampered_wrapper_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, code) = ring(tmp.path())
            .create_keys("s1", UnlockMode::NodeUnlock, Some(PW))
            .unwrap();
        let code = code.expose().to_string();
        let original = read_json(tmp.path(), "s1");

        // (field path, mutation) pairs on the node wrapper and the others
        for idx in 0..3 {
            for field in ["ct", "nonce"] {
                let mut v = original.clone();
                let cur = v["wrappers"][idx][field].as_str().unwrap().to_string();
                v["wrappers"][idx][field] = flip_hex(&cur).into();
                write_json(tmp.path(), "s1", &v);
                let r = ring(tmp.path());
                let res = match v["wrappers"][idx]["kind"].as_str().unwrap() {
                    "node" => r.unlock_with_node("s1"),
                    "password" => r.unlock_with_password("s1", PW),
                    _ => r.unlock_with_recovery("s1", &code),
                };
                assert!(res.is_err(), "tamper {field} of wrapper {idx} accepted");
                assert!(r.is_locked("s1"));
            }
        }

        // associated data: the encrypt_records flag and the mode are bound
        let mut v = original.clone();
        v["encrypt_records"] = false.into();
        write_json(tmp.path(), "s1", &v);
        assert!(ring(tmp.path()).unlock_with_node("s1").is_err());
        assert!(ring(tmp.path()).unlock_with_recovery("s1", &code).is_err());

        let mut v = original.clone();
        v["mode"] = "password_required".into();
        write_json(tmp.path(), "s1", &v);
        assert!(ring(tmp.path()).unlock_with_password("s1", PW).is_err());
        assert!(ring(tmp.path()).unlock_with_recovery("s1", &code).is_err());

        // restored file works again
        write_json(tmp.path(), "s1", &original);
        assert!(ring(tmp.path()).unlock_with_node("s1").is_ok());
    }

    #[test]
    fn keystore_moved_to_other_space_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        ring(tmp.path())
            .create_keys("s1", UnlockMode::NodeUnlock, None)
            .unwrap();
        std::fs::create_dir_all(tmp.path().join("s2")).unwrap();
        std::fs::copy(ks_path(tmp.path(), "s1"), ks_path(tmp.path(), "s2")).unwrap();
        let r = ring(tmp.path());
        assert!(r.unlock_with_node("s2").is_err());
        assert!(r.is_locked("s2"));

        // even with the space id patched, the AD (space id) fails the tag
        let mut v = read_json(tmp.path(), "s2");
        v["space_id"] = "s2".into();
        write_json(tmp.path(), "s2", &v);
        assert!(is_auth_failed(
            &ring(tmp.path()).unlock_with_node("s2").unwrap_err()
        ));
    }

    #[test]
    fn wrong_node_key_cannot_unlock() {
        let tmp = tempfile::tempdir().unwrap();
        ring(tmp.path())
            .create_keys("s1", UnlockMode::NodeUnlock, None)
            .unwrap();
        let other = KeyRing::new(tmp.path(), Arc::new(StaticNodeKek::new([8u8; 32])));
        assert!(is_auth_failed(&other.unlock_with_node("s1").unwrap_err()));
    }

    #[test]
    fn wrapper_nonces_unique_and_salts_random() {
        let tmp = tempfile::tempdir().unwrap();
        let r = ring(tmp.path());
        let mut nonces = HashSet::new();
        let mut salts = HashSet::new();
        for i in 0..6 {
            let id = format!("s{i}");
            r.create_keys(&id, UnlockMode::NodeUnlock, Some(PW))
                .unwrap();
            let v = read_json(tmp.path(), &id);
            for w in v["wrappers"].as_array().unwrap() {
                assert!(nonces.insert(w["nonce"].as_str().unwrap().to_string()));
                if let Some(s) = w["salt"].as_str() {
                    assert!(salts.insert(s.to_string()));
                }
            }
        }
        assert_eq!(nonces.len(), 18);
        assert_eq!(salts.len(), 6);
    }

    #[test]
    fn record_codec_roundtrip_unique_nonces_and_binding() {
        let tmp = tempfile::tempdir().unwrap();
        let r = ring(tmp.path());
        r.create_keys("s1", UnlockMode::NodeUnlock, None).unwrap();
        r.create_keys("s2", UnlockMode::NodeUnlock, None).unwrap();
        let c1 = r.codec("s1", "message");
        let mut nonces = HashSet::new();
        for _ in 0..200 {
            let s = c1.seal("hello world", "7").unwrap();
            assert!(s.starts_with("xr1:"));
            assert!(!s.contains("hello"));
            assert!(nonces.insert(s[4..28].to_string()), "nonce reused");
            assert_eq!(c1.open(&s, "7").unwrap(), "hello world");
        }
        // bound to space and kind
        let sealed = c1.seal("secret", "1").unwrap();
        assert!(r.codec("s2", "message").open(&sealed, "1").is_err());
        assert!(r.codec("s1", "other").open(&sealed, "1").is_err());
        // bound to the record id
        assert!(c1.open(&sealed, "2").is_err());
        // tamper
        let mut t = sealed.clone().into_bytes();
        let n = t.len() - 1;
        t[n] = if t[n] == b'0' { b'1' } else { b'0' };
        assert!(c1.open(&String::from_utf8(t).unwrap(), "1").is_err());
        // no plaintext fallback while encrypting
        assert!(c1.open("plain text row", "1").is_err());
    }

    #[test]
    fn locked_codec_refuses_both_directions() {
        let tmp = tempfile::tempdir().unwrap();
        let r = ring(tmp.path());
        r.create_keys("s1", UnlockMode::PasswordRequired, Some(PW))
            .unwrap();
        let sealed = r.codec("s1", "message").seal("x", "0").unwrap();
        let r2 = ring(tmp.path());
        let c = r2.codec("s1", "message");
        for res in [c.seal("x", "0"), c.open(&sealed, "0")] {
            let e = res.unwrap_err();
            assert!(matches!(
                e.downcast_ref::<KeysError>(),
                Some(KeysError::Locked(_))
            ));
        }
        assert!(r2.check_unlocked("s1").is_err());
        r2.unlock_with_password("s1", PW).unwrap();
        assert_eq!(c.open(&sealed, "0").unwrap(), "x");
    }

    #[test]
    fn encrypt_records_false_is_passthrough() {
        let tmp = tempfile::tempdir().unwrap();
        let r = ring(tmp.path());
        r.create_keys_with("s1", UnlockMode::NodeUnlock, None, false)
            .unwrap();
        let c = r.codec("s1", "message");
        assert_eq!(c.seal("abc", "0").unwrap(), "abc");
        assert_eq!(c.open("abc", "0").unwrap(), "abc");
        // ciphertext is never served as plaintext content
        assert!(c.open(&format!("xr1:{}", "ab".repeat(40)), "0").is_err());
    }

    #[test]
    fn keystore_contains_no_secret_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let r = ring(tmp.path());
        let (h, code) = r
            .create_keys("s1", UnlockMode::NodeUnlock, Some(PW))
            .unwrap();
        let raw = std::fs::read(ks_path(tmp.path(), "s1")).unwrap();
        let text = String::from_utf8(raw.clone()).unwrap();
        let contains = |hay: &[u8], needle: &[u8]| hay.windows(needle.len()).any(|w| w == needle);
        assert!(!contains(&raw, h.bytes()));
        assert!(!text.contains(&hex_encode(h.bytes())));
        assert!(!text.contains(PW));
        let code_str = code.expose();
        let compact = code_str.replace('-', "");
        assert!(!text.to_uppercase().contains(&compact[4..]));
        assert!(!text.contains(code_str));
        let code_raw = recovery_parse(code_str).unwrap();
        assert!(!text.contains(&hex_encode(code_raw.as_bytes())));
        // only expected keys
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        assert_eq!(
            keys.iter().map(String::as_str).collect::<HashSet<_>>(),
            HashSet::from(["version", "space_id", "mode", "encrypt_records", "wrappers"])
        );
    }

    #[test]
    fn debug_never_prints_secrets() {
        let tmp = tempfile::tempdir().unwrap();
        let r = ring(tmp.path());
        let (h, code) = r.create_keys("s1", UnlockMode::NodeUnlock, None).unwrap();
        for s in [format!("{h:?}"), format!("{code:?}"), format!("{r:?}")] {
            assert!(!s.contains(&hex_encode(h.bytes())));
            assert!(!s.contains(code.expose()));
        }
    }

    #[test]
    fn create_keys_refuses_overwrite() {
        let tmp = tempfile::tempdir().unwrap();
        let r = ring(tmp.path());
        r.create_keys("s1", UnlockMode::NodeUnlock, None).unwrap();
        let e = r
            .create_keys("s1", UnlockMode::NodeUnlock, None)
            .unwrap_err();
        assert!(matches!(
            e.downcast_ref::<KeysError>(),
            Some(KeysError::AlreadyExists(_))
        ));
        assert!(r.create_keys("../x", UnlockMode::NodeUnlock, None).is_err());
    }

    #[test]
    fn change_password_requires_the_unlocked_key() {
        let tmp = tempfile::tempdir().unwrap();
        let r = ring(tmp.path());
        let (_, _) = r.create_keys("s1", UnlockMode::NodeUnlock, None).unwrap();
        let (foreign, _) = r.create_keys("s2", UnlockMode::NodeUnlock, None).unwrap();
        assert!(r.change_password("s1", &foreign, PW).is_err());
        let own = r.unlock_with_node("s1").unwrap();
        r.change_password("s1", &own, PW).unwrap();
        assert!(ring(tmp.path()).unlock_with_password("s1", PW).is_ok());
        // locked ring cannot change the password
        let cold = ring(tmp.path());
        assert!(cold
            .change_password("s1", &own, "another password")
            .is_err());
    }
}
