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
        filters: Option<MemoryQueryFilters>,
    ) -> anyhow::Result<Vec<MemoryRecord>> {
        if let Some(ref guard) = self.rbac_guard {
            // Prefer granular permission if workspace/agent context is available
            let perm = if let Some(ref f) = filters {
                Permission::AgentMemoryRead(f.workspace_id.clone().unwrap_or("*".to_string()))
            } else {
                Permission::MemoryRead
            };

            if !guard.can(&perm) && !guard.can(&Permission::MemoryRead) {
                return Err(anyhow::anyhow!("Permission denied: {}", perm));
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
        self.inner.search(query, filters).await
    }

    async fn add(&self, record: MemoryRecord) -> anyhow::Result<String> {
        if let Some(ref guard) = self.rbac_guard {
            let perm = Permission::AgentMemoryWrite(record.workspace_id.clone());
            if !guard.can(&perm) && !guard.can(&Permission::MemoryWrite) {
                return Err(anyhow::anyhow!("Permission denied: {}", perm));
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

    async fn delete(&self, id: &str) -> anyhow::Result<Option<MemoryRecord>> {
        if let Some(ref guard) = self.rbac_guard {
            // For delete, we still use MemoryDelete as a baseline,
            // but we could also check AgentMemoryWrite for the workspace if we had it.
            guard
                .require(Permission::MemoryDelete)
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
            _filters: Option<MemoryQueryFilters>,
        ) -> anyhow::Result<Vec<MemoryRecord>> {
            Ok(vec![])
        }
        async fn add(&self, record: MemoryRecord) -> anyhow::Result<String> {
            Ok(record.id)
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

        let result = usecase.search("query", None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_memory_usecase_search_blocked() {
        let inner = Arc::new(MockMemoryPort);
        let detector = Arc::new(MockThreatDetector {
            should_clean: false,
        });
        let usecase = MemoryUseCase::new(inner, Some(detector));

        let result = usecase.search("bad query", None).await;
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
            revisions: vec![],
        };

        let result = usecase.add(record).await;
        assert!(result.is_ok());
    }
}
