use tempfile::tempdir;
use xavier::isolate_test_process;
use xavier::mesh::replay_store::{Claim, ReplayStore, SqliteReplayStore};

isolate_test_process!();

#[test]
fn fresh_then_replayed_across_reopen() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("replay.db");

    let now = 1_000_000_000;

    // First open
    {
        let store = SqliteReplayStore::open(&db_path).unwrap();
        let res = store.claim("s1", "n1", now, now).unwrap();
        assert_eq!(res, Claim::Fresh);
    }

    // Reopen
    {
        let store = SqliteReplayStore::open(&db_path).unwrap();
        let res = store.claim("s1", "n1", now, now).unwrap();
        assert_eq!(res, Claim::Replayed);
    }
}

#[test]
fn senders_are_isolated_across_reopen() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("replay.db");

    let now = 1_000_000_000;

    // First open for s1
    {
        let store = SqliteReplayStore::open(&db_path).unwrap();
        let res = store.claim("s1", "n1", now, now).unwrap();
        assert_eq!(res, Claim::Fresh);
    }

    // Reopen for s2
    {
        let store = SqliteReplayStore::open(&db_path).unwrap();
        let res = store.claim("s2", "n1", now, now).unwrap();
        assert_eq!(res, Claim::Fresh);
    }
}

#[test]
fn stale_timestamp_rejected() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("replay.db");

    let now = 1_000_000_000;

    // The skew constant is 300 seconds.
    // Any skew > 300 should be rejected.
    let stale_ts = now - 301;

    let store = SqliteReplayStore::open(&db_path).unwrap();

    // Expect Stale
    let res = store.claim("s1", "n1", stale_ts, now).unwrap();
    assert_eq!(res, Claim::Stale);

    // Later in-window claim with same nonce should be Fresh, because the stale one wasn't recorded.
    let res_fresh = store.claim("s1", "n1", now, now).unwrap();
    assert_eq!(res_fresh, Claim::Fresh);
}

#[test]
fn two_handles_same_file() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("replay.db");

    let now = 1_000_000_000;

    let store1 = SqliteReplayStore::open(&db_path).unwrap();
    let store2 = SqliteReplayStore::open(&db_path).unwrap();

    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let b1 = barrier.clone();
    let b2 = barrier;

    let s1_ref = &store1;
    let s2_ref = &store2;

    let res = std::thread::scope(|s| {
        let t1 = s.spawn(move || {
            b1.wait();
            s1_ref.claim("s1", "n1", now, now)
        });

        let t2 = s.spawn(move || {
            b2.wait();
            s2_ref.claim("s1", "n1", now, now)
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

    assert_eq!(results[0], Claim::Fresh);
    assert_eq!(results[1], Claim::Replayed);
}
