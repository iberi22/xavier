//! One-way space links (WP-13n).
//!
//! A link lets space A READ space B's memory: B grants, A reads, never the
//! reverse. It is a [`CrossGrant`] stored in the GRANTOR's `espacio.sqlite`
//! (`links` table) with `target_node = "space:A"` and
//! `resource_id = "space:B/memory"`, plus an optional namespace and
//! path-prefix filter. Expiry and revocation come from
//! [`CrossGrant::is_active`]. Links are read-only by construction: nothing in
//! this module (or its callers) writes into the grantor's memory.

use anyhow::{anyhow, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::manager::SpaceManager;
use super::store::{validate_space_id, SpaceStore};
use crate::enterprise::rbac::Permission;
use crate::mesh::network::CrossGrant;

/// A link as stored: the grant plus its optional memory filter.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SpaceLink {
    #[serde(flatten)]
    pub grant: CrossGrant,
    /// Only records of this namespace (project) are visible through the link.
    #[serde(default)]
    pub namespace: Option<String>,
    /// Only records whose path starts with this prefix are visible.
    #[serde(default)]
    pub path_prefix: Option<String>,
}

fn ts(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Nanos, true)
}

fn parse_ts(s: &str) -> Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(s)
        .map_err(|e| anyhow!("bad timestamp {s}: {e}"))?
        .with_timezone(&Utc))
}

/// `target_node` value naming the space that may read.
pub fn link_target(grantee: &str) -> String {
    format!("space:{grantee}")
}

/// `resource_id` value naming the memory of the granting space.
pub fn link_resource(grantor: &str) -> String {
    format!("space:{grantor}/memory")
}

impl SpaceLink {
    /// True when this link, stored in `grantor`'s database, currently lets
    /// `caller` read `grantor`'s memory. Direction is checked on both ends: the
    /// resource must be the grantor's own memory and the target the caller.
    pub fn grants_read(&self, grantor: &str, caller: &str) -> bool {
        self.grant.target_node == link_target(caller)
            && self.grant.resource_id == link_resource(grantor)
            && self.grant.permission == Permission::Read
            && self.grant.is_active()
    }

    /// Whether a record is inside the link's filter.
    pub fn allows(&self, path: &str, metadata: &serde_json::Value) -> bool {
        if let Some(prefix) = &self.path_prefix {
            if !path.starts_with(prefix.as_str()) {
                return false;
            }
        }
        if let Some(ns) = &self.namespace {
            let rec = metadata
                .get("namespace")
                .unwrap_or(&serde_json::Value::Null);
            let ok = rec.as_str() == Some(ns.as_str())
                || rec.get("project").and_then(|v| v.as_str()) == Some(ns.as_str());
            if !ok {
                return false;
            }
        }
        true
    }
}

/// Parse every row of a `links` table. Rows that do not parse are dropped.
fn read_links(conn: &rusqlite::Connection) -> Result<Vec<SpaceLink>> {
    let mut stmt = conn.prepare(
        "SELECT id, resource_id, target_node, permission, expires_at, revoked,
                created_at, namespace, path_prefix
         FROM links ORDER BY created_at, id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, String>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, bool>(5)?,
            r.get::<_, String>(6)?,
            r.get::<_, Option<String>>(7)?,
            r.get::<_, Option<String>>(8)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, resource_id, target_node, perm, exp, revoked, created, namespace, prefix) = row?;
        // A row that does not parse is dropped: a link must never widen
        // access because of a damaged record.
        let (Ok(permission), Ok(created_at)) = (
            serde_json::from_value::<Permission>(serde_json::Value::String(perm)),
            parse_ts(&created),
        ) else {
            continue;
        };
        let expires_at = match exp {
            Some(e) => match parse_ts(&e) {
                Ok(t) => Some(t),
                Err(_) => continue,
            },
            None => None,
        };
        out.push(SpaceLink {
            grant: CrossGrant {
                id,
                resource_id,
                target_node,
                permission,
                expires_at,
                revoked,
                created_at,
            },
            namespace,
            path_prefix: prefix,
        });
    }
    Ok(out)
}

impl SpaceStore {
    pub fn insert_link(&self, l: &SpaceLink) -> Result<()> {
        self.lock().execute(
            "INSERT INTO links
               (id, resource_id, target_node, permission, expires_at, revoked,
                created_at, namespace, path_prefix)
             VALUES (?1, ?2, ?3, 'read', ?4, ?5, ?6, ?7, ?8)",
            params![
                l.grant.id,
                l.grant.resource_id,
                l.grant.target_node,
                l.grant.expires_at.map(ts),
                l.grant.revoked,
                ts(l.grant.created_at),
                l.namespace,
                l.path_prefix,
            ],
        )?;
        Ok(())
    }

    pub fn list_links(&self) -> Result<Vec<SpaceLink>> {
        read_links(&self.lock())
    }

    /// Revoke every active link whose grantee is `grantee`. Returns how many
    /// rows changed.
    pub fn revoke_links_to(&self, grantee: &str) -> Result<usize> {
        Ok(self.lock().execute(
            "UPDATE links SET revoked = 1 WHERE target_node = ?1 AND revoked = 0",
            params![link_target(grantee)],
        )?)
    }

    /// Revoke a link. Returns false when the id is unknown.
    pub fn revoke_link(&self, id: &str) -> Result<bool> {
        let n = self
            .lock()
            .execute("UPDATE links SET revoked = 1 WHERE id = ?1", params![id])?;
        Ok(n > 0)
    }

    pub fn get_link(&self, id: &str) -> Result<Option<SpaceLink>> {
        Ok(self.list_links()?.into_iter().find(|l| l.grant.id == id))
    }
}

/// Create a read link: `grantor` lets `grantee` read its memory.
pub async fn create_link(
    manager: &SpaceManager,
    grantor: &str,
    grantee: &str,
    namespace: Option<String>,
    path_prefix: Option<String>,
    expires_at: Option<DateTime<Utc>>,
) -> Result<SpaceLink> {
    validate_space_id(grantor)?;
    validate_space_id(grantee)?;
    if grantor == grantee {
        return Err(anyhow!("a space cannot link to itself"));
    }
    manager.get(grantor).await?;
    manager.get(grantee).await?;
    let link = SpaceLink {
        grant: CrossGrant {
            id: format!("lnk_{}", uuid::Uuid::new_v4().simple()),
            resource_id: link_resource(grantor),
            target_node: link_target(grantee),
            permission: Permission::Read,
            expires_at,
            revoked: false,
            created_at: Utc::now(),
        },
        namespace: namespace.filter(|s| !s.is_empty()),
        path_prefix: path_prefix.filter(|s| !s.is_empty()),
    };
    manager.stores().get(grantor)?.insert_link(&link)?;
    Ok(link)
}

/// Links stored in `grantor`'s database. A LOCKED grantor refuses store
/// access, but its `links` table is not encrypted: it is read through a
/// read-only connection so the caller can still be told the space is locked.
fn links_of(stores: &super::store::SpaceStores, grantor: &str) -> Option<Vec<SpaceLink>> {
    match stores.get(grantor) {
        Ok(store) => store.list_links().ok(),
        Err(_) if stores.key_ring().is_some_and(|r| r.is_locked(grantor)) => {
            let db = stores
                .spaces_dir()?
                .join(grantor)
                .join(super::store::DB_FILE);
            if !db.is_file() {
                return None;
            }
            let conn = rusqlite::Connection::open_with_flags(
                &db,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .ok()?;
            let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
            read_links(&conn).ok()
        }
        Err(_) => None,
    }
}

/// Active links that let `caller` read other spaces: `(grantor, link)`.
///
/// Only the caller's own id is consulted from the request; the grantors'
/// databases are scanned for grants naming it. Revoked, expired and
/// wrong-direction rows never appear. The scan does blocking sqlite work, so
/// it runs on the blocking pool, off the async workers.
pub async fn inbound_links(manager: &SpaceManager, caller: &str) -> Vec<(String, SpaceLink)> {
    let grantors: Vec<String> = manager
        .list()
        .await
        .into_iter()
        .map(|i| i.id)
        .filter(|id| id != caller)
        .collect();
    let stores = manager.stores();
    let caller = caller.to_string();
    tokio::task::spawn_blocking(move || {
        let mut out = Vec::new();
        for grantor in grantors {
            let Some(links) = links_of(&stores, &grantor) else {
                continue;
            };
            out.extend(
                links
                    .into_iter()
                    .filter(|l| l.grants_read(&grantor, &caller))
                    .map(|l| (grantor.clone(), l)),
            );
        }
        out
    })
    .await
    .unwrap_or_default()
}

/// Revoke every link, stored in any of `grantors`, that names `grantee`.
/// Called when `grantee` is deleted or trashed so a later space re-created
/// with the same id can never inherit its read access. Locked grantors are
/// handled through a direct connection (the `links` table is not encrypted).
/// Returns the number of links revoked. Blocking.
pub fn revoke_links_naming(
    stores: &super::store::SpaceStores,
    grantors: &[String],
    grantee: &str,
) -> usize {
    let mut n = 0;
    for grantor in grantors {
        if grantor == grantee {
            continue;
        }
        n += match stores.get(grantor) {
            Ok(store) => store.revoke_links_to(grantee).unwrap_or_else(|e| {
                tracing::error!("espacio: could not revoke links of {grantee} in {grantor}: {e}");
                0
            }),
            Err(_) if stores.key_ring().is_some_and(|r| r.is_locked(grantor)) => stores
                .spaces_dir()
                .map(|d| d.join(grantor).join(super::store::DB_FILE))
                .filter(|db| db.is_file())
                .and_then(|db| rusqlite::Connection::open(db).ok())
                .and_then(|conn| {
                    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
                    conn.execute(
                        "UPDATE links SET revoked = 1 WHERE target_node = ?1 AND revoked = 0",
                        params![link_target(grantee)],
                    )
                    .ok()
                })
                .unwrap_or(0),
            Err(_) => 0,
        };
    }
    n
}

/// Rows to fetch from a linked space for a page of `limit` results. Link
/// filters (namespace, path prefix) and the clearance ceiling are applied
/// AFTER the search, so fetching exactly `limit` would under-fill the page;
/// over-fetch 3x, capped.
pub fn linked_fetch_limit(limit: usize) -> usize {
    limit.saturating_mul(3).clamp(limit, limit.max(300))
}

/// A linked space whose memory can be searched read-only.
pub struct LinkedMemory {
    pub space_id: String,
    pub link: SpaceLink,
    pub ctx: crate::workspace::WorkspaceContext,
}

/// Result of resolving the linked spaces of a caller.
#[derive(Default)]
pub struct LinkedResolution {
    pub searchable: Vec<LinkedMemory>,
    /// Linked spaces that could not be searched (encrypted, locked, ...), as
    /// `{"space": id, "reason": "EncryptionPending"}`. Never an error.
    pub skipped: Vec<serde_json::Value>,
}

/// Resolve the memory of every space with an active inbound link to `caller`.
/// Encrypted spaces are skipped with an `EncryptionPending` note (their memory
/// store is not available until record encryption lands), locked spaces with
/// `Locked`; the rest of the query is unaffected.
pub async fn resolve_linked(manager: &SpaceManager, caller: &str) -> LinkedResolution {
    use crate::workspace::SpaceScopeError;
    let mut out = LinkedResolution::default();
    for (grantor, link) in inbound_links(manager, caller).await {
        match crate::workspace::space_workspace_context(manager, &grantor).await {
            Ok(ctx) => out.searchable.push(LinkedMemory {
                space_id: grantor,
                link,
                ctx,
            }),
            Err(e) => {
                let reason = match e {
                    SpaceScopeError::EncryptionPending => "EncryptionPending",
                    SpaceScopeError::Locked => "Locked",
                    SpaceScopeError::Unavailable => "Unavailable",
                };
                out.skipped
                    .push(serde_json::json!({"space": grantor, "reason": reason}));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(grantor: &str, grantee: &str) -> SpaceLink {
        SpaceLink {
            grant: CrossGrant {
                id: "lnk_t".into(),
                resource_id: link_resource(grantor),
                target_node: link_target(grantee),
                permission: Permission::Read,
                expires_at: None,
                revoked: false,
                created_at: Utc::now(),
            },
            namespace: None,
            path_prefix: None,
        }
    }

    use crate::espacio::manager::KeyOptions;
    use crate::espacio::{StaticNodeKek, UnlockMode};
    use std::sync::Arc;

    async fn mk(m: &SpaceManager, id: &str) {
        m.create(id.into(), id.into(), "".into(), "owner".into(), false)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn deleting_the_grantee_revokes_links_so_id_reuse_inherits_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let m = SpaceManager::open(tmp.path());
        mk(&m, "esp_a").await;
        mk(&m, "esp_b").await;
        create_link(&m, "esp_b", "esp_a", None, None, None)
            .await
            .unwrap();
        assert_eq!(inbound_links(&m, "esp_a").await.len(), 1);
        // A is trashed and an unrelated space takes the id.
        m.delete("esp_a").await.unwrap();
        mk(&m, "esp_a").await;
        assert!(
            inbound_links(&m, "esp_a").await.is_empty(),
            "re-created space inherited the old grantee's link"
        );
        // The row is revoked, not deleted (audit trail kept).
        let links = m.stores().get("esp_b").unwrap().list_links().unwrap();
        assert_eq!(links.len(), 1);
        assert!(links[0].grant.revoked);
    }

    #[tokio::test]
    async fn deleting_a_third_space_keeps_other_links() {
        let tmp = tempfile::tempdir().unwrap();
        let m = SpaceManager::open(tmp.path());
        for id in ["esp_a", "esp_b", "esp_c"] {
            mk(&m, id).await;
        }
        create_link(&m, "esp_b", "esp_a", None, None, None)
            .await
            .unwrap();
        m.delete("esp_c").await.unwrap();
        assert_eq!(inbound_links(&m, "esp_a").await.len(), 1);
    }

    #[tokio::test]
    async fn locked_grantor_is_reported_as_locked_not_silently_dropped() {
        let tmp = tempfile::tempdir().unwrap();
        let kek = || Arc::new(StaticNodeKek::new([6u8; 32]));
        {
            let m = SpaceManager::open_with_keys_and_legacy(tmp.path(), kek(), None);
            mk(&m, "esp_a").await;
            m.create_with_keys(
                "esp_b".into(),
                "b".into(),
                "".into(),
                "owner".into(),
                false,
                KeyOptions {
                    mode: UnlockMode::PasswordRequired,
                    password: Some("correct horse battery staple 42".into()),
                    encrypt_records: false,
                },
            )
            .await
            .unwrap();
            create_link(&m, "esp_b", "esp_a", None, None, None)
                .await
                .unwrap();
        }
        // Restart: the password_required grantor is locked again.
        let m = SpaceManager::open_with_keys_and_legacy(tmp.path(), kek(), None);
        assert!(m.key_ring().unwrap().is_locked("esp_b"));
        let r = resolve_linked(&m, "esp_a").await;
        assert!(r.searchable.is_empty());
        assert_eq!(
            r.skipped,
            vec![serde_json::json!({"space": "esp_b", "reason": "Locked"})]
        );
        // A space that was never linked learns nothing about the locked one.
        mk(&m, "esp_z").await;
        assert!(resolve_linked(&m, "esp_z").await.skipped.is_empty());
    }

    #[test]
    fn fetch_limit_overfetches_with_a_cap() {
        assert_eq!(linked_fetch_limit(10), 30);
        assert_eq!(linked_fetch_limit(100), 300);
        assert_eq!(linked_fetch_limit(1000), 1000, "never below the page");
    }

    #[test]
    fn direction_is_one_way() {
        let l = link("esp_b", "esp_a"); // B grants A
        assert!(l.grants_read("esp_b", "esp_a"));
        assert!(!l.grants_read("esp_a", "esp_b"), "reverse must not read");
        assert!(
            !l.grants_read("esp_b", "esp_c"),
            "third space must not read"
        );
        assert!(!l.grants_read("esp_c", "esp_a"), "wrong grantor");
    }

    #[test]
    fn revoked_and_expired_grant_nothing() {
        let mut l = link("esp_b", "esp_a");
        l.grant.revoked = true;
        assert!(!l.grants_read("esp_b", "esp_a"));
        let mut l = link("esp_b", "esp_a");
        l.grant.expires_at = Some(Utc::now() - chrono::Duration::seconds(5));
        assert!(!l.grants_read("esp_b", "esp_a"));
    }

    #[test]
    fn filters_apply() {
        let mut l = link("esp_b", "esp_a");
        l.path_prefix = Some("pub/".into());
        l.namespace = Some("proj".into());
        let meta = serde_json::json!({"namespace": {"project": "proj"}});
        assert!(l.allows("pub/x", &meta));
        assert!(!l.allows("priv/x", &meta));
        assert!(!l.allows(
            "pub/x",
            &serde_json::json!({"namespace": {"project": "other"}})
        ));
    }
}
