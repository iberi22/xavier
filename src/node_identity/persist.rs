//! Persist sealed node vault + public identity under `XAVIER_DATA_DIR/node/`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

use super::derive::DerivedNodeKeys;
use super::vault::{OpenedVault, SealedVault, VaultError};
use super::{CheckCodes, SeedPhrase};

/// On-disk public identity (no secrets).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PublicNodeIdentity {
    pub version: u8,
    pub node_id: String,
    pub ed25519_public_hex: String,
    pub ml_dsa_commitment_hex: String,
    pub created_at: String,
}

impl PublicNodeIdentity {
    pub fn from_keys(keys: &DerivedNodeKeys) -> Self {
        Self {
            version: 1,
            node_id: keys.node_id.as_str().to_string(),
            ed25519_public_hex: crate::crypto::hex_encode(&keys.ed25519_public),
            ml_dsa_commitment_hex: crate::crypto::hex_encode(&keys.ml_dsa_commitment),
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// Paths for the SWAL Fase 0 node vault under the Xavier data dir.
#[derive(Clone, Debug)]
pub struct NodeStorePaths {
    pub root: PathBuf,
    pub vault: PathBuf,
    pub public_identity: PathBuf,
}

impl NodeStorePaths {
    pub fn from_data_dir(data_dir: impl AsRef<Path>) -> Self {
        let root = data_dir.as_ref().join("node");
        Self {
            vault: root.join("vault.json"),
            public_identity: root.join("identity.public.json"),
            root,
        }
    }

    pub fn default_from_env() -> Self {
        Self::from_data_dir(crate::settings::XavierSettings::resolve_data_dir())
    }
}

/// Load/save sealed vault + public identity.
pub struct NodeStore {
    pub paths: NodeStorePaths,
}

impl NodeStore {
    pub fn new(paths: NodeStorePaths) -> Self {
        Self { paths }
    }

    pub fn default_from_env() -> Self {
        Self::new(NodeStorePaths::default_from_env())
    }

    pub fn ensure_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.paths.root)
            .with_context(|| format!("create {}", self.paths.root.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.paths.root, fs::Permissions::from_mode(0o700));
        }
        Ok(())
    }

    pub fn vault_exists(&self) -> bool {
        self.paths.vault.exists()
    }

    pub fn save_vault(&self, vault: &SealedVault) -> Result<()> {
        self.ensure_dir()?;
        let json = serde_json::to_string_pretty(vault).context("serialize vault")?;
        write_private_file(&self.paths.vault, json.as_bytes())?;
        Ok(())
    }

    pub fn load_vault(&self) -> Result<SealedVault> {
        let raw = fs::read_to_string(&self.paths.vault)
            .with_context(|| format!("read vault {}", self.paths.vault.display()))?;
        serde_json::from_str(&raw).context("parse vault.json")
    }

    pub fn save_public_identity(&self, identity: &PublicNodeIdentity) -> Result<()> {
        self.ensure_dir()?;
        let json = serde_json::to_string_pretty(identity).context("serialize public identity")?;
        write_private_file(&self.paths.public_identity, json.as_bytes())?;
        Ok(())
    }

    pub fn load_public_identity(&self) -> Result<PublicNodeIdentity> {
        let raw = fs::read_to_string(&self.paths.public_identity).with_context(|| {
            format!(
                "read public identity {}",
                self.paths.public_identity.display()
            )
        })?;
        serde_json::from_str(&raw).context("parse identity.public.json")
    }

    /// Unlock vault and re-derive keys (for status --unlock / recover verification).
    pub fn unlock(
        &self,
        pin: &str,
        device_key: Option<&[u8; 32]>,
    ) -> Result<(OpenedVault, DerivedNodeKeys, CheckCodes), VaultError> {
        let vault = self.load_vault().map_err(|e| VaultError::Other(e))?;
        let opened = vault.unlock(pin, device_key)?;
        let phrase = SeedPhrase::from_entropy(
            &opened.entropy,
            if opened.passphrase.is_empty() {
                None
            } else {
                Some(opened.passphrase.as_str())
            },
        )
        .map_err(VaultError::Other)?;
        let keys =
            DerivedNodeKeys::from_seed_bytes(&phrase.seed_bytes).map_err(VaultError::Other)?;
        let codes = CheckCodes::from_seed_bytes(&phrase.seed_bytes);
        Ok((opened, keys, codes))
    }
}

fn write_private_file(path: &Path, bytes: &[u8]) -> Result<()> {
    fs::write(path, bytes).with_context(|| format!("write {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .with_context(|| format!("chmod 0600 {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::node_identity::NodeBootstrap;

    #[test]
    fn persist_roundtrip_create_load() {
        let dir = tempfile::tempdir().unwrap();
        let store = NodeStore::new(NodeStorePaths::from_data_dir(dir.path()));
        let bundle = NodeBootstrap::create(None, "112233", None).unwrap();
        let pub_id = PublicNodeIdentity::from_keys(&bundle.keys);
        store.save_vault(&bundle.vault).unwrap();
        store.save_public_identity(&pub_id).unwrap();

        assert!(store.vault_exists());
        let loaded_pub = store.load_public_identity().unwrap();
        assert_eq!(loaded_pub.node_id, pub_id.node_id);

        let (_opened, keys, _codes) = store.unlock("112233", None).unwrap();
        assert_eq!(keys.node_id.as_str(), pub_id.node_id);
        assert_eq!(keys.ed25519_public, bundle.keys.ed25519_public);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&store.paths.vault)
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn persist_recover_same_identity() {
        use crate::node_identity::{OrderMode, OrderedChallenge};

        let dir = tempfile::tempdir().unwrap();
        let store = NodeStore::new(NodeStorePaths::from_data_dir(dir.path()));
        let original = NodeBootstrap::create(Some("pass"), "111111", None).unwrap();
        let pub_before = PublicNodeIdentity::from_keys(&original.keys);

        let challenge = OrderedChallenge::new(OrderMode::Desc, &original.check_codes);
        let response = challenge.expected_response(&original.check_codes);
        let recovered = NodeBootstrap::recover_from_shares(
            &original.shares[0..2],
            Some("pass"),
            &response,
            &challenge,
            "222222",
            None,
        )
        .unwrap();

        store.save_vault(&recovered.vault).unwrap();
        store
            .save_public_identity(&PublicNodeIdentity::from_keys(&recovered.keys))
            .unwrap();

        let (_o, keys, _) = store.unlock("222222", None).unwrap();
        assert_eq!(keys.node_id.as_str(), pub_before.node_id);
        assert_eq!(keys.ml_dsa_commitment, original.keys.ml_dsa_commitment);
    }
}
