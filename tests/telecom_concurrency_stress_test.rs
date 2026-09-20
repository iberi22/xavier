use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::task;

use xavier_core_logic::types::ClearanceLevel;
use xavier::storage::pragma::{apply_pragmas, maybe_wal_checkpoint};
use xavier::telecom::chat::{create_room, init_chat_db, send_direct_message};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn test_telecom_concurrency_stress_50_tasks_100_msgs() {
    let temp_dir = tempdir().expect("Failed to create tempdir");
    let db_path = temp_dir.path().join("telecom_stress.db");

    let manager = SqliteConnectionManager::file(&db_path).with_init(|conn| {
        apply_pragmas(conn).expect("Failed to apply pragmas in pool init");
        init_chat_db(conn).expect("Failed to init chat db schema");
        Ok(())
    });

    let pool = Pool::builder()
        .max_size(50)
        .build(manager)
        .expect("Failed to create connection pool");

    let pool_arc = Arc::new(pool);

    // Create a room using the first connection
    let room = {
        let conn = pool_arc.get().expect("Failed to get connection from pool");
        create_room(&conn, "node-alpha", "node-beta").expect("Failed to create chat room")
    };

    let room_arc = Arc::new(room);
    let mut handles = vec![];

    let start_barrier = Arc::new(tokio::sync::Barrier::new(50));

    for task_id in 0..50 {
        let pool_clone = Arc::clone(&pool_arc);
        let room_clone = Arc::clone(&room_arc);
        let barrier_clone = Arc::clone(&start_barrier);

        let handle = task::spawn(async move {
            barrier_clone.wait().await;

            // Acquire connection for this task
            let conn = pool_clone.get().expect("Task failed to get connection");

            let sender = if task_id % 2 == 0 { "node-alpha" } else { "node-beta" };

            for msg_idx in 0..100 {
                let content = format!("Stress test payload from task {} msg {}", task_id, msg_idx);

                // Use a block to constrain connection borrowing
                let _msg = send_direct_message(
                    &conn,
                    &room_clone,
                    sender,
                    &content,
                    false, // ephemeral = false (we want it persisted to DB)
                    ClearanceLevel::Confidential,
                ).unwrap_or_else(|e| panic!("Task {} failed to send msg {}: {}", task_id, msg_idx, e));
            }
        });

        handles.push(handle);
    }

    // Wait for all tasks to complete
    for handle in handles {
        handle.await.expect("Task panicked");
    }

    // Validate the results
    let conn = pool_arc.get().expect("Failed to get connection for validation");

    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM chat_messages WHERE room_id = ?1",
            rusqlite::params![room_arc.room_id],
            |row| row.get(0),
        )
        .expect("Failed to query messages count");

    assert_eq!(count, 5000, "Expected exactly 5000 messages (50 tasks * 100 msgs) but found {}", count);

    // Force a WAL checkpoint and assert it succeeds
    // In our test, maybe_wal_checkpoint returns true if checkpointed or false if under threshold.
    // If we pass 1 as threshold, it should return true.
    let checkpointed = maybe_wal_checkpoint(&conn, 1).expect("Failed to run WAL checkpoint");
    assert!(checkpointed, "Expected WAL checkpoint to occur with threshold 1");
}
