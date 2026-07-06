//! Memory use case orchestration
//!
//! Provides the implementation and data structures for this module's
//! responsibilities within the Xavier cognitive memory system.
use crate::domain::memory::{MemoryQueryFilters, MemoryRecord};
use crate::enterprise::rbac::{Permission, RoleGuard};
use crate::ports::inbound::MemoryQueryPort;
use crate::ports::outbound::ThreatDetectionPort;
use async_trait::async_trait;
use std::sync::Arc;
use tracing::warn;

pub struct MemoryUseCase {
    inner: Arc<dyn MemoryQueryPort>,
    threat_detector: Option<Arc<dyn ThreatDetectionPort>>,
    rbac_guard: Option<Arc<RoleGuard>>,
}

impl MemoryUseCase {
    pub fn new(
        inner: Arc<dyn MemoryQueryPort>,
        threat_detector: Option<Arc<dyn ThreatDetectionPort>>,
    ) -> Self {
        Self {
            inner,
            threat_detector,
            rbac_guard: None,
        }
    }

    pub fn with_rbac(mut self, guard: Arc<RoleGuard>) -> Self {
        self.rbac_guard = Some(guard);
        self
    }
}

#[async_trait]
impl MemoryQueryPort for MemoryUseCase {
    async fn search(
        &self,
        query: &str,
        limit: usize,
        filters: Option<MemoryQueryFilters>,
    ) -> anyhow::Result<Vec<MemoryRecord>> {
        if let Some(ref guard) = self.rbac_guard {
            // Scaffolding for RBAC check using new Permission model
            if !guard.can(&Permission::Read) {
                return Err(anyhow::anyhow!("Permission denied: Read"));
            }
        }
        if let Some(ref detector) = self.threat_detector {
            let clean = detector.scan_and_log(query, "memory_search").await?;
            if !clean {
                warn!("Memory search blocked: security threat detected in query");
                return Err(anyhow::anyhow!(
                    "Security policy violation detected in search query"
                ));
            }
        }
        self.inner.search(query, limit, filters).await
    }

    async fn add(&self, record: MemoryRecord) -> anyhow::Result<String> {
        if let Some(ref guard) = self.rbac_guard {
            if !guard.can(&Permission::Write) {
                return Err(anyhow::anyhow!("Permission denied: Write"));
            }
        }
        if let Some(ref detector) = self.threat_detector {
            let clean = detector.scan_and_log(&record.content, "memory_add").await?;
            if !clean {
                warn!("Memory add blocked: security threat detected in content");
                return Err(anyhow::anyhow!(
                    "Security policy violation detected in memory content"
                ));
            }
        }
        self.inner.add(record).await
    }

    async fn update(&self, id: &str, record: MemoryRecord) -> anyhow::Result<MemoryRecord> {
        if let Some(ref guard) = self.rbac_guard {
            if !guard.can(&Permission::Write) {
                return Err(anyhow::anyhow!("Permission denied: Write"));
            }
        }
        if let Some(ref detector) = self.threat_detector {
            let clean = detector
                .scan_and_log(&record.content, "memory_update")
                .await?;
            if !clean {
                warn!("Memory update blocked: security threat detected in content");
                return Err(anyhow::anyhow!(
                    "Security policy violation detected in memory content"
                ));
            }
        }
        self.inner.update(id, record).await
    }

    async fn delete(&self, id: &str) -> anyhow::Result<Option<MemoryRecord>> {
        if let Some(ref guard) = self.rbac_guard {
            guard
                .require(Permission::Delete)
                .map_err(|e| anyhow::anyhow!(e))?;
        }

        // HITL check for destructive actions
        if let Some(ref detector) = self.threat_detector {
            if detector.requires_hitl("memory_delete", id).await? {
                return Err(anyhow::anyhow!(
                    "Action requires human approval. Please provide an approval token."
                ));
            }
        }

        self.inner.delete(id).await
    }

    async fn get(&self, id: &str) -> anyhow::Result<Option<MemoryRecord>> {
        self.inner.get(id).await
    }

    async fn list(&self, workspace_id: &str, limit: usize) -> anyhow::Result<Vec<MemoryRecord>> {
        self.inner.list(workspace_id, limit).await
    }

    async fn export(&self, public_only: bool) -> anyhow::Result<Vec<MemoryRecord>> {
        self.inner.export(public_only).await
    }

    async fn ls(&self, path: &str) -> anyhow::Result<Vec<crate::memory::qmd::types::NavEntry>> {
        self.inner.ls(path).await
    }

    async fn expand_depth(
        &self,
        results: &[MemoryRecord],
        depth: usize,
        filters: Option<MemoryQueryFilters>,
    ) -> anyhow::Result<Vec<MemoryRecord>> {
        self.inner.expand_depth(results, depth, filters).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::schema::MemoryLevel;
    use chrono::Utc;

    struct MockMemoryPort;

    #[async_trait]
    impl MemoryQueryPort for MockMemoryPort {
        async fn search(
            &self,
            _query: &str,
            _limit: usize,
            _filters: Option<MemoryQueryFilters>,
        ) -> anyhow::Result<Vec<MemoryRecord>> {
            Ok(vec![])
        }
        async fn add(&self, record: MemoryRecord) -> anyhow::Result<String> {
            Ok(record.id)
        }
        async fn update(&self, _id: &str, record: MemoryRecord) -> anyhow::Result<MemoryRecord> {
            Ok(record)
        }
        async fn delete(&self, _id: &str) -> anyhow::Result<Option<MemoryRecord>> {
            Ok(None)
        }
        async fn get(&self, _id: &str) -> anyhow::Result<Option<MemoryRecord>> {
            Ok(None)
        }
        async fn list(
            &self,
            _workspace_id: &str,
            _limit: usize,
        ) -> anyhow::Result<Vec<MemoryRecord>> {
            Ok(vec![])
        }
        async fn export(&self, _public_only: bool) -> anyhow::Result<Vec<MemoryRecord>> {
            Ok(vec![])
        }
        async fn ls(&self, _path: &str) -> anyhow::Result<Vec<crate::memory::qmd::types::NavEntry>> {
            Ok(vec![])
        }
        async fn expand_depth(
            &self,
            results: &[MemoryRecord],
            _depth: usize,
            _filters: Option<MemoryQueryFilters>,
        ) -> anyhow::Result<Vec<MemoryRecord>> {
            Ok(results.to_vec())
        }
    }

    struct MockThreatDetector {
        should_clean: bool,
    }

    #[async_trait]
    impl ThreatDetectionPort for MockThreatDetector {
        async fn scan_and_log(&self, _text: &str, _component: &str) -> anyhow::Result<bool> {
            Ok(self.should_clean)
        }
    }

    #[tokio::test]
    async fn test_memory_usecase_search_clean() {
        let inner = Arc::new(MockMemoryPort);
        let detector = Arc::new(MockThreatDetector { should_clean: true });
        let usecase = MemoryUseCase::new(inner, Some(detector));

        let result = usecase.search("query", 10, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_memory_usecase_search_blocked() {
        let inner = Arc::new(MockMemoryPort);
        let detector = Arc::new(MockThreatDetector {
            should_clean: false,
        });
        let usecase = MemoryUseCase::new(inner, Some(detector));

        let result = usecase.search("bad query", 10, None).await;
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Security policy violation detected in search query"
        );
    }

    #[tokio::test]
    async fn test_memory_usecase_add_clean() {
        let inner = Arc::new(MockMemoryPort);
        let detector = Arc::new(MockThreatDetector { should_clean: true });
        let usecase = MemoryUseCase::new(inner, Some(detector));

        let record = MemoryRecord {
            id: "1".to_string(),
            score: 0.0,
            content: "clean content".to_string(),
            path: "test.txt".to_string(),
            workspace_id: "default".to_string(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            metadata: serde_json::json!({}),
            embedding: vec![],
            revision: 1,
            primary: true,
            parent_id: None,
            cluster_id: None,
            level: MemoryLevel::default(),
            relation: None,
            clearance: Default::default(),
            revisions: vec![],
            content_iv: None,
            encrypted_dek: None,
            metadata_iv: None,
        };

        let result = usecase.add(record).await;
        assert!(result.is_ok());
    }
}
