//! Encrypted cloud backup of the memory store to Xavier Cloud.
//!
//! Before this module a node configured with `PGHEART_URL` / `PGHEART_TOKEN`
//! never uploaded anything: the only cloud code path (`CloudPeer`) runs only
//! for mesh peers flagged `is_cloud`, and `xavier sync now` merely printed the
//! session-sync status. This module is the real backup path.
//!
//! # What is uploaded
//!
//! Every record of every workspace of the local store, serialized as JSONL,
//! gzip-compressed and grouped into *packs* (one per ISO week of `created_at`,
//! split further when large). Embeddings are **not** uploaded (owner policy:
//! vectors stay local, see `sqlite_vec_store::at_rest`); restored records come
//! back with `embedding_status = "pending"` and are re-embedded locally.
//! Records the node cannot decrypt (locked private rows) are never uploaded:
//! they are counted and the backup is reported incomplete.
//!
//! # Encryption (client-side, the Worker only sees ciphertext)
//!
//! - `KEK = Argon2id(passphrase, salt)` (64 MiB, t=3, p=4 by default). The
//!   passphrase comes from `XAVIER_CLOUD_BACKUP_PASSPHRASE` or the file named
//!   by `XAVIER_CLOUD_BACKUP_PASSPHRASE_FILE`; it never leaves the device.
//! - Per pack: `pack_id = HMAC(KEK, "xcb1/pack-id" || gz)`,
//!   `DEK = HMAC(KEK, "xcb1/pack-dek" || pack_id)`,
//!   `nonce = HMAC(KEK, "xcb1/pack-nonce" || pack_id)[..12]`,
//!   `blob = "XCB1" || nonce || AES-256-GCM(DEK, nonce, gz, aad = "xcb1|" || pack_id)`.
//!   The DEK is derived, not stored: no key material (wrapped or not) is ever
//!   uploaded. Determinism is deliberate (SIV-style): an unchanged pack yields
//!   the same blob, so periodic backups only upload what changed. A nonce is
//!   only ever reused with the exact same plaintext under the same key.
//! - The blob is stored under `sha256(blob)`; restore verifies that hash, the
//!   GCM tag and the keyed `pack_id` before importing anything.
//!
//! The manifest (stored as the chunk `xcb1-manifest-<instance>`) holds only
//! public parameters: KDF salt and costs, a key-check value, pack hashes,
//! sizes and record counts. Visible to the operator: number and size of packs
//! and record counts; never content, paths, metadata or keys.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Write};

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{anyhow, bail, Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Datelike, Utc};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::crypto::hmac::hmac_sha256;
use crate::memory::store::{MemoryRecord, MemoryStore};

/// Manifest format identifier.
pub const FORMAT: &str = "xavier-cloud-backup/v1";
/// Magic prefix of every encrypted pack blob.
const BLOB_MAGIC: &[u8; 4] = b"XCB1";
const NONCE_LEN: usize = 12;
/// Env var holding the backup passphrase.
pub const PASSPHRASE_ENV: &str = "XAVIER_CLOUD_BACKUP_PASSPHRASE";
/// Env var naming a file that holds the backup passphrase (first line).
pub const PASSPHRASE_FILE_ENV: &str = "XAVIER_CLOUD_BACKUP_PASSPHRASE_FILE";
/// Env var with the periodic backup interval in minutes (0 disables).
pub const INTERVAL_ENV: &str = "XAVIER_CLOUD_BACKUP_INTERVAL_MINS";
/// Default periodic interval when cloud backup is fully configured.
pub const DEFAULT_INTERVAL_MINS: u64 = 360;
/// Minimum passphrase length (characters).
pub const MIN_PASSPHRASE_CHARS: usize = 12;
/// Uncompressed JSONL budget per pack before splitting.
const PACK_SPLIT_BYTES: usize = 16 * 1024 * 1024;
/// Hard ceiling for one encrypted blob (the Worker accepts 8 MiB).
const MAX_BLOB_BYTES: usize = 7 * 1024 * 1024;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Connection settings for the Xavier Cloud Worker.
#[derive(Clone)]
pub struct CloudBackupConfig {
    pub url: String,
    pub token: String,
    pub instance_id: String,
}

impl std::fmt::Debug for CloudBackupConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CloudBackupConfig")
            .field("url", &self.url)
            .field("token", &"[REDACTED]")
            .field("instance_id", &self.instance_id)
            .finish()
    }
}

/// Read `NAME`, falling back to the documented alias `XAVIER_NAME`.
fn lookup_with_alias(lookup: &dyn Fn(&str) -> Option<String>, name: &str) -> Option<String> {
    let clean = |v: Option<String>| v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    clean(lookup(name)).or_else(|| clean(lookup(&format!("XAVIER_{name}"))))
}

impl CloudBackupConfig {
    /// Build from a variable lookup. `PGHEART_*` is canonical; `XAVIER_PGHEART_*`
    /// is accepted as an alias. The instance id defaults to `default`.
    pub fn from_lookup(lookup: &dyn Fn(&str) -> Option<String>) -> Option<Self> {
        let url = lookup_with_alias(lookup, "PGHEART_URL")?;
        let token = lookup_with_alias(lookup, "PGHEART_TOKEN")?;
        let instance_id = lookup_with_alias(lookup, "PGHEART_INSTANCE_ID")
            .unwrap_or_else(|| "default".to_string());
        Some(Self {
            url: url.trim_end_matches('/').to_string(),
            token,
            instance_id,
        })
    }

    /// From the process environment, then the persisted `pgheart` settings.
    pub fn from_env() -> Option<Self> {
        if let Some(cfg) = Self::from_lookup(&|k| std::env::var(k).ok()) {
            return Some(cfg);
        }
        let s = crate::settings::XavierSettings::current().pgheart;
        let (url, token) = (s.url?, s.token?);
        let map: HashMap<&str, String> = [
            ("PGHEART_URL", url),
            ("PGHEART_TOKEN", token),
            ("PGHEART_INSTANCE_ID", s.instance_id.unwrap_or_default()),
        ]
        .into_iter()
        .collect();
        Self::from_lookup(&|k| map.get(k).cloned())
    }

    /// Instance id restricted to the Worker's chunk-name alphabet.
    pub fn safe_instance_id(&self) -> String {
        sanitize_instance_id(&self.instance_id)
    }
}

/// Map an instance id to `[A-Za-z0-9_-]{1,48}`.
pub fn sanitize_instance_id(id: &str) -> String {
    let s: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .take(48)
        .collect();
    if s.is_empty() {
        "default".into()
    } else {
        s
    }
}

/// Chunk name of the manifest for an instance.
pub fn manifest_chunk_name(instance_id: &str) -> String {
    format!("xcb1-manifest-{}", sanitize_instance_id(instance_id))
}

/// Read and validate the backup passphrase from the environment.
/// `Ok(None)` when neither variable is set.
pub fn passphrase_from_env() -> Result<Option<Zeroizing<String>>> {
    if let Ok(p) = std::env::var(PASSPHRASE_ENV) {
        if !p.is_empty() {
            return validate_passphrase(Zeroizing::new(p)).map(Some);
        }
    }
    if let Ok(path) = std::env::var(PASSPHRASE_FILE_ENV) {
        if !path.trim().is_empty() {
            return passphrase_from_file(std::path::Path::new(path.trim())).map(Some);
        }
    }
    Ok(None)
}

/// Read the passphrase from the first line of a file.
pub fn passphrase_from_file(path: &std::path::Path) -> Result<Zeroizing<String>> {
    ensure_private_file(path)?;
    let raw = Zeroizing::new(
        std::fs::read_to_string(path)
            .with_context(|| format!("cannot read passphrase file {}", path.display()))?,
    );
    let first = raw.lines().next().unwrap_or("").trim_end_matches('\r');
    validate_passphrase(Zeroizing::new(first.to_string()))
}

/// Refuse passphrase files readable by group or others (unix).
fn ensure_private_file(path: &std::path::Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)
            .with_context(|| format!("cannot read passphrase file {}", path.display()))?
            .permissions()
            .mode();
        if mode & 0o077 != 0 {
            tracing::warn!(
                "cloud_backup: passphrase file {} has mode {:o}",
                path.display(),
                mode & 0o777
            );
            bail!(
                "passphrase file {} is accessible by other users (mode {:o}); run `chmod 600` on it",
                path.display(),
                mode & 0o777
            );
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Create a new passphrase file readable only by the owner (0600 on unix).
pub fn write_passphrase_file(path: &std::path::Path, passphrase: &str) -> Result<()> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts
        .open(path)
        .with_context(|| format!("cannot create passphrase file {}", path.display()))?;
    f.write_all(passphrase.as_bytes())?;
    f.write_all(b"\n")?;
    Ok(())
}

fn validate_passphrase(p: Zeroizing<String>) -> Result<Zeroizing<String>> {
    if p.chars().count() < MIN_PASSPHRASE_CHARS {
        bail!("the cloud backup passphrase must have at least {MIN_PASSPHRASE_CHARS} characters");
    }
    Ok(p)
}

/// Periodic interval from the environment (`None` = disabled).
pub fn interval_from_env() -> Option<std::time::Duration> {
    let mins = match std::env::var(INTERVAL_ENV) {
        Ok(v) => v.trim().parse::<u64>().unwrap_or(DEFAULT_INTERVAL_MINS),
        Err(_) => DEFAULT_INTERVAL_MINS,
    };
    (mins > 0).then(|| std::time::Duration::from_secs(mins * 60))
}

// ---------------------------------------------------------------------------
// Key derivation and pack sealing
// ---------------------------------------------------------------------------

/// Argon2id parameters stored (publicly) in the manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KdfParams {
    pub alg: String,
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
    pub salt_b64: String,
}

/// Upper bounds for Argon2 costs accepted from a (remote) manifest.
const MAX_KDF_M_KIB: u32 = 256 * 1024;
const MAX_KDF_T: u32 = 10;
const MAX_KDF_P: u32 = 16;

impl KdfParams {
    /// Production defaults with a fresh random salt.
    pub fn new_default() -> Self {
        Self::with_costs(64 * 1024, 3, 4)
    }

    /// Custom costs (tests use cheap ones) with a fresh random salt.
    pub fn with_costs(m_kib: u32, t: u32, p: u32) -> Self {
        let mut salt = [0u8; 16];
        rand::rngs::OsRng.fill_bytes(&mut salt);
        Self {
            alg: "argon2id".into(),
            m_kib,
            t,
            p,
            salt_b64: crate::crypto::base64_encode(salt),
        }
    }

    /// Derive the 32-byte KEK.
    pub fn derive(&self, passphrase: &str) -> Result<Zeroizing<[u8; 32]>> {
        if self.alg != "argon2id" {
            bail!("unsupported KDF '{}'", self.alg);
        }
        if self.m_kib > MAX_KDF_M_KIB || self.t > MAX_KDF_T || self.p > MAX_KDF_P {
            bail!(
                "refusing Argon2 costs above the cap (m_kib={} max {MAX_KDF_M_KIB}, t={} max {MAX_KDF_T}, p={} max {MAX_KDF_P})",
                self.m_kib,
                self.t,
                self.p
            );
        }
        let salt = crate::crypto::base64_decode(&self.salt_b64)
            .filter(|s| s.len() >= 8)
            .ok_or_else(|| anyhow!("invalid KDF salt in the backup manifest"))?;
        let params = argon2::Params::new(self.m_kib, self.t, self.p, Some(32))
            .map_err(|e| anyhow!("invalid Argon2 parameters: {e}"))?;
        let argon =
            argon2::Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
        let mut out = Zeroizing::new([0u8; 32]);
        argon
            .hash_password_into(passphrase.as_bytes(), &salt, out.as_mut())
            .map_err(|e| anyhow!("Argon2 derivation failed: {e}"))?;
        Ok(out)
    }
}

fn tagged_mac(key: &[u8; 32], tag: &[u8], data: &[u8]) -> [u8; 32] {
    let mut buf = Vec::with_capacity(tag.len() + data.len());
    buf.extend_from_slice(tag);
    buf.extend_from_slice(data);
    hmac_sha256(key, &buf)
}

/// Key-check value: lets a wrong passphrase fail fast without revealing the key.
pub fn key_check_value(kek: &[u8; 32]) -> String {
    crate::crypto::hex_encode(&tagged_mac(kek, b"xcb1/kcv", b"")[..16])
}

/// An encrypted pack ready to upload.
#[derive(Debug, Clone)]
pub struct SealedPack {
    /// `sha256(blob)` hex, the chunk name on the Worker.
    pub hash: String,
    /// Keyed id of the plaintext (hex HMAC).
    pub pack_id: String,
    pub blob: Vec<u8>,
}

fn pack_keys(kek: &[u8; 32], pack_id_hex: &str) -> (Zeroizing<[u8; 32]>, [u8; NONCE_LEN]) {
    let dek = Zeroizing::new(tagged_mac(kek, b"xcb1/pack-dek", pack_id_hex.as_bytes()));
    let n = tagged_mac(kek, b"xcb1/pack-nonce", pack_id_hex.as_bytes());
    let mut nonce = [0u8; NONCE_LEN];
    nonce.copy_from_slice(&n[..NONCE_LEN]);
    (dek, nonce)
}

/// Encrypt one pack plaintext (already gzip'd).
pub fn seal_pack(kek: &[u8; 32], plaintext: &[u8]) -> Result<SealedPack> {
    let pack_id = crate::crypto::hex_encode(tagged_mac(kek, b"xcb1/pack-id", plaintext));
    let (dek, nonce) = pack_keys(kek, &pack_id);
    let cipher = Aes256Gcm::new_from_slice(dek.as_ref()).map_err(|_| anyhow!("bad DEK length"))?;
    let aad = format!("xcb1|{pack_id}");
    let ct = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| anyhow!("pack encryption failed"))?;
    let mut blob = Vec::with_capacity(4 + NONCE_LEN + ct.len());
    blob.extend_from_slice(BLOB_MAGIC);
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ct);
    let hash = crate::crypto::hex_encode(Sha256::digest(&blob));
    Ok(SealedPack {
        hash,
        pack_id,
        blob,
    })
}

/// Verify and decrypt a pack blob. Fails on any tampering or wrong key.
pub fn open_pack(
    kek: &[u8; 32],
    expected_hash: &str,
    pack_id: &str,
    blob: &[u8],
) -> Result<Vec<u8>> {
    let actual = crate::crypto::hex_encode(Sha256::digest(blob));
    if actual != expected_hash {
        bail!("pack {expected_hash}: content hash mismatch (corrupted or substituted blob)");
    }
    if blob.len() < 4 + NONCE_LEN + 16 || &blob[..4] != BLOB_MAGIC {
        bail!("pack {expected_hash}: not an XCB1 blob");
    }
    let (dek, nonce) = pack_keys(kek, pack_id);
    if blob[4..4 + NONCE_LEN] != nonce {
        bail!("pack {expected_hash}: nonce does not match its manifest entry");
    }
    let cipher = Aes256Gcm::new_from_slice(dek.as_ref()).map_err(|_| anyhow!("bad DEK length"))?;
    let aad = format!("xcb1|{pack_id}");
    let pt = cipher
        .decrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: &blob[4 + NONCE_LEN..],
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| {
            anyhow!(
                "pack {expected_hash}: authentication failed (wrong passphrase or tampered data)"
            )
        })?;
    let check = crate::crypto::hex_encode(tagged_mac(kek, b"xcb1/pack-id", &pt));
    if check != pack_id {
        bail!("pack {expected_hash}: plaintext does not match its keyed id");
    }
    Ok(pt)
}

// ---------------------------------------------------------------------------
// Record (de)serialization and packing
// ---------------------------------------------------------------------------

/// Copy of a record as stored in a backup: plaintext fields only, no vectors,
/// no local at-rest envelope (the restoring node re-seals with its own key).
pub fn backup_copy(record: &MemoryRecord) -> MemoryRecord {
    let mut r = record.clone();
    r.embedding = Vec::new();
    r.embedding_status = "pending".into();
    r.embedding_attempts = 0;
    r.encrypted_dek = None;
    r.content_iv = None;
    r.metadata_iv = None;
    r.score = 0.0;
    r
}

fn week_key(ts: &DateTime<Utc>) -> String {
    let w = ts.iso_week();
    format!("{}-W{:02}", w.year(), w.week())
}

fn gzip(data: &[u8]) -> Result<Vec<u8>> {
    // GzEncoder writes mtime 0 and no file name: identical input → identical bytes.
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(data)?;
    Ok(enc.finish()?)
}

fn gunzip(data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(data)
        .read_to_end(&mut out)
        .context("pack is not valid gzip")?;
    Ok(out)
}

fn jsonl(records: &[MemoryRecord]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for r in records {
        serde_json::to_writer(&mut out, r)?;
        out.push(b'\n');
    }
    Ok(out)
}

/// Group records into deterministic pack plaintexts (gzip'd JSONL).
/// Returns `(gz_bytes, record_count)` per pack.
pub fn build_pack_plaintexts(records: &[MemoryRecord]) -> Result<Vec<(Vec<u8>, usize)>> {
    let mut groups: BTreeMap<String, Vec<MemoryRecord>> = BTreeMap::new();
    for r in records {
        groups
            .entry(week_key(&r.created_at))
            .or_default()
            .push(r.clone());
    }
    let mut out = Vec::new();
    for (_, mut group) in groups {
        group.sort_by(|a, b| (&a.workspace_id, &a.id).cmp(&(&b.workspace_id, &b.id)));
        split_into(&group, &mut out)?;
    }
    Ok(out)
}

fn split_into(group: &[MemoryRecord], out: &mut Vec<(Vec<u8>, usize)>) -> Result<()> {
    let raw = jsonl(group)?;
    let gz = gzip(&raw)?;
    if group.len() > 1 && (raw.len() > PACK_SPLIT_BYTES || gz.len() > MAX_BLOB_BYTES) {
        let mid = group.len() / 2;
        split_into(&group[..mid], out)?;
        split_into(&group[mid..], out)?;
        return Ok(());
    }
    if gz.len() > MAX_BLOB_BYTES {
        bail!(
            "memory record {} is too large for a cloud pack ({} bytes compressed)",
            group.first().map(|r| r.id.as_str()).unwrap_or("?"),
            gz.len()
        );
    }
    out.push((gz, group.len()));
    Ok(())
}

/// Parse a decrypted pack back into records.
pub fn parse_pack(gz: &[u8]) -> Result<Vec<MemoryRecord>> {
    let raw = gunzip(gz)?;
    let mut out = Vec::new();
    for (i, line) in raw.split(|b| *b == b'\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        out.push(
            serde_json::from_slice(line)
                .with_context(|| format!("invalid record at line {}", i + 1))?,
        );
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Manifest
// ---------------------------------------------------------------------------

/// One pack in the manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PackRef {
    pub hash: String,
    pub pack_id: String,
    pub bytes: u64,
    pub records: usize,
}

/// Public description of the latest backup of an instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    pub format: String,
    pub instance_id: String,
    pub created_at: DateTime<Utc>,
    pub xavier_version: String,
    pub kdf: KdfParams,
    pub kcv: String,
    pub record_count: usize,
    pub workspace_count: usize,
    pub total_bytes: u64,
    pub packs: Vec<PackRef>,
    /// False when locked records were left out of the backup.
    #[serde(default)]
    pub complete: bool,
    /// Hex HMAC over every other field, keyed with the KEK.
    #[serde(default)]
    pub mac: String,
}

impl BackupManifest {
    fn compute_mac(&self, kek: &[u8; 32]) -> Result<String> {
        let mut unsigned = self.clone();
        unsigned.mac = String::new();
        let bytes = serde_json::to_vec(&unsigned)?;
        Ok(crate::crypto::hex_encode(tagged_mac(
            kek,
            b"xcb1/manifest",
            &bytes,
        )))
    }

    fn seal(&mut self, kek: &[u8; 32]) -> Result<()> {
        self.mac = self.compute_mac(kek)?;
        Ok(())
    }

    /// Fails when the manifest (packs, counts, order) was altered or never signed.
    fn authenticate(&self, kek: &[u8; 32]) -> Result<()> {
        use subtle::ConstantTimeEq;
        let expected = self.compute_mac(kek)?;
        if !bool::from(expected.as_bytes().ct_eq(self.mac.as_bytes())) {
            bail!(
                "the cloud backup manifest failed authentication (tampered, truncated or unsigned)"
            );
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Remote storage
// ---------------------------------------------------------------------------

/// Tenant usage as reported by the Worker.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct CloudUsage {
    #[serde(default)]
    pub tenant: String,
    #[serde(default)]
    pub bytes: u64,
    #[serde(default)]
    pub chunks: u64,
    #[serde(default)]
    pub quota: u64,
    #[serde(default)]
    pub percent: f64,
}

/// Minimal blob API of the Xavier Cloud Worker (`/v1/cloud/*`).
#[async_trait]
pub trait BlobStore: Send + Sync {
    async fn put(&self, name: &str, data: &[u8]) -> Result<()>;
    async fn get(&self, name: &str) -> Result<Option<Vec<u8>>>;
    async fn exists(&self, name: &str) -> Result<bool>;
    async fn usage(&self) -> Result<CloudUsage>;
}

/// HTTP client for the Worker. Uses the native `/v1/cloud/chunks/:name`
/// routes (raw bytes, no base64 overhead) with `Authorization: Bearer`.
pub struct HttpBlobStore {
    client: reqwest::Client,
    base: String,
    token: String,
}

/// The Bearer token may only travel over HTTPS, or HTTP to a loopback host.
fn ensure_token_transport(url: &str) -> Result<()> {
    let u = reqwest::Url::parse(url).with_context(|| "invalid PGHEART_URL")?;
    match u.scheme() {
        "https" => Ok(()),
        "http" => {
            let loopback = match u.host() {
                Some(url::Host::Domain(d)) => d.eq_ignore_ascii_case("localhost"),
                Some(url::Host::Ipv4(a)) => a.is_loopback(),
                Some(url::Host::Ipv6(a)) => a.is_loopback(),
                None => false,
            };
            if loopback {
                Ok(())
            } else {
                bail!("refusing to send the Xavier Cloud token over plain HTTP to a non-loopback host; use https://")
            }
        }
        other => bail!("unsupported PGHEART_URL scheme '{other}'"),
    }
}

impl HttpBlobStore {
    pub fn new(cfg: &CloudBackupConfig) -> Result<Self> {
        ensure_token_transport(&cfg.url)?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Ok(Self {
            client,
            base: cfg.url.trim_end_matches('/').to_string(),
            token: cfg.token.clone(),
        })
    }

    fn chunk_url(&self, name: &str) -> String {
        format!("{}/v1/cloud/chunks/{}", self.base, name)
    }

    /// `GET /health` of the Worker (no auth).
    pub async fn health(&self) -> Result<serde_json::Value> {
        let resp = self
            .client
            .get(format!("{}/health", self.base))
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("Xavier Cloud /health answered {}", resp.status());
        }
        Ok(resp.json().await?)
    }
}

async fn error_text(resp: reqwest::Response) -> String {
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    format!("{status}: {}", body.chars().take(300).collect::<String>())
}

#[async_trait]
impl BlobStore for HttpBlobStore {
    async fn put(&self, name: &str, data: &[u8]) -> Result<()> {
        let resp = self
            .client
            .put(self.chunk_url(name))
            .bearer_auth(&self.token)
            .header("Content-Type", "application/octet-stream")
            .body(data.to_vec())
            .send()
            .await
            .with_context(|| format!("upload of chunk {name} failed"))?;
        if !resp.status().is_success() {
            bail!(
                "Xavier Cloud rejected chunk {name}: {}",
                error_text(resp).await
            );
        }
        Ok(())
    }

    async fn get(&self, name: &str) -> Result<Option<Vec<u8>>> {
        let resp = self
            .client
            .get(self.chunk_url(name))
            .bearer_auth(&self.token)
            .send()
            .await
            .with_context(|| format!("download of chunk {name} failed"))?;
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        if !resp.status().is_success() {
            bail!(
                "Xavier Cloud refused chunk {name}: {}",
                error_text(resp).await
            );
        }
        Ok(Some(resp.bytes().await?.to_vec()))
    }

    async fn exists(&self, name: &str) -> Result<bool> {
        let resp = self
            .client
            .head(self.chunk_url(name))
            .bearer_auth(&self.token)
            .send()
            .await?;
        match resp.status() {
            s if s.is_success() => Ok(true),
            reqwest::StatusCode::NOT_FOUND => Ok(false),
            s => bail!("Xavier Cloud HEAD {name} answered {s}"),
        }
    }

    async fn usage(&self) -> Result<CloudUsage> {
        let resp = self
            .client
            .get(format!("{}/v1/cloud/usage", self.base))
            .bearer_auth(&self.token)
            .send()
            .await?;
        if !resp.status().is_success() {
            bail!("Xavier Cloud /v1/cloud/usage: {}", error_text(resp).await);
        }
        Ok(resp.json().await?)
    }
}

// ---------------------------------------------------------------------------
// Backup / restore / verify
// ---------------------------------------------------------------------------

/// Outcome of a backup run. Every number is measured, never assumed.
#[derive(Debug, Clone, Serialize, Default)]
pub struct BackupReport {
    pub instance_id: String,
    pub records_total: usize,
    pub records_backed_up: usize,
    /// Private rows the node could not decrypt: NOT uploaded.
    pub records_locked: usize,
    pub workspaces: usize,
    pub packs_total: usize,
    pub packs_uploaded: usize,
    pub packs_unchanged: usize,
    /// Packs listed by the previous manifest but gone from the cloud, re-uploaded.
    pub packs_repaired: usize,
    /// False when an incomplete run left the last complete backup in place.
    pub manifest_published: bool,
    pub bytes_uploaded: u64,
    pub backup_bytes_total: u64,
    pub manifest_chunk: String,
    pub usage_after: Option<CloudUsage>,
    pub complete: bool,
}

/// A row the store could not decrypt: `at_rest::mark_locked` replaces its
/// metadata with exactly `{"locked": true}` (and its content with a marker).
fn is_locked_row(r: &MemoryRecord) -> bool {
    r.metadata
        .as_object()
        .is_some_and(|m| m.len() == 1 && m.get("locked") == Some(&serde_json::Value::Bool(true)))
}

/// Collect every record of every workspace, split into backup copies and a
/// count of locked rows.
pub async fn collect_records(store: &dyn MemoryStore) -> Result<(Vec<MemoryRecord>, usize, usize)> {
    let workspaces = store.list_workspaces().await?;
    let mut out = Vec::new();
    let mut locked = 0usize;
    for ws in &workspaces {
        for r in store.list(ws).await? {
            if is_locked_row(&r) {
                locked += 1;
            } else {
                out.push(backup_copy(&r));
            }
        }
    }
    Ok((out, locked, workspaces.len()))
}

async fn load_manifest(
    remote: &dyn BlobStore,
    instance_id: &str,
) -> Result<Option<BackupManifest>> {
    match remote.get(&manifest_chunk_name(instance_id)).await? {
        None => Ok(None),
        Some(bytes) => {
            let m: BackupManifest = serde_json::from_slice(&bytes)
                .context("the cloud backup manifest is not valid JSON")?;
            if m.format != FORMAT {
                bail!("unsupported cloud backup format '{}'", m.format);
            }
            Ok(Some(m))
        }
    }
}

/// Encrypt and upload the whole store. `new_kdf` is only used when the
/// instance has no previous manifest (the salt is then kept stable so that
/// unchanged packs are not re-uploaded).
pub async fn run_backup(
    store: &dyn MemoryStore,
    remote: &dyn BlobStore,
    cfg: &CloudBackupConfig,
    passphrase: &str,
    new_kdf: KdfParams,
) -> Result<BackupReport> {
    let instance = cfg.safe_instance_id();
    let previous = load_manifest(remote, &instance).await?;
    let kdf = previous.as_ref().map(|m| m.kdf.clone()).unwrap_or(new_kdf);
    let kek = kdf.derive(passphrase)?;
    let kcv = key_check_value(&kek);
    if let Some(prev) = &previous {
        if prev.kcv != kcv {
            bail!(
                "the passphrase does not match the existing cloud backup of instance '{instance}' \
                 (use the same passphrase, or a new PGHEART_INSTANCE_ID)"
            );
        }
        prev.authenticate(&kek)?;
    }
    let already: HashSet<String> = previous
        .as_ref()
        .map(|m| m.packs.iter().map(|p| p.hash.clone()).collect())
        .unwrap_or_default();

    let (records, locked, workspaces) = collect_records(store).await?;
    let plaintexts = build_pack_plaintexts(&records)?;

    let mut report = BackupReport {
        instance_id: instance.clone(),
        records_total: records.len() + locked,
        records_locked: locked,
        workspaces,
        manifest_chunk: manifest_chunk_name(&instance),
        ..Default::default()
    };
    let complete = locked == 0;
    // An incomplete run must never replace the last complete backup.
    if !complete && previous.as_ref().is_some_and(|p| p.complete) {
        report.usage_after = remote.usage().await.ok();
        return Ok(report);
    }
    let mut packs = Vec::with_capacity(plaintexts.len());
    for (gz, count) in plaintexts {
        let sealed = seal_pack(&kek, &gz)?;
        let size = sealed.blob.len() as u64;
        if already.contains(&sealed.hash) && remote.exists(&sealed.hash).await? {
            report.packs_unchanged += 1;
        } else {
            if already.contains(&sealed.hash) {
                report.packs_repaired += 1;
            }
            remote.put(&sealed.hash, &sealed.blob).await?;
            report.packs_uploaded += 1;
            report.bytes_uploaded += size;
        }
        report.records_backed_up += count;
        report.backup_bytes_total += size;
        packs.push(PackRef {
            hash: sealed.hash,
            pack_id: sealed.pack_id,
            bytes: size,
            records: count,
        });
    }
    report.packs_total = packs.len();

    // Manifest last: until it is written the previous backup stays the valid one.
    let mut manifest = BackupManifest {
        format: FORMAT.into(),
        instance_id: instance.clone(),
        created_at: Utc::now(),
        xavier_version: env!("CARGO_PKG_VERSION").into(),
        kdf,
        kcv,
        record_count: report.records_backed_up,
        workspace_count: workspaces,
        total_bytes: report.backup_bytes_total,
        packs,
        complete,
        mac: String::new(),
    };
    manifest.seal(&kek)?;
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    remote.put(&report.manifest_chunk, &manifest_bytes).await?;
    report.bytes_uploaded += manifest_bytes.len() as u64;
    report.manifest_published = true;
    report.usage_after = remote.usage().await.ok();
    report.complete = complete;
    Ok(report)
}

/// Outcome of a restore.
#[derive(Debug, Clone, Serialize, Default)]
pub struct RestoreReport {
    pub instance_id: String,
    pub backup_created_at: Option<DateTime<Utc>>,
    pub records_in_backup: usize,
    pub restored: usize,
    pub skipped_newer_local: usize,
    pub packs: usize,
    pub bytes_downloaded: u64,
}

async fn fetch_and_open(
    remote: &dyn BlobStore,
    kek: &[u8; 32],
    pack: &PackRef,
) -> Result<(Vec<MemoryRecord>, u64)> {
    let blob = remote.get(&pack.hash).await?.ok_or_else(|| {
        anyhow!(
            "pack {} listed in the manifest is missing in the cloud",
            pack.hash
        )
    })?;
    let gz = open_pack(kek, &pack.hash, &pack.pack_id, &blob)?;
    let recs = parse_pack(&gz)?;
    if recs.len() != pack.records {
        bail!(
            "pack {}: {} records, manifest says {}",
            pack.hash,
            recs.len(),
            pack.records
        );
    }
    Ok((recs, blob.len() as u64))
}

/// Download, verify, decrypt and import the latest backup of `instance_id`.
/// Every pack is verified before the first record is written. Local records
/// with the same id and a newer-or-equal `updated_at` are kept.
pub async fn run_restore(
    store: &dyn MemoryStore,
    remote: &dyn BlobStore,
    instance_id: &str,
    passphrase: &str,
) -> Result<RestoreReport> {
    let instance = sanitize_instance_id(instance_id);
    let manifest = load_manifest(remote, &instance)
        .await?
        .ok_or_else(|| anyhow!("no cloud backup found for instance '{instance}'"))?;
    let kek = manifest.kdf.derive(passphrase)?;
    if key_check_value(&kek) != manifest.kcv {
        bail!("wrong passphrase for the cloud backup of instance '{instance}'");
    }
    manifest.authenticate(&kek)?;
    let mut report = RestoreReport {
        instance_id: instance,
        backup_created_at: Some(manifest.created_at),
        records_in_backup: manifest.record_count,
        packs: manifest.packs.len(),
        ..Default::default()
    };
    let mut all = Vec::with_capacity(manifest.record_count);
    for pack in &manifest.packs {
        let (recs, n) = fetch_and_open(remote, &kek, pack).await?;
        report.bytes_downloaded += n;
        all.extend(recs);
    }
    if all.len() != manifest.record_count {
        bail!(
            "backup holds {} records, manifest says {}",
            all.len(),
            manifest.record_count
        );
    }
    for rec in all {
        if let Some(local) = store.get(&rec.workspace_id, &rec.id).await? {
            if local.id == rec.id && local.updated_at >= rec.updated_at {
                report.skipped_newer_local += 1;
                continue;
            }
        }
        store
            .put(rec)
            .await
            .context("importing a restored record failed")?;
        report.restored += 1;
    }
    Ok(report)
}

/// Outcome of `verify`.
#[derive(Debug, Clone, Serialize, Default)]
pub struct VerifyReport {
    pub instance_id: String,
    pub manifest_found: bool,
    pub backup_created_at: Option<DateTime<Utc>>,
    pub records: usize,
    pub packs: usize,
    pub packs_missing: Vec<String>,
    /// `Some(true)` when the manifest MAC checked out (needs the passphrase).
    pub manifest_authenticated: Option<bool>,
    /// `Some(true)` when every pack was downloaded and decrypted (`--deep`).
    pub decrypted_ok: Option<bool>,
    pub usage: Option<CloudUsage>,
    pub ok: bool,
}

/// Check that the latest backup is complete in the cloud. With a passphrase,
/// also download and decrypt every pack (nothing is written locally).
pub async fn run_verify(
    remote: &dyn BlobStore,
    instance_id: &str,
    passphrase: Option<&str>,
) -> Result<VerifyReport> {
    let instance = sanitize_instance_id(instance_id);
    let mut report = VerifyReport {
        instance_id: instance.clone(),
        usage: Some(remote.usage().await?),
        ..Default::default()
    };
    let Some(manifest) = load_manifest(remote, &instance).await? else {
        return Ok(report);
    };
    report.manifest_found = true;
    report.backup_created_at = Some(manifest.created_at);
    report.records = manifest.record_count;
    report.packs = manifest.packs.len();
    for p in &manifest.packs {
        if !remote.exists(&p.hash).await? {
            report.packs_missing.push(p.hash.clone());
        }
    }
    if let Some(pass) = passphrase {
        let kek = manifest.kdf.derive(pass)?;
        if key_check_value(&kek) != manifest.kcv {
            bail!("wrong passphrase for the cloud backup of instance '{instance}'");
        }
        manifest.authenticate(&kek)?;
        report.manifest_authenticated = Some(true);
        let mut ok = report.packs_missing.is_empty();
        if ok {
            for p in &manifest.packs {
                if fetch_and_open(remote, &kek, p).await.is_err() {
                    ok = false;
                    break;
                }
            }
        }
        report.decrypted_ok = Some(ok);
    }
    report.ok = report.packs_missing.is_empty() && report.decrypted_ok != Some(false);
    Ok(report)
}

/// Background loop used by the server: back up every `interval` while the
/// process lives. Errors are logged, never fatal.
pub async fn periodic_backup_loop(
    store: std::sync::Arc<dyn MemoryStore>,
    cfg: CloudBackupConfig,
    passphrase: Zeroizing<String>,
    interval: std::time::Duration,
) {
    let remote = match HttpBlobStore::new(&cfg) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("cloud_backup: disabled: {e:#}");
            return;
        }
    };
    // Let the server finish starting before the first run.
    tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    loop {
        match run_backup(
            store.as_ref(),
            &remote,
            &cfg,
            &passphrase,
            KdfParams::new_default(),
        )
        .await
        {
            Ok(r) => tracing::info!(
                instance = %r.instance_id,
                records = r.records_backed_up,
                locked = r.records_locked,
                packs_uploaded = r.packs_uploaded,
                packs_unchanged = r.packs_unchanged,
                bytes_uploaded = r.bytes_uploaded,
                complete = r.complete,
                "cloud_backup: run finished"
            ),
            Err(e) => tracing::warn!("cloud_backup: run failed: {e:#}"),
        }
        tokio::time::sleep(interval).await;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// In-memory Worker double that also counts uploads.
    #[derive(Default)]
    struct MemBlobs {
        blobs: Mutex<HashMap<String, Vec<u8>>>,
        puts: Mutex<usize>,
    }

    #[async_trait]
    impl BlobStore for MemBlobs {
        async fn put(&self, name: &str, data: &[u8]) -> Result<()> {
            *self.puts.lock().unwrap() += 1;
            self.blobs
                .lock()
                .unwrap()
                .insert(name.into(), data.to_vec());
            Ok(())
        }
        async fn get(&self, name: &str) -> Result<Option<Vec<u8>>> {
            Ok(self.blobs.lock().unwrap().get(name).cloned())
        }
        async fn exists(&self, name: &str) -> Result<bool> {
            Ok(self.blobs.lock().unwrap().contains_key(name))
        }
        async fn usage(&self) -> Result<CloudUsage> {
            let b = self.blobs.lock().unwrap();
            Ok(CloudUsage {
                tenant: "test".into(),
                bytes: b.values().map(|v| v.len() as u64).sum(),
                chunks: b.len() as u64,
                ..Default::default()
            })
        }
    }

    async fn temp_store(
        dir: &std::path::Path,
    ) -> crate::memory::sqlite_vec_store::VecSqliteMemoryStore {
        crate::memory::sqlite_vec_store::VecSqliteMemoryStore::new(
            crate::memory::sqlite_vec_store::VecSqliteStoreConfig {
                path: dir.join("mem.db"),
                embedding_dimensions: 0,
            },
        )
        .await
        .expect("temp store")
    }

    fn record(ws: &str, id: &str, content: &str, days_ago: i64) -> MemoryRecord {
        let ts = Utc::now() - chrono::Duration::days(days_ago);
        MemoryRecord {
            id: id.into(),
            workspace_id: ws.into(),
            path: format!("notes/{id}"),
            content: content.into(),
            metadata: serde_json::json!({"kind": "note", "public": true}),
            embedding: vec![0.25; 8],
            created_at: ts,
            updated_at: ts,
            revision: 1,
            primary: true,
            parent_id: None,
            cluster_id: None,
            level: Default::default(),
            relation: None,
            clearance: Default::default(),
            revisions: Vec::new(),
            encrypted_dek: None,
            content_iv: None,
            metadata_iv: None,
            score: 0.0,
            deleted_at: None,
            embedding_status: "done".into(),
            embedding_attempts: 0,
        }
    }

    fn cfg() -> CloudBackupConfig {
        CloudBackupConfig {
            url: "https://cloud.invalid".into(),
            token: "tok".into(),
            instance_id: "test node!".into(),
        }
    }

    const PASS: &str = "correct horse battery staple";

    fn cheap_kdf() -> KdfParams {
        KdfParams::with_costs(256, 1, 1)
    }

    #[test]
    fn config_prefers_canonical_and_accepts_alias() {
        let env: HashMap<&str, &str> = [
            ("XAVIER_PGHEART_URL", "https://alias.example/"),
            ("XAVIER_PGHEART_TOKEN", "alias-token"),
        ]
        .into_iter()
        .collect();
        let c = CloudBackupConfig::from_lookup(&|k| env.get(k).map(|v| v.to_string())).unwrap();
        assert_eq!(c.url, "https://alias.example");
        assert_eq!(c.token, "alias-token");
        assert_eq!(c.instance_id, "default");

        let env2: HashMap<&str, &str> = [
            ("PGHEART_URL", "https://canon.example"),
            ("XAVIER_PGHEART_URL", "https://alias.example"),
            ("PGHEART_TOKEN", "t"),
            ("PGHEART_INSTANCE_ID", "box-1"),
        ]
        .into_iter()
        .collect();
        let c2 = CloudBackupConfig::from_lookup(&|k| env2.get(k).map(|v| v.to_string())).unwrap();
        assert_eq!(c2.url, "https://canon.example");
        assert_eq!(c2.instance_id, "box-1");
        assert!(
            !format!("{c2:?}").contains("\"t\""),
            "token must be redacted in Debug"
        );

        assert!(CloudBackupConfig::from_lookup(&|_| None).is_none());
    }

    #[test]
    fn instance_ids_are_sanitized_for_chunk_names() {
        assert_eq!(sanitize_instance_id("groku-box"), "groku-box");
        assert_eq!(sanitize_instance_id("a/b c"), "a-b-c");
        assert_eq!(sanitize_instance_id(""), "default");
        assert_eq!(manifest_chunk_name("x"), "xcb1-manifest-x");
    }

    #[test]
    fn short_passphrases_are_rejected() {
        assert!(validate_passphrase(Zeroizing::new("short".into())).is_err());
        assert!(validate_passphrase(Zeroizing::new(PASS.into())).is_ok());
    }

    #[test]
    fn seal_is_deterministic_authenticated_and_hides_plaintext() {
        let kek = cheap_kdf().derive(PASS).unwrap();
        let pt = gzip(b"secret memory about project titan").unwrap();
        let a = seal_pack(&kek, &pt).unwrap();
        let b = seal_pack(&kek, &pt).unwrap();
        assert_eq!(
            a.hash, b.hash,
            "same plaintext must give the same blob (dedupe)"
        );
        assert!(!a.blob.windows(6).any(|w| w == b"secret"));
        assert_eq!(open_pack(&kek, &a.hash, &a.pack_id, &a.blob).unwrap(), pt);

        let other = seal_pack(&kek, &gzip(b"different").unwrap()).unwrap();
        assert_ne!(other.hash, a.hash);

        let mut tampered = a.blob.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        let th = crate::crypto::hex_encode(Sha256::digest(&tampered));
        assert!(open_pack(&kek, &th, &a.pack_id, &tampered).is_err());
        assert!(open_pack(&kek, &a.hash, &a.pack_id, &tampered).is_err());

        let wrong = KdfParams { ..cheap_kdf() }
            .derive("another passphrase!")
            .unwrap();
        assert!(open_pack(&wrong, &a.hash, &a.pack_id, &a.blob).is_err());
    }

    #[test]
    fn packs_strip_vectors_and_local_envelope() {
        let mut r = record("ws", "r1", "hello", 0);
        r.encrypted_dek = Some(vec![1, 2, 3]);
        let copy = backup_copy(&r);
        assert!(copy.embedding.is_empty());
        assert!(copy.encrypted_dek.is_none());
        assert_eq!(copy.embedding_status, "pending");
        let packs = build_pack_plaintexts(std::slice::from_ref(&copy)).unwrap();
        let back = parse_pack(&packs[0].0).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].content, "hello");
        assert!(back[0].embedding.is_empty());
    }

    #[test]
    fn locked_rows_are_detected() {
        let mut r = record("ws", "l", "[locked]", 0);
        assert!(!is_locked_row(&r));
        r.metadata = serde_json::json!({"locked": true});
        assert!(is_locked_row(&r));
        r.metadata = serde_json::json!({"locked": true, "kind": "note"});
        assert!(!is_locked_row(&r));
    }

    #[test]
    fn records_are_grouped_by_week_deterministically() {
        let recs = vec![
            record("ws", "a", "1", 0),
            record("ws", "b", "2", 30),
            record("other", "c", "3", 30),
        ];
        let p1 = build_pack_plaintexts(&recs).unwrap();
        let mut rev = recs.clone();
        rev.reverse();
        let p2 = build_pack_plaintexts(&rev).unwrap();
        assert_eq!(p1.len(), 2);
        assert_eq!(p1, p2, "input order must not change the packs");
        assert_eq!(p1.iter().map(|p| p.1).sum::<usize>(), 3);
    }

    #[tokio::test]
    async fn backup_then_restore_round_trips_into_an_empty_store() {
        let src_dir = tempfile::tempdir().unwrap();
        let dst_dir = tempfile::tempdir().unwrap();
        let src = temp_store(src_dir.path()).await;
        for (i, (ws, days)) in [("alpha", 0), ("alpha", 10), ("beta", 40)]
            .iter()
            .enumerate()
        {
            src.put(record(
                ws,
                &format!("m{i}"),
                &format!("memory number {i}"),
                *days,
            ))
            .await
            .unwrap();
        }
        let remote = MemBlobs::default();

        let rep = run_backup(&src, &remote, &cfg(), PASS, cheap_kdf())
            .await
            .unwrap();
        assert_eq!(rep.records_backed_up, 3);
        assert_eq!(rep.records_locked, 0);
        assert_eq!(rep.workspaces, 2);
        assert!(rep.complete);
        assert!(rep.packs_uploaded >= 2);
        assert!(rep.bytes_uploaded > 0);
        assert_eq!(rep.usage_after.as_ref().unwrap().bytes, {
            let b = remote.blobs.lock().unwrap();
            b.values().map(|v| v.len() as u64).sum::<u64>()
        });
        // Nothing in the cloud contains plaintext.
        for blob in remote.blobs.lock().unwrap().values() {
            assert!(!blob.windows(13).any(|w| w == b"memory number"));
        }

        // Second run without changes uploads only the manifest.
        let puts_before = *remote.puts.lock().unwrap();
        let rep2 = run_backup(&src, &remote, &cfg(), PASS, cheap_kdf())
            .await
            .unwrap();
        assert_eq!(rep2.packs_uploaded, 0);
        assert_eq!(rep2.packs_unchanged, rep.packs_total);
        assert_eq!(*remote.puts.lock().unwrap(), puts_before + 1);

        // Wrong passphrase: refused for both backup and restore.
        assert!(run_backup(
            &src,
            &remote,
            &cfg(),
            "not the right passphrase",
            cheap_kdf()
        )
        .await
        .is_err());
        let dst = temp_store(dst_dir.path()).await;
        assert!(run_restore(
            &dst,
            &remote,
            &cfg().instance_id,
            "not the right passphrase"
        )
        .await
        .is_err());

        let rr = run_restore(&dst, &remote, &cfg().instance_id, PASS)
            .await
            .unwrap();
        assert_eq!(rr.records_in_backup, 3);
        assert_eq!(rr.restored, 3);
        let mut contents: Vec<String> = Vec::new();
        for ws in ["alpha", "beta"] {
            for r in dst.list(ws).await.unwrap() {
                contents.push(r.content);
            }
        }
        contents.sort();
        assert_eq!(
            contents,
            vec!["memory number 0", "memory number 1", "memory number 2"]
        );

        // Restoring again keeps the local copies (same updated_at).
        let again = run_restore(&dst, &remote, &cfg().instance_id, PASS)
            .await
            .unwrap();
        assert_eq!(again.restored, 0);
        assert_eq!(again.skipped_newer_local, 3);

        let v = run_verify(&remote, &cfg().instance_id, Some(PASS))
            .await
            .unwrap();
        assert!(v.ok && v.manifest_found && v.decrypted_ok == Some(true));
        assert_eq!(v.records, 3);
    }

    #[tokio::test]
    async fn verify_reports_missing_packs_and_absent_backups() {
        let remote = MemBlobs::default();
        let v = run_verify(&remote, "nobody", None).await.unwrap();
        assert!(!v.manifest_found);

        let dir = tempfile::tempdir().unwrap();
        let store = temp_store(dir.path()).await;
        store.put(record("ws", "x", "content", 0)).await.unwrap();
        let rep = run_backup(&store, &remote, &cfg(), PASS, cheap_kdf())
            .await
            .unwrap();
        let first_pack = remote
            .blobs
            .lock()
            .unwrap()
            .keys()
            .find(|k| !k.starts_with("xcb1-manifest-"))
            .cloned()
            .unwrap();
        remote.blobs.lock().unwrap().remove(&first_pack);
        let v = run_verify(&remote, &rep.instance_id, None).await.unwrap();
        assert!(!v.ok);
        assert_eq!(v.packs_missing, vec![first_pack]);
    }

    async fn two_pack_backup(
        remote: &MemBlobs,
        dir: &std::path::Path,
    ) -> crate::memory::sqlite_vec_store::VecSqliteMemoryStore {
        let store = temp_store(dir).await;
        store.put(record("ws", "a", "one", 0)).await.unwrap();
        store.put(record("ws", "b", "two", 30)).await.unwrap();
        run_backup(&store, remote, &cfg(), PASS, cheap_kdf())
            .await
            .unwrap();
        store
    }

    fn rewrite_manifest(remote: &MemBlobs, f: impl FnOnce(&mut BackupManifest)) {
        let name = manifest_chunk_name(&cfg().instance_id);
        let mut blobs = remote.blobs.lock().unwrap();
        let mut m: BackupManifest = serde_json::from_slice(&blobs[&name]).unwrap();
        f(&mut m);
        blobs.insert(name, serde_json::to_vec(&m).unwrap());
    }

    #[tokio::test]
    async fn tampered_manifest_is_rejected_by_restore_and_verify() {
        let dir = tempfile::tempdir().unwrap();
        let dst_dir = tempfile::tempdir().unwrap();
        let remote = MemBlobs::default();
        two_pack_backup(&remote, dir.path()).await;
        let dst = temp_store(dst_dir.path()).await;
        let id = cfg().instance_id;

        // Truncation with consistent counters.
        rewrite_manifest(&remote, |m| {
            let gone = m.packs.pop().unwrap();
            m.record_count -= gone.records;
            m.total_bytes -= gone.bytes;
        });
        let e = run_restore(&dst, &remote, &id, PASS).await.unwrap_err();
        assert!(e.to_string().contains("authentication"), "{e}");
        let e = run_verify(&remote, &id, Some(PASS)).await.unwrap_err();
        assert!(e.to_string().contains("authentication"), "{e}");
        assert!(dst.list("ws").await.unwrap().is_empty());

        // Reordering.
        let remote = MemBlobs::default();
        two_pack_backup(&remote, dir.path()).await;
        rewrite_manifest(&remote, |m| m.packs.reverse());
        assert!(run_restore(&dst, &remote, &id, PASS).await.is_err());

        // A backup must not build on a tampered previous manifest.
        let store = temp_store(dir.path()).await;
        assert!(run_backup(&store, &remote, &cfg(), PASS, cheap_kdf())
            .await
            .is_err());
    }

    #[test]
    fn kdf_costs_above_the_cap_are_rejected() {
        for (m, t, p) in [
            (MAX_KDF_M_KIB + 1, 1, 1),
            (256, MAX_KDF_T + 1, 1),
            (256, 1, MAX_KDF_P + 1),
            (u32::MAX, u32::MAX, u32::MAX),
        ] {
            let e = KdfParams::with_costs(m, t, p).derive(PASS).unwrap_err();
            assert!(e.to_string().contains("above the cap"), "{e}");
        }
        assert!(KdfParams::with_costs(MAX_KDF_M_KIB.min(256), 1, 1)
            .derive(PASS)
            .is_ok());
    }

    #[tokio::test]
    async fn hostile_manifest_costs_fail_before_deriving() {
        let dir = tempfile::tempdir().unwrap();
        let remote = MemBlobs::default();
        let store = two_pack_backup(&remote, dir.path()).await;
        rewrite_manifest(&remote, |m| m.kdf.m_kib = u32::MAX);
        let id = cfg().instance_id;
        let e = run_restore(&store, &remote, &id, PASS).await.unwrap_err();
        assert!(e.to_string().contains("above the cap"), "{e}");
        let e = run_verify(&remote, &id, Some(PASS)).await.unwrap_err();
        assert!(e.to_string().contains("above the cap"), "{e}");
    }

    #[tokio::test]
    async fn incomplete_backup_keeps_the_last_complete_one() {
        let dir = tempfile::tempdir().unwrap();
        let remote = MemBlobs::default();
        let store = two_pack_backup(&remote, dir.path()).await;
        let name = manifest_chunk_name(&cfg().instance_id);
        let before = remote.blobs.lock().unwrap()[&name].clone();

        let mut locked = record("ws", "locked", "[locked]", 0);
        locked.metadata = serde_json::json!({"locked": true});
        store.put(locked).await.unwrap();
        let rep = run_backup(&store, &remote, &cfg(), PASS, cheap_kdf())
            .await
            .unwrap();
        assert!(!rep.complete && !rep.manifest_published);
        assert_eq!(rep.records_locked, 1);
        assert_eq!(remote.blobs.lock().unwrap()[&name], before);

        // With no previous complete backup, an incomplete one is still published.
        let remote2 = MemBlobs::default();
        let rep = run_backup(&store, &remote2, &cfg(), PASS, cheap_kdf())
            .await
            .unwrap();
        assert!(!rep.complete && rep.manifest_published);
    }

    #[tokio::test]
    async fn missing_packs_are_reuploaded_on_the_next_backup() {
        let dir = tempfile::tempdir().unwrap();
        let remote = MemBlobs::default();
        let store = two_pack_backup(&remote, dir.path()).await;
        let id = cfg().instance_id;
        let pack = remote
            .blobs
            .lock()
            .unwrap()
            .keys()
            .find(|k| !k.starts_with("xcb1-manifest-"))
            .cloned()
            .unwrap();
        remote.blobs.lock().unwrap().remove(&pack);
        assert!(!run_verify(&remote, &id, Some(PASS)).await.unwrap().ok);
        let dst_dir = tempfile::tempdir().unwrap();
        let dst = temp_store(dst_dir.path()).await;
        let e = run_restore(&dst, &remote, &id, PASS).await.unwrap_err();
        assert!(e.to_string().contains("missing"), "{e}");

        let rep = run_backup(&store, &remote, &cfg(), PASS, cheap_kdf())
            .await
            .unwrap();
        assert_eq!(rep.packs_repaired, 1);
        assert!(run_verify(&remote, &id, Some(PASS)).await.unwrap().ok);
    }

    #[test]
    fn bearer_token_requires_https_or_loopback() {
        for ok in [
            "https://cloud.example",
            "http://127.0.0.1:8787",
            "http://localhost:8787",
            "http://[::1]:8787",
        ] {
            assert!(ensure_token_transport(ok).is_ok(), "{ok}");
        }
        for bad in [
            "http://cloud.example",
            "http://10.0.0.5",
            "http://localhost.evil.example",
            "ftp://cloud.example",
            "not a url",
        ] {
            assert!(ensure_token_transport(bad).is_err(), "{bad}");
        }
        let c = CloudBackupConfig {
            url: "http://cloud.example".into(),
            token: "t".into(),
            instance_id: "i".into(),
        };
        assert!(HttpBlobStore::new(&c).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn passphrase_file_is_created_0600_and_loose_modes_are_refused() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pass");
        write_passphrase_file(&path, PASS).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(passphrase_from_file(&path).unwrap().as_str(), PASS);
        assert!(write_passphrase_file(&path, PASS).is_err(), "no overwrite");

        for mode in [0o640, 0o604, 0o644] {
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
            let e = passphrase_from_file(&path).unwrap_err();
            assert!(e.to_string().contains("other users"), "{mode:o}: {e}");
        }
    }

    #[tokio::test]
    async fn http_store_uses_native_routes_and_bearer_auth() {
        let mut server = mockito::Server::new_async().await;
        let put = server
            .mock("PUT", "/v1/cloud/chunks/abcdef0123")
            .match_header("authorization", "Bearer tok")
            .match_body(mockito::Matcher::Exact("blob".into()))
            .with_status(200)
            .with_body(r#"{"stored":true}"#)
            .create_async()
            .await;
        let missing = server
            .mock("GET", "/v1/cloud/chunks/nothere00")
            .with_status(404)
            .create_async()
            .await;
        let usage = server
            .mock("GET", "/v1/cloud/usage")
            .match_header("authorization", "Bearer tok")
            .with_status(200)
            .with_body(r#"{"tenant":"t1","bytes":42,"chunks":2,"quota":100,"percent":42.0}"#)
            .create_async()
            .await;
        let quota = server
            .mock("PUT", "/v1/cloud/chunks/full000000")
            .with_status(429)
            .with_body(r#"{"error":"quota_exceeded"}"#)
            .create_async()
            .await;

        let c = CloudBackupConfig {
            url: server.url(),
            token: "tok".into(),
            instance_id: "i".into(),
        };
        let http = HttpBlobStore::new(&c).unwrap();
        http.put("abcdef0123", b"blob").await.unwrap();
        assert!(http.get("nothere00").await.unwrap().is_none());
        let u = http.usage().await.unwrap();
        assert_eq!((u.bytes, u.chunks), (42, 2));
        let err = http.put("full000000", b"x").await.unwrap_err().to_string();
        assert!(
            err.contains("429") && err.contains("quota_exceeded"),
            "{err}"
        );
        put.assert_async().await;
        missing.assert_async().await;
        usage.assert_async().await;
        quota.assert_async().await;
    }
}
