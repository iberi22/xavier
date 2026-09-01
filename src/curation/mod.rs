use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum CurationStatus {
    Pending,
    Approved,
    Rejected { reason: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CurationItem {
    pub id: String,
    pub content_ref: String,
    pub proposed_clearance: String,
    pub status: CurationStatus,
    pub curated_by: Option<String>,
    pub curated_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classification: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CurationHistoryEntry {
    pub who: String,
    pub what: String,
    pub when: DateTime<Utc>,
    pub classification: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CurationQueue {
    pub items: Vec<CurationItem>,
    #[serde(skip)]
    pub file_path: Option<PathBuf>,
    #[serde(skip)]
    pub history_path: Option<PathBuf>,
}

impl CurationQueue {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            file_path: Some(PathBuf::from("data/curation/queue.json")),
            history_path: Some(PathBuf::from("data/curation/history.json")),
        }
    }

    pub fn new_with_path(path: PathBuf) -> Self {
        let history_path = path
            .parent()
            .unwrap_or_else(|| Path::new("data/curation"))
            .join("history.json");
        Self {
            items: Vec::new(),
            file_path: Some(path),
            history_path: Some(history_path),
        }
    }

    pub fn load() -> Result<Self, String> {
        Self::load_from_path(Path::new("data/curation/queue.json"))
    }

    pub fn load_from_path(path: &Path) -> Result<Self, String> {
        let history_path = path
            .parent()
            .unwrap_or_else(|| Path::new("data/curation"))
            .join("history.json");
        if !path.exists() {
            return Ok(Self {
                items: Vec::new(),
                file_path: Some(path.to_path_buf()),
                history_path: Some(history_path),
            });
        }
        let data = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let items: Vec<CurationItem> = serde_json::from_str(&data).map_err(|e| e.to_string())?;
        Ok(Self {
            items,
            file_path: Some(path.to_path_buf()),
            history_path: Some(history_path),
        })
    }

    pub fn save(&self) -> Result<(), String> {
        let path = self
            .file_path
            .as_deref()
            .unwrap_or_else(|| Path::new("data/curation/queue.json"));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let data = serde_json::to_string_pretty(&self.items).map_err(|e| e.to_string())?;
        std::fs::write(path, data).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn submit_for_curation(
        &mut self,
        content_ref: String,
        proposed_clearance: String,
        source: Option<String>,
    ) -> CurationItem {
        let id = ulid::Ulid::new().to_string();
        let item = CurationItem {
            id,
            content_ref,
            proposed_clearance,
            status: CurationStatus::Pending,
            curated_by: None,
            curated_at: None,
            source,
            classification: None,
        };
        self.items.push(item.clone());
        item
    }

    pub fn record_history_entry(&self, entry: &CurationHistoryEntry) -> Result<(), String> {
        let path = self
            .history_path
            .clone()
            .unwrap_or_else(|| PathBuf::from("data/curation/history.json"));
        Self::record_history_entry_to_path(&path, entry)
    }

    pub fn record_history_entry_to_path(
        path: &Path,
        entry: &CurationHistoryEntry,
    ) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut history = Self::load_history_from_path(path).unwrap_or_default();
        history.push(entry.clone());
        let data = serde_json::to_string_pretty(&history).map_err(|e| e.to_string())?;
        std::fs::write(path, data).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn load_history_from_path(path: &Path) -> Result<Vec<CurationHistoryEntry>, String> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let data = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let history: Vec<CurationHistoryEntry> =
            serde_json::from_str(&data).map_err(|e| e.to_string())?;
        Ok(history)
    }

    pub fn load_history(&self) -> Result<Vec<CurationHistoryEntry>, String> {
        let path = self
            .history_path
            .clone()
            .unwrap_or_else(|| PathBuf::from("data/curation/history.json"));
        Self::load_history_from_path(&path)
    }

    pub fn list_pending(&self) -> Vec<CurationItem> {
        self.pending_items()
    }

    pub fn approve(
        &mut self,
        id: &str,
        curator: String,
        classification: Option<String>,
        clearance: Option<String>,
    ) -> Result<CurationItem, String> {
        let history_path = self
            .history_path
            .clone()
            .unwrap_or_else(|| PathBuf::from("data/curation/history.json"));

        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            let now = Utc::now();
            item.status = CurationStatus::Approved;
            item.curated_by = Some(curator.clone());
            item.curated_at = Some(now);
            if let Some(cls) = classification {
                item.classification = Some(cls);
            }
            if let Some(clr) = clearance {
                item.proposed_clearance = clr;
            }

            let entry = CurationHistoryEntry {
                who: curator,
                what: format!("Approved item {}", id),
                when: now,
                classification: item.classification.clone(),
            };
            let ret = item.clone();
            let _ = Self::record_history_entry_to_path(&history_path, &entry);

            Ok(ret)
        } else {
            Err(format!("Item with id {} not found", id))
        }
    }

    pub fn reject(
        &mut self,
        id: &str,
        curator: String,
        reason: String,
    ) -> Result<CurationItem, String> {
        let history_path = self
            .history_path
            .clone()
            .unwrap_or_else(|| PathBuf::from("data/curation/history.json"));

        if let Some(item) = self.items.iter_mut().find(|i| i.id == id) {
            let now = Utc::now();
            item.status = CurationStatus::Rejected {
                reason: reason.clone(),
            };
            item.curated_by = Some(curator.clone());
            item.curated_at = Some(now);

            let entry = CurationHistoryEntry {
                who: curator,
                what: format!("Rejected item {}: {}", id, reason),
                when: now,
                classification: item.classification.clone(),
            };
            let ret = item.clone();
            let _ = Self::record_history_entry_to_path(&history_path, &entry);

            Ok(ret)
        } else {
            Err(format!("Item with id {} not found", id))
        }
    }

    pub fn pending_items(&self) -> Vec<CurationItem> {
        self.items
            .iter()
            .filter(|i| matches!(i.status, CurationStatus::Pending))
            .cloned()
            .collect()
    }

    pub fn curated_items(&self) -> Vec<CurationItem> {
        self.items
            .iter()
            .filter(|i| matches!(i.status, CurationStatus::Approved))
            .cloned()
            .collect()
    }

    pub fn curated_dataset(&self) -> Vec<CurationItem> {
        self.curated_items()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_submit_for_curation() {
        let mut queue = CurationQueue::new();
        let item = queue.submit_for_curation(
            "doc-123".to_string(),
            "confidential".to_string(),
            Some("session".to_string()),
        );
        assert_eq!(item.content_ref, "doc-123");
        assert_eq!(item.proposed_clearance, "confidential");
        assert_eq!(item.source, Some("session".to_string()));
        assert_eq!(item.status, CurationStatus::Pending);
        assert_eq!(queue.items.len(), 1);
    }

    #[test]
    fn test_approve_item() {
        let mut queue = CurationQueue::new();
        let item = queue.submit_for_curation(
            "doc-123".to_string(),
            "confidential".to_string(),
            Some("import".to_string()),
        );
        let approved = queue
            .approve(
                &item.id,
                "alice".to_string(),
                Some("internal".to_string()),
                Some("restricted".to_string()),
            )
            .unwrap();
        assert_eq!(approved.status, CurationStatus::Approved);
        assert_eq!(approved.curated_by, Some("alice".to_string()));
        assert_eq!(approved.classification, Some("internal".to_string()));
        assert_eq!(approved.proposed_clearance, "restricted");
        assert!(approved.curated_at.is_some());
    }

    #[test]
    fn test_reject_item() {
        let mut queue = CurationQueue::new();
        let item =
            queue.submit_for_curation("doc-123".to_string(), "confidential".to_string(), None);
        let rejected = queue
            .reject(&item.id, "bob".to_string(), "offensive content".to_string())
            .unwrap();
        assert_eq!(
            rejected.status,
            CurationStatus::Rejected {
                reason: "offensive content".to_string()
            }
        );
        assert_eq!(rejected.curated_by, Some("bob".to_string()));
    }

    #[test]
    fn test_approve_nonexistent() {
        let mut queue = CurationQueue::new();
        let res = queue.approve("invalid-id", "alice".to_string(), None, None);
        assert!(res.is_err());
    }

    #[test]
    fn test_reject_nonexistent() {
        let mut queue = CurationQueue::new();
        let res = queue.reject("invalid-id", "bob".to_string(), "reason".to_string());
        assert!(res.is_err());
    }

    #[test]
    fn test_curated_dataset_only_approved_are_eligible() {
        let mut queue = CurationQueue::new();
        let item1 = queue.submit_for_curation(
            "doc-1".to_string(),
            "public".to_string(),
            Some("agent".to_string()),
        );
        let item2 = queue.submit_for_curation(
            "doc-2".to_string(),
            "secret".to_string(),
            Some("import".to_string()),
        );
        let _item3 = queue.submit_for_curation(
            "doc-3".to_string(),
            "internal".to_string(),
            Some("session".to_string()),
        );

        queue
            .approve(
                &item1.id,
                "alice".to_string(),
                Some("public_data".to_string()),
                None,
            )
            .unwrap();
        queue
            .reject(&item2.id, "bob".to_string(), "bad format".to_string())
            .unwrap();

        let eligible = queue.curated_dataset();
        assert_eq!(eligible.len(), 1);
        assert_eq!(eligible[0].id, item1.id);
        assert_eq!(eligible[0].status, CurationStatus::Approved);
        assert_eq!(eligible[0].classification, Some("public_data".to_string()));

        let pending = queue.pending_items();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].content_ref, "doc-3");
    }

    #[test]
    fn test_save_and_load_flow() {
        let file = NamedTempFile::new().unwrap();
        let mut queue = CurationQueue::new_with_path(file.path().to_path_buf());
        let item = queue.submit_for_curation(
            "ref-789".to_string(),
            "secret".to_string(),
            Some("agent".to_string()),
        );
        queue.save().unwrap();

        let loaded = CurationQueue::load_from_path(file.path()).unwrap();
        assert_eq!(loaded.items.len(), 1);
        assert_eq!(loaded.items[0].id, item.id);
        assert_eq!(loaded.items[0].content_ref, "ref-789");
        assert_eq!(loaded.items[0].source, Some("agent".to_string()));
    }

    #[test]
    fn test_load_nonexistent_returns_empty() {
        let path = Path::new("nonexistent_path_curation_queue.json");
        let queue = CurationQueue::load_from_path(path).unwrap();
        assert!(queue.items.is_empty());
    }
}
