use crate::espacio::store::{SpaceStore, SpaceStores};
use chrono::{DateTime, TimeDelta, Utc};

/// Returns the cutoff time for message retention, or None if retention is disabled (0).
pub fn retention_cutoff(now: DateTime<Utc>, retention_days: u64) -> Option<DateTime<Utc>> {
    if retention_days == 0 {
        return None;
    }

    let days = i64::try_from(retention_days).unwrap_or(i64::MAX / 86400); // Prevent overflow
    let delta = TimeDelta::try_days(days)?;

    now.checked_sub_signed(delta)
}

/// Sweeps old messages from a single space store.
pub fn sweep_space(
    store: &SpaceStore,
    now: DateTime<Utc>,
    retention_days: u64,
) -> anyhow::Result<usize> {
    if let Some(cutoff) = retention_cutoff(now, retention_days) {
        store.delete_messages_before(cutoff)
    } else {
        Ok(0)
    }
}

/// Sweeps multiple spaces, skipping missing or locked ones.
pub fn sweep_spaces(
    stores: &SpaceStores,
    space_ids: &[String],
    now: DateTime<Utc>,
    retention_days: u64,
) -> Vec<(String, usize)> {
    let mut results = Vec::new();

    for id in space_ids {
        match stores.get_existing(id) {
            Ok(Some(store)) => match sweep_space(&store, now, retention_days) {
                Ok(deleted) => {
                    results.push((id.clone(), deleted));
                }
                Err(e) => {
                    tracing::warn!("Failed to sweep space {}: {}", id, e);
                }
            },
            Ok(None) => {
                tracing::warn!("Skipping sweep: space {} not found", id);
            }
            Err(e) => {
                tracing::warn!("Skipping sweep: space {} unavailable: {}", id, e);
            }
        }
    }

    results
}

/// Sweeps spaces with their own retention_days; `None` uses `fallback_days`.
pub fn sweep_spaces_per_space(
    stores: &SpaceStores,
    spaces: &[(String, Option<u64>)],
    now: DateTime<Utc>,
    fallback_days: u64,
) -> Vec<(String, usize)> {
    let mut results = Vec::new();
    for (id, days) in spaces {
        let days = days.unwrap_or(fallback_days);
        results.extend(sweep_spaces(stores, std::slice::from_ref(id), now, days));
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeDelta;
    use rusqlite::{params, TransactionBehavior};

    #[test]
    fn test_retention_cutoff_zero_and_valid() {
        let now = Utc::now();
        assert_eq!(retention_cutoff(now, 0), None);

        let cutoff = retention_cutoff(now, 5).unwrap();
        let expected = now
            .checked_sub_signed(TimeDelta::try_days(5).unwrap())
            .unwrap();
        assert_eq!(cutoff, expected);
    }

    #[tokio::test]
    async fn test_sweep_space_deletes_old_but_preserves_head() {
        let stores = SpaceStores::in_memory();
        let store = stores.get("sp_123").unwrap();

        // 1. Append messages
        store.append_message("sp_123", "alice", "msg 1").unwrap();
        store.append_message("sp_123", "alice", "msg 2").unwrap();
        store.append_message("sp_123", "alice", "msg 3").unwrap();

        let now = Utc::now();
        let old_time = now
            .checked_sub_signed(TimeDelta::try_days(10).unwrap())
            .unwrap();

        // Use the internal timestamp format exactly as `store.rs` does for `ts()`
        let old_ts = old_time.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);

        // 2. Manually backdate the first two messages
        {
            let mut conn = store.lock();
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            tx.execute(
                "UPDATE messages SET created_at = ?1 WHERE seq < 2",
                params![old_ts],
            )
            .unwrap();
            tx.commit().unwrap();
        }

        // 3. Sweep with 5 days retention. Should delete msg 1 and msg 2, keep msg 3.
        let deleted = sweep_space(&store, now, 5).unwrap();
        assert_eq!(deleted, 2);

        let messages = store.messages_since(None).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].seq, 2); // seq is 0-indexed, so msg 3 has seq 2

        // 4. Test max-row guard: if all messages are old, it still keeps the latest one.
        {
            let mut conn = store.lock();
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            tx.execute("UPDATE messages SET created_at = ?1", params![old_ts])
                .unwrap();
            tx.commit().unwrap();
        }

        let deleted2 = sweep_space(&store, now, 5).unwrap();
        assert_eq!(deleted2, 0); // Preserves max-row

        let messages = store.messages_since(None).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].seq, 2);
    }

    #[tokio::test]
    async fn test_sweep_space_zero_days() {
        let stores = SpaceStores::in_memory();
        let store = stores.get("sp_123").unwrap();

        store.append_message("sp_123", "alice", "msg 1").unwrap();

        let deleted = sweep_space(&store, Utc::now(), 0).unwrap();
        assert_eq!(deleted, 0);
    }

    #[tokio::test]
    async fn test_sweep_spaces_ignores_missing() {
        let stores = SpaceStores::in_memory();
        let _store = stores.get("sp_existing").unwrap();

        let results = sweep_spaces(
            &stores,
            &["sp_missing".to_string(), "sp_existing".to_string()],
            Utc::now(),
            5,
        );

        // Should skip missing and succeed on existing
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, "sp_existing");
    }

    fn backdate(store: &SpaceStore, days: i64, upto_seq: i64) {
        let ts = (Utc::now() - TimeDelta::try_days(days).unwrap())
            .to_rfc3339_opts(chrono::SecondsFormat::Nanos, true);
        let conn = store.lock();
        conn.execute(
            "UPDATE messages SET created_at = ?1 WHERE seq < ?2",
            params![ts, upto_seq],
        )
        .unwrap();
    }

    #[tokio::test]
    async fn test_per_space_retention_sweeps_each_by_its_own_days() {
        let stores = SpaceStores::in_memory();
        for id in ["sp_a", "sp_b", "sp_c"] {
            let s = stores.get(id).unwrap();
            for m in ["1", "2", "3"] {
                s.append_message(id, "alice", m).unwrap();
            }
            // Two rows are 10 days old, the head stays fresh.
            backdate(&s, 10, 2);
        }
        let spaces = vec![
            ("sp_a".to_string(), Some(5)),  // 10d > 5d: swept
            ("sp_b".to_string(), Some(30)), // 10d < 30d: kept
            ("sp_c".to_string(), None),     // fallback 0: disabled
        ];
        let res = sweep_spaces_per_space(&stores, &spaces, Utc::now(), 0);
        let get = |id: &str| res.iter().find(|r| r.0 == id).map(|r| r.1);
        assert_eq!(get("sp_a"), Some(2));
        assert_eq!(get("sp_b"), Some(0));
        assert_eq!(get("sp_c"), Some(0));
        assert_eq!(
            stores
                .get("sp_a")
                .unwrap()
                .messages_since(None)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            stores
                .get("sp_b")
                .unwrap()
                .messages_since(None)
                .unwrap()
                .len(),
            3
        );
        assert_eq!(
            stores
                .get("sp_c")
                .unwrap()
                .messages_since(None)
                .unwrap()
                .len(),
            3
        );
    }

    #[tokio::test]
    async fn test_space_without_retention_uses_global_fallback() {
        let stores = SpaceStores::in_memory();
        let s = stores.get("sp_d").unwrap();
        for m in ["1", "2", "3"] {
            s.append_message("sp_d", "alice", m).unwrap();
        }
        backdate(&s, 10, 2);
        let res = sweep_spaces_per_space(&stores, &[("sp_d".to_string(), None)], Utc::now(), 5);
        assert_eq!(res, vec![("sp_d".to_string(), 2)]);
    }
}
