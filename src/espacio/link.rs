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
        let conn = self.lock();
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
            let (id, resource_id, target_node, perm, exp, revoked, created, namespace, prefix) =
                row?;
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

/// Active links that let `caller` read other spaces: `(grantor, link)`.
///
/// Only the caller's own id is consulted from the request; the grantors'
/// databases are scanned for grants naming it. Revoked, expired and
/// wrong-direction rows never appear.
pub async fn inbound_links(manager: &SpaceManager, caller: &str) -> Vec<(String, SpaceLink)> {
    let mut out = Vec::new();
    for info in manager.list().await {
        let grantor = info.id;
        if grantor == caller {
            continue;
        }
        let Ok(store) = manager.stores().get(&grantor) else {
            continue;
        };
        let Ok(links) = store.list_links() else {
            continue;
        };
        out.extend(
            links
                .into_iter()
                .filter(|l| l.grants_read(&grantor, caller))
                .map(|l| (grantor.clone(), l)),
        );
    }
    out
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
