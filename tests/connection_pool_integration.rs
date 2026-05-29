use libsql::Builder;
use xavier::utils::connection_pool::{LibsqlConnectionPool, PoolConfig};

async fn setup_test_pool() -> (LibsqlConnectionPool, tempfile::NamedTempFile) {
    let temp_file = tempfile::NamedTempFile::new().unwrap();
    let db = Builder::new_local(temp_file.path().to_str().unwrap())
        .build()
        .await
        .unwrap();
    (
        LibsqlConnectionPool::new(db, PoolConfig::default()),
        temp_file,
    )
}

#[tokio::test]
async fn test_pool_creation_and_get() {
    let (pool, _tmp) = setup_test_pool().await;
    let conn = pool.get().await;
    assert!(conn.is_ok());
}

#[tokio::test]
async fn test_pool_concurrency() {
    let (pool, _tmp) = setup_test_pool().await;
    let mut tasks = vec![];

    {
        let conn = pool.get().await.unwrap();
        conn.execute("CREATE TABLE IF NOT EXISTS test (id INTEGER)", ())
            .await
            .unwrap();
    }

    for i in 0..30 {
        let pool_clone = pool.clone();
        tasks.push(tokio::spawn(async move {
            let conn = pool_clone.get().await.unwrap();
            conn.execute("INSERT INTO test VALUES (?1)", libsql::params![i as i64])
                .await
                .unwrap();
        }));
    }

    for task in tasks {
        task.await.unwrap();
    }

    let conn = pool.get().await.unwrap();
    let mut rows = conn.query("SELECT COUNT(*) FROM test", ()).await.unwrap();
    let row = rows.next().await.unwrap().unwrap();
    let count: i64 = row.get(0).unwrap();
    assert_eq!(count, 30);
}

#[tokio::test]
async fn test_pool_telemetry() {
    let (pool, _tmp) = setup_test_pool().await;
    assert_eq!(pool.active_connections(), 0);
    assert_eq!(pool.idle_connections(), 0);

    {
        let _conn1 = pool.get().await.unwrap();
        assert_eq!(pool.active_connections(), 1);
        assert_eq!(pool.idle_connections(), 0);

        let _conn2 = pool.get().await.unwrap();
        assert_eq!(pool.active_connections(), 2);
        assert_eq!(pool.idle_connections(), 0);
    } // conns dropped here

    assert_eq!(pool.active_connections(), 0);
    assert_eq!(pool.idle_connections(), 2);
}

#[tokio::test]
async fn test_pool_timeout() {
    let temp_file = tempfile::NamedTempFile::new().unwrap();
    let db = Builder::new_local(temp_file.path().to_str().unwrap())
        .build()
        .await
        .unwrap();

    let config = PoolConfig {
        max_size: 1,
        connection_timeout: std::time::Duration::from_millis(50),
    };
    let pool = LibsqlConnectionPool::new(db, config);

    let _conn1 = pool.get().await.unwrap();
    let conn2_res = pool.get().await;
    assert!(conn2_res.is_err());
    let err_msg = conn2_res.err().unwrap().to_string();
    assert!(err_msg.contains("timed out"));
}
