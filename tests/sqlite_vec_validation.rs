use rusqlite::Connection;
use xavier::memory::sqlite_vec_store::vector;

#[tokio::test]
async fn test_sqlite_vec_available() {
    vector::register_sqlite_vec_extension().unwrap();
    let conn = Connection::open_in_memory().unwrap();

    // Check if vec_version() works
    let version: String = conn
        .query_row("SELECT vec_version()", [], |row| row.get(0))
        .unwrap();
    println!("sqlite-vec version: {}", version);

    // Check if vector32 and vector_distance_cos work
    let res: f32 = conn
        .query_row(
            "SELECT vec_distance_cosine(vec_f32('[1, 0, 0]'), vec_f32('[1, 0, 0]'))",
            [],
            |row| row.get(0),
        )
        .unwrap();

    assert!(res.abs() < 1e-6);
}
