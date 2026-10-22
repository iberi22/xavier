use rusqlite::{Connection, OpenFlags};
use std::path::Path;
use std::sync::Mutex;

/// Result of a replay store claim.
#[derive(Debug, PartialEq, Eq)]
pub enum Claim {
    /// The nonce is fresh and was successfully durably stored.
    Fresh,
    /// The nonce has already been seen (exact same sender and nonce).
    Replayed,
    /// The request is stale (timestamp too old).
    Stale,
    /// The maximum capacity for this sender has been reached.
    Full,
}

/// A storage primitive to ensure replay requests are rejected and new ones recorded durably.
pub trait ReplayStore: Send + Sync {
    /// Attempt to claim a sender/nonce pair.
    ///
    /// This method is synchronous.
    ///
    /// # Arguments
    /// * `sender` - The identity of the sender (max 128 bytes UTF-8).
    /// * `nonce` - The unique nonce for the request (max 64 bytes UTF-8).
    /// * `ts` - The signed timestamp of the request.
    /// * `now` - The current timestamp for validating skew.
    ///
    /// # Returns
    /// An error on database failure, or a `Claim` variant indicating the outcome.
    fn claim(&self, sender: &str, nonce: &str, ts: i64, now: i64) -> anyhow::Result<Claim>;
}

/// SQLite-backed implementation of `ReplayStore`.
pub struct SqliteReplayStore {
    conn: Mutex<Connection>,
}

impl SqliteReplayStore {
    /// Create or open a replay store database at the specified path.
    ///
    /// The database will use WAL mode, FULL synchronous, and a busy timeout.
    pub fn open<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let conn = Connection::open(path)?;

        conn.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = FULL;
            PRAGMA busy_timeout = 5000;

            CREATE TABLE IF NOT EXISTS seen (
                sender TEXT NOT NULL,
                nonce TEXT NOT NULL,
                ts INTEGER NOT NULL,
                PRIMARY KEY (sender, nonce)
            );

            CREATE TABLE IF NOT EXISTS meta (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                floor INTEGER NOT NULL
            );

            INSERT OR IGNORE INTO meta (id, floor) VALUES (1, 0);
            ",
        )?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

impl ReplayStore for SqliteReplayStore {
    fn claim(&self, sender: &str, nonce: &str, ts: i64, now: i64) -> anyhow::Result<Claim> {
        if sender.is_empty() || sender.len() > 128 {
            anyhow::bail!("invalid sender");
        }
        if nonce.is_empty() || nonce.len() > 64 {
            anyhow::bail!("invalid nonce");
        }

        let skew = now.abs_diff(ts);
        if skew > 300 {
            return Ok(Claim::Stale);
        }

        let mut conn = self
            .conn
            .lock()
            .map_err(|_| anyhow::anyhow!("mutex poisoned"))?;
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;

        // 1. Get current floor
        let current_floor: i64 =
            tx.query_row("SELECT floor FROM meta WHERE id = 1", [], |row| row.get(0))?;

        // 2. Advance floor with saturation if needed
        let new_floor_base = now.saturating_sub(301); // max(oldfloor, now-301)
        let effective_floor = current_floor.max(new_floor_base);

        // Reject stale against effective floor
        if ts < effective_floor {
            return Ok(Claim::Stale);
        }

        // 3. Check for exact Replayed (repeated sender,nonce) even if different timestamp, while retained
        let count: i64 = tx.query_row(
            "SELECT COUNT(1) FROM seen WHERE sender = ?1 AND nonce = ?2",
            rusqlite::params![sender, nonce],
            |row| row.get(0),
        )?;

        if count > 0 {
            return Ok(Claim::Replayed);
        }

        // 4. Update floor if changed and delete old rows before enforcing capacity limits
        if effective_floor > current_floor {
            tx.execute(
                "UPDATE meta SET floor = ?1 WHERE id = 1",
                rusqlite::params![effective_floor],
            )?;
            tx.execute(
                "DELETE FROM seen WHERE ts < ?1",
                rusqlite::params![effective_floor],
            )?;
        }

        // 5. Enforce max entries per sender
        let sender_count: i64 = tx.query_row(
            "SELECT COUNT(1) FROM seen WHERE sender = ?1",
            rusqlite::params![sender],
            |row| row.get(0),
        )?;

        if sender_count >= 10000 {
            return Ok(Claim::Full);
        }

        // 6. Insert new record
        tx.execute(
            "INSERT INTO seen (sender, nonce, ts) VALUES (?1, ?2, ?3)",
            rusqlite::params![sender, nonce, ts],
        )?;

        tx.commit()?;

        Ok(Claim::Fresh)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use tempfile::NamedTempFile;

    #[test]
    fn replay_same_second_after_reopen_rejected() {
        let temp_db = NamedTempFile::new().unwrap();
        let path = temp_db.path().to_str().unwrap();

        let now = 1000000;

        {
            let store = SqliteReplayStore::open(path).unwrap();
            let res = store.claim("s1", "n1", now, now).unwrap();
            assert_eq!(res, Claim::Fresh);

            let res_same = store.claim("s1", "n1", now, now).unwrap();
            assert_eq!(res_same, Claim::Replayed);
        }

        {
            let store = SqliteReplayStore::open(path).unwrap();
            let res_reopen = store.claim("s1", "n1", now, now).unwrap();
            assert_eq!(res_reopen, Claim::Replayed);
        }
    }

    #[test]
    fn replay_sender_ahead_ts_plus_200_after_reopen_rejected() {
        let temp_db = NamedTempFile::new().unwrap();
        let path = temp_db.path().to_str().unwrap();

        let now = 1000000;

        {
            let store = SqliteReplayStore::open(path).unwrap();
            let res = store.claim("s1", "n1", now + 200, now).unwrap();
            assert_eq!(res, Claim::Fresh);
        }

        {
            let store = SqliteReplayStore::open(path).unwrap();
            let res_reopen = store.claim("s1", "n1", now + 200, now).unwrap();
            assert_eq!(res_reopen, Claim::Replayed);
        }
    }

    #[test]
    fn floor_only_rises_and_pruned_rows_stay_rejected() {
        let temp_db = NamedTempFile::new().unwrap();
        let path = temp_db.path().to_str().unwrap();

        let now = 1000000;
        {
            let store = SqliteReplayStore::open(path).unwrap();

            // ts must be >= now - 300 to not be stale immediately.
            // effective_floor = max(0, 1000000 - 301) = 999699
            let res = store.claim("s1", "n1", now, now).unwrap();
            assert_eq!(res, Claim::Fresh);

            // Fast forward clock
            let future = now + 1000; // effective_floor will be 1000699.
            let future_ts = future;
            let res_future = store.claim("s2", "n2", future_ts, future).unwrap();
            assert_eq!(res_future, Claim::Fresh);
        }

        // At this point, the old row n1 with ts = 1000000 should be pruned because ts < 1000699
        // A rollback of the clock should not lower the floor. We use rollback_now = now (skew 0).
        let rollback_now = now;

        // Reopen store before replay
        {
            let store = SqliteReplayStore::open(path).unwrap();
            let res_rollback = store.claim("s1", "n1", now, rollback_now).unwrap();
            // Since n1 was deleted but the floor remains at 1000699, ts=1000000 < floor, so it should be rejected as Stale.
            // It MUST fail if floor rejection is removed (which it isn't).
            assert_eq!(res_rollback, Claim::Stale);
        }

        // Verify via raw SQLite that the floor is indeed 1000699 and the row was pruned
        let conn = rusqlite::Connection::open(path).unwrap();
        let floor: i64 = conn
            .query_row("SELECT floor FROM meta WHERE id = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(floor, 1000699);

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(1) FROM seen WHERE sender = 's1' AND nonce = 'n1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn sender_nonce_namespace_is_isolated() {
        let temp_db = NamedTempFile::new().unwrap();
        let path = temp_db.path().to_str().unwrap();
        let store = SqliteReplayStore::open(path).unwrap();
        let now = 1000000;

        let res1 = store.claim("senderA", "nonce1", now, now).unwrap();
        assert_eq!(res1, Claim::Fresh);

        let res2 = store.claim("senderB", "nonce1", now, now).unwrap();
        assert_eq!(res2, Claim::Fresh);
    }

    #[test]
    fn repeated_nonce_with_new_timestamp_is_rejected_while_retained() {
        let temp_db = NamedTempFile::new().unwrap();
        let path = temp_db.path().to_str().unwrap();
        let store = SqliteReplayStore::open(path).unwrap();
        let now = 1000000;

        let res1 = store.claim("s1", "n1", now, now).unwrap();
        assert_eq!(res1, Claim::Fresh);

        // Same sender/nonce, different timestamp, within retention window.
        let res2 = store.claim("s1", "n1", now + 10, now + 10).unwrap();
        assert_eq!(res2, Claim::Replayed);
    }

    #[test]
    fn per_sender_cap_returns_full_without_claiming() {
        let temp_db = NamedTempFile::new().unwrap();
        let path = temp_db.path().to_str().unwrap();

        // Seed exactly 10000 rows efficiently to avoid 10k individual fsync loops
        {
            let mut conn = rusqlite::Connection::open(path).unwrap();
            conn.execute_batch(
                "
                PRAGMA journal_mode = WAL;
                CREATE TABLE IF NOT EXISTS seen (
                    sender TEXT NOT NULL,
                    nonce TEXT NOT NULL,
                    ts INTEGER NOT NULL,
                    PRIMARY KEY (sender, nonce)
                );
                CREATE TABLE IF NOT EXISTS meta (
                    id INTEGER PRIMARY KEY CHECK (id = 1),
                    floor INTEGER NOT NULL
                );
                INSERT OR IGNORE INTO meta (id, floor) VALUES (1, 0);
            ",
            )
            .unwrap();

            let tx = conn.transaction().unwrap();
            for i in 0..10000 {
                tx.execute(
                    "INSERT INTO seen (sender, nonce, ts) VALUES (?1, ?2, ?3)",
                    rusqlite::params!["s1", format!("n{}", i), 1000000],
                )
                .unwrap();
            }
            tx.commit().unwrap();
        }

        let store = SqliteReplayStore::open(path).unwrap();
        let res = store.claim("s1", "n_new", 1000000, 1000000).unwrap();
        assert_eq!(res, Claim::Full);

        // Verify another sender can claim successfully.
        let res_other = store.claim("s2", "n_new", 1000000, 1000000).unwrap();
        assert_eq!(res_other, Claim::Fresh);

        // Query DB to verify the rejected nonce wasn't inserted.
        let conn = rusqlite::Connection::open(path).unwrap();
        let count_rejected: i64 = conn
            .query_row(
                "SELECT COUNT(1) FROM seen WHERE sender = 's1' AND nonce = 'n_new'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count_rejected, 0);

        // Verify existing nonce claims still return Replayed at cap.
        let res_replayed = store.claim("s1", "n0", 1000000, 1000000).unwrap();
        assert_eq!(res_replayed, Claim::Replayed);
    }

    #[test]
    fn invalid_empty_or_oversized_identity_and_nonce_rejected() {
        let temp_db = NamedTempFile::new().unwrap();
        let path = temp_db.path().to_str().unwrap();
        let store = SqliteReplayStore::open(path).unwrap();
        let now = 1000000;

        assert!(store.claim("", "n1", now, now).is_err());
        assert!(store.claim("s1", "", now, now).is_err());

        let oversized_sender = "a".repeat(129);
        assert!(store.claim(&oversized_sender, "n1", now, now).is_err());

        let oversized_nonce = "b".repeat(65);
        assert!(store.claim("s1", &oversized_nonce, now, now).is_err());
    }

    #[test]
    fn sqlite_open_or_transaction_error_is_not_fresh() {
        // use an invalid path
        let res = SqliteReplayStore::open("/invalid/path/that/does/not/exist");
        assert!(res.is_err());

        // Real transaction/query failure case
        let temp_db = NamedTempFile::new().unwrap();
        let path = temp_db.path().to_str().unwrap();
        let store = SqliteReplayStore::open(path).unwrap();

        // Drop the seen table via an external connection to force a query failure
        {
            let conn = rusqlite::Connection::open(path).unwrap();
            conn.execute("DROP TABLE seen", []).unwrap();
        }

        let res_claim = store.claim("s1", "n1", 1000000, 1000000);
        assert!(res_claim.is_err());
    }

    #[test]
    fn timestamp_extremes_are_rejected_without_overflow() {
        let temp_db = NamedTempFile::new().unwrap();
        let path = temp_db.path().to_str().unwrap();
        let store = SqliteReplayStore::open(path).unwrap();

        // Exact 300 skew inclusive should be accepted (i.e. skew <= 300)
        // From instruction: skew > 300 rejected, so <= 300 is fresh
        let now = 1000000;
        assert_eq!(
            store.claim("s1", "n1", now - 300, now).unwrap(),
            Claim::Fresh
        );
        assert_eq!(
            store.claim("s2", "n2", now + 300, now).unwrap(),
            Claim::Fresh
        );

        assert_eq!(
            store.claim("s3", "n3", now - 301, now).unwrap(),
            Claim::Stale
        );
        assert_eq!(
            store.claim("s4", "n4", now + 301, now).unwrap(),
            Claim::Stale
        );

        // Using large i64 values without overflow panic
        assert_eq!(store.claim("s5", "n5", i64::MAX, 0).unwrap(), Claim::Stale);
        assert_eq!(store.claim("s6", "n6", i64::MIN, 0).unwrap(), Claim::Stale);
    }

    #[test]
    fn poisoned_mutex_returns_err() {
        use std::panic::{catch_unwind, AssertUnwindSafe};

        let temp_db = NamedTempFile::new().unwrap();
        let store = SqliteReplayStore::open(temp_db.path()).unwrap();
        let store = std::sync::Arc::new(store);

        let store_clone = store.clone();
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _guard = store_clone.conn.lock().unwrap();
            panic!("Intentional panic to poison mutex");
        }));

        let res = store.claim("s1", "n1", 1000000, 1000000);
        assert!(res.is_err());
        assert!(res.unwrap_err().to_string().contains("poisoned"));
    }

    #[test]
    fn missing_meta_row_or_table_is_err_and_inserts_nothing() {
        // Test missing meta row
        let temp_db1 = NamedTempFile::new().unwrap();
        let path1 = temp_db1.path().to_str().unwrap();
        let store1 = SqliteReplayStore::open(path1).unwrap();

        {
            let conn = rusqlite::Connection::open(path1).unwrap();
            conn.execute("DELETE FROM meta", []).unwrap();
        }

        let now = 1000000;
        let res1 = store1.claim("s1", "n1", now, now);
        assert!(res1.is_err());

        let conn1 = rusqlite::Connection::open(path1).unwrap();
        let count1: i64 = conn1
            .query_row("SELECT COUNT(*) FROM seen", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count1, 0);

        // Test missing meta table
        let temp_db2 = NamedTempFile::new().unwrap();
        let path2 = temp_db2.path().to_str().unwrap();
        let store2 = SqliteReplayStore::open(path2).unwrap();

        {
            let conn = rusqlite::Connection::open(path2).unwrap();
            conn.execute("DROP TABLE meta", []).unwrap();
        }

        let res2 = store2.claim("s1", "n1", now, now);
        assert!(res2.is_err());

        let conn2 = rusqlite::Connection::open(path2).unwrap();
        let count2: i64 = conn2
            .query_row("SELECT COUNT(*) FROM seen", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count2, 0);
    }

    #[test]
    fn non_sqlite_file_open_is_err() {
        use std::io::Write;

        let mut temp_file = NamedTempFile::new().unwrap();
        temp_file
            .write_all(b"this is not a valid sqlite database")
            .unwrap();
        let path = temp_file.path().to_str().unwrap();

        let res = SqliteReplayStore::open(path);
        assert!(res.is_err());

        let content = std::fs::read(path).unwrap();
        assert_eq!(content, b"this is not a valid sqlite database");
    }

    #[test]
    fn pragmas_are_wal_and_full() {
        let temp_db = NamedTempFile::new().unwrap();
        let path = temp_db.path().to_str().unwrap();
        let store = SqliteReplayStore::open(path).unwrap();

        let conn = store.conn.lock().unwrap();

        let journal_mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .unwrap();
        assert_eq!(journal_mode.to_lowercase(), "wal");

        let synchronous: i64 = conn
            .query_row("PRAGMA synchronous", [], |r| r.get(0))
            .unwrap();
        assert_eq!(synchronous, 2); // 2 is FULL
    }

    #[test]
    fn cap_recovers_after_expiry_and_boundary_9999() {
        let temp_db = NamedTempFile::new().unwrap();
        let path = temp_db.path().to_str().unwrap();

        let store = SqliteReplayStore::open(path).unwrap();
        let now = 1000000;

        {
            let mut conn = store.conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            for i in 0..9999 {
                tx.execute(
                    "INSERT INTO seen (sender, nonce, ts) VALUES (?1, ?2, ?3)",
                    rusqlite::params!["s1", format!("n{}", i), now],
                )
                .unwrap();
            }
            tx.commit().unwrap();
        }

        // 10000th claim
        let res_10000 = store.claim("s1", "n9999", now, now).unwrap();
        assert_eq!(res_10000, Claim::Fresh);

        // 10001st claim
        let res_10001 = store.claim("s1", "n10000", now, now).unwrap();
        assert_eq!(res_10001, Claim::Full);

        // Verify n10000 wasn't inserted
        {
            let conn = store.conn.lock().unwrap();
            let count_n10000: i64 = conn
                .query_row(
                    "SELECT COUNT(1) FROM seen WHERE sender = 's1' AND nonce = 'n10000'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(count_n10000, 0);
        }

        // Fast forward clock > 601s (let's use 1000s)
        let future_now = now + 1000;
        let future_ts = future_now;

        // This will trigger pruning of all rows with ts < effective_floor
        // effective_floor = future_now - 301 = 1000000 + 1000 - 301 = 1000699
        // Old rows had ts = 1000000
        let res_future = store
            .claim("s1", "n_future", future_ts, future_now)
            .unwrap();
        assert_eq!(res_future, Claim::Fresh);

        {
            let conn = store.conn.lock().unwrap();
            let count_old: i64 = conn
                .query_row(
                    "SELECT COUNT(1) FROM seen WHERE sender = 's1' AND ts = ?1",
                    rusqlite::params![now],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(count_old, 0); // They should have been pruned

            let floor: i64 = conn
                .query_row("SELECT floor FROM meta WHERE id = 1", [], |r| r.get(0))
                .unwrap();
            assert_eq!(floor, 1000699);
        }

        // test before pruning happens
        let res_replayed = store.claim("s1", "n9999", now, now).unwrap();
        assert_eq!(res_replayed, Claim::Replayed);
    }

    #[test]
    fn concurrent_cap_and_duplicate_claims() {
        let temp_db = NamedTempFile::new().unwrap();
        let path = temp_db.path().to_path_buf();

        // 50 rounds of duplicate claims
        let store1 = SqliteReplayStore::open(&path).unwrap();
        let store2 = SqliteReplayStore::open(&path).unwrap();

        let mut success = true;
        for i in 0..50 {
            let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
            let b1 = barrier.clone();
            let b2 = barrier;

            let nonce = format!("concurrent_nonce_{}", i);
            let n1 = nonce.clone();
            let n2 = nonce;

            // Re-open inside loop or outside? The instructions say:
            // "strengthen duplicate test using independent open connections, opened before spawning threads, per-round unique nonces, >=50 rounds"
            // Wait, we need to pass the stores, but we can't easily move them multiple times without Arc.
            // Oh, we can just use scoped threads to borrow `store1` and `store2`.

            let res = std::thread::scope(|s| {
                let t1 = s.spawn(move || {
                    b1.wait();
                    store1.claim("s1", &n1, 1000000, 1000000)
                });

                let t2 = s.spawn(move || {
                    b2.wait();
                    store2.claim("s1", &n2, 1000000, 1000000)
                });

                let res1 = t1.join().unwrap();
                let res2 = t2.join().unwrap();
                (res1, res2)
            });

            let mut results = [res.0.unwrap(), res.1.unwrap()];
            results.sort_by_key(|r| match r {
                Claim::Fresh => 1,
                Claim::Replayed => 2,
                _ => 3,
            });

            if results[0] != Claim::Fresh || results[1] != Claim::Replayed {
                success = false;
                break;
            }
        }
        assert!(success);

        // Separate 9999 seeded cap race
        let temp_db2 = NamedTempFile::new().unwrap();
        let path2 = temp_db2.path().to_path_buf();

        {
            let store = SqliteReplayStore::open(&path2).unwrap();
            let mut conn = store.conn.lock().unwrap();
            let tx = conn.transaction().unwrap();
            for i in 0..9999 {
                tx.execute(
                    "INSERT INTO seen (sender, nonce, ts) VALUES (?1, ?2, ?3)",
                    rusqlite::params!["s2", format!("n{}", i), 1000000],
                )
                .unwrap();
            }
            tx.commit().unwrap();
        }

        let store3 = SqliteReplayStore::open(&path2).unwrap();
        let store4 = SqliteReplayStore::open(&path2).unwrap();
        let barrier_cap = std::sync::Arc::new(std::sync::Barrier::new(2));

        let res_cap = std::thread::scope(|s| {
            let b1 = barrier_cap.clone();
            let b2 = barrier_cap;

            let t1 = s.spawn(move || {
                b1.wait();
                store3.claim("s2", "n_race_1", 1000000, 1000000)
            });

            let t2 = s.spawn(move || {
                b2.wait();
                store4.claim("s2", "n_race_2", 1000000, 1000000)
            });

            let r1 = t1.join().unwrap().unwrap();
            let r2 = t2.join().unwrap().unwrap();
            (r1, r2)
        });

        let mut results_cap = [res_cap.0, res_cap.1];
        results_cap.sort_by_key(|r| match r {
            Claim::Fresh => 1,
            Claim::Full => 2,
            _ => 3,
        });

        assert_eq!(results_cap[0], Claim::Fresh);
        assert_eq!(results_cap[1], Claim::Full);

        // final sender count 10000
        let conn2 = rusqlite::Connection::open(&path2).unwrap();
        let final_count: i64 = conn2
            .query_row("SELECT COUNT(1) FROM seen WHERE sender = 's2'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(final_count, 10000);
    }
}
