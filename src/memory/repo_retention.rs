use crate::memory::sqlite_vec_store::VecSqliteMemoryStore;
use crate::memory::store::MemoryStore;
use chrono::{DateTime, TimeDelta, Utc};
use rusqlite::params;

pub fn retention_cutoff(now: DateTime<Utc>, retention_days: Option<u64>) -> Option<DateTime<Utc>> {
    let days = retention_days?;
    if days == 0 {
        return None;
    }

    // Convert u64 to i64 safely
    let duration = TimeDelta::try_days(days as i64)?;
    now.checked_sub_signed(duration)
}

pub async fn sweep_repo_store(
    store: &VecSqliteMemoryStore,
    workspace_id: &str,
    retention_days: Option<u64>,
    now: DateTime<Utc>,
) -> anyhow::Result<usize> {
    let Some(cutoff) = retention_cutoff(now, retention_days) else {
        return Ok(0);
    };

    let cutoff_str = cutoff.to_rfc3339();
    let workspace_id_str = workspace_id.to_string();

    let ids_to_delete: Vec<String> = store
        .conn_provider
        .with_conn(store.connection_project_id(), move |conn| {
            let mut stmt = conn.prepare(
                "SELECT id FROM memory_records WHERE workspace_id = ? AND created_at < ?",
            )?;
            let ids = stmt
                .query_map(params![workspace_id_str, cutoff_str], |row| {
                    row.get::<_, String>(0)
                })?
                .collect::<rusqlite::Result<Vec<String>>>()?;
            Ok(ids)
        })
        .await?;

    let mut deleted_count = 0;
    for id in ids_to_delete {
        store.delete(workspace_id, &id).await?;
        deleted_count += 1;
    }

    Ok(deleted_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::connection_provider::InMemoryProvider;
    use crate::memory::sqlite_vec_store::VecSqliteStoreConfig;
    use crate::memory::store::MemoryRecord;
    use std::sync::Arc;
    use tokio;

    xavier::isolate_test_process!();

    #[test]
    fn test_retention_cutoff_none_or_zero() {
        let now = Utc::now();
        assert_eq!(retention_cutoff(now, None), None);
        assert_eq!(retention_cutoff(now, Some(0)), None);
    }

    #[test]
    fn test_retention_cutoff_valid() {
        let now = Utc::now();
        let cutoff = retention_cutoff(now, Some(5)).unwrap();
        assert_eq!(now.signed_duration_since(cutoff).num_days(), 5);
    }

    #[tokio::test]
    async fn test_sweep_repo_store_no_retention() {
        let temp = tempfile::tempdir().unwrap();
        let store = VecSqliteMemoryStore::new_with_provider(
            VecSqliteStoreConfig {
                path: temp.path().join("memory.sqlite3"),
                embedding_dimensions: 768,
            },
            Arc::new(InMemoryProvider::new()),
        )
        .await
        .unwrap();

        let count = sweep_repo_store(&store, "ws", None, Utc::now())
            .await
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn test_sweep_repo_store_with_retention() {
        let temp = tempfile::tempdir().unwrap();
        let store = VecSqliteMemoryStore::new_with_provider(
            VecSqliteStoreConfig {
                path: temp.path().join("memory.sqlite3"),
                embedding_dimensions: 768,
            },
            Arc::new(InMemoryProvider::new()),
        )
        .await
        .unwrap();

        // Insert some test records
        for i in 1..=3 {
            store
                .put(MemoryRecord {
                    id: format!("record-{}", i),
                    workspace_id: "test-ws".into(),
                    path: format!("path/{}", i),
                    content: "content".into(),
                    embedding: vec![0.1; 768],
                    ..Default::default()
                })
                .await
                .unwrap();
        }

        // Backdate two of them
        let old_date = (Utc::now() - TimeDelta::try_days(10).unwrap()).to_rfc3339();
        store
            .conn_provider
            .with_conn(store.connection_project_id(), move |conn| {
                conn.execute(
                    "UPDATE memory_records SET created_at = ? WHERE id IN ('record-1', 'record-2')",
                    params![old_date],
                )?;
                Ok(())
            })
            .await
            .unwrap();

        let count = sweep_repo_store(&store, "test-ws", Some(5), Utc::now())
            .await
            .unwrap();
        assert_eq!(count, 2);

        // Verify they are deleted
        let remaining = store.list("test-ws").await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, "record-3");
    }
}
