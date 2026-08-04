use std::net::TcpListener;
use std::process::{Child, Stdio};
use std::time::Duration;
use tempfile::TempDir;

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct NodeProcess {
    child: Option<ChildGuard>,
    port: u16,
    home_dir: TempDir,
    token: String,
}

impl NodeProcess {
    fn start(port: u16, home_dir: TempDir, token: String) -> Self {
        let mut node = Self {
            child: None,
            port,
            home_dir,
            token,
        };
        node.spawn_child();
        node
    }

    fn spawn_child(&mut self) {
        let url = format!("http://127.0.0.1:{}", self.port);
        let child_proc = std::process::Command::new(env!("CARGO_BIN_EXE_xavier"))
            .arg("http")
            .arg(self.port.to_string())
            .arg("--mcp-port")
            .arg("0")
            .env("XAVIER_HOST", "127.0.0.1")
            .env("XAVIER_PORT", self.port.to_string())
            .env("XAVIER_URL", &url)
            .env("XAVIER_TOKEN", &self.token)
            .env("XAVIER_MCP_PORT", "0")
            .env("XAVIER_HOME", self.home_dir.path())
            .env("XAVIER_STATE_DIR", self.home_dir.path())
            .env(
                "XAVIER_CODE_GRAPH_DB_PATH",
                self.home_dir.path().join("code_graph.db"),
            )
            .env(
                "XAVIER_MEMORY_VEC_PATH",
                self.home_dir.path().join("memory_vec.db"),
            )
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("failed to start xavier binary");

        self.child = Some(ChildGuard(child_proc));
    }

    fn stop(&mut self) {
        self.child = None;
    }

    fn resume(&mut self) {
        self.spawn_child();
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    async fn wait_until_ready(&self) {
        let client = reqwest::Client::new();
        let ready_url = format!("{}/ready", self.url());
        for _ in 0..120 {
            if let Ok(resp) = client.get(&ready_url).send().await {
                if resp.status().is_success() {
                    return;
                }
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        panic!("Node on port {} did not become ready", self.port);
    }
}

async fn add_memory(node: &NodeProcess, path: &str, content: &str) -> String {
    let client = reqwest::Client::new();
    let url = format!("{}/memory/add", node.url());
    let payload = serde_json::json!({
        "content": content,
        "path": path,
    });
    let resp = client
        .post(&url)
        .header("X-Xavier-Token", &node.token)
        .json(&payload)
        .send()
        .await
        .expect("add memory request");

    assert!(
        resp.status().is_success(),
        "failed to add memory to node, got status {}",
        resp.status()
    );
    let val: serde_json::Value = resp.json().await.expect("add memory response");
    assert_eq!(val["status"], "ok");
    val["id"].as_str().expect("id").to_string()
}

async fn search_memory(node: &NodeProcess, query: &str) -> Vec<serde_json::Value> {
    let client = reqwest::Client::new();
    let url = format!("{}/memory/search", node.url());
    let payload = serde_json::json!({
        "query": query,
        "limit": 10,
    });
    let resp = client
        .post(&url)
        .header("X-Xavier-Token", &node.token)
        .json(&payload)
        .send()
        .await
        .expect("search request");

    if !resp.status().is_success() {
        return vec![];
    }
    let val: serde_json::Value = resp.json().await.expect("search response");
    val["results"].as_array().cloned().unwrap_or_default()
}

async fn trigger_sync_push(node: &NodeProcess, peer_url: &str) -> serde_json::Value {
    let client = reqwest::Client::new();
    let url = format!("{}/api/v1/memory/sync/push", node.url());
    let payload = serde_json::json!({
        "peer_url": peer_url,
        "workspace_id": "default",
        "since": "0",
    });
    let resp = client
        .post(&url)
        .json(&payload)
        .send()
        .await
        .expect("sync push request");

    assert!(resp.status().is_success());
    resp.json().await.expect("sync push response")
}

async fn trigger_sync_pull(node: &NodeProcess, peer_url: &str) -> serde_json::Value {
    let client = reqwest::Client::new();
    let url = format!("{}/api/v1/memory/sync/pull", node.url());
    let payload = serde_json::json!({
        "peer_url": peer_url,
        "workspace_id": "default",
        "since": "0",
    });
    let resp = client
        .post(&url)
        .json(&payload)
        .send()
        .await
        .expect("sync pull request");

    assert!(resp.status().is_success());
    resp.json().await.expect("sync pull response")
}

fn get_free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("bind ephemeral port")
        .local_addr()
        .expect("local addr")
        .port()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_multi_node_e2e_sync() {
    let port_a = get_free_port();
    let port_b = get_free_port();

    let home_a = TempDir::new().expect("temp home A");
    let home_b = TempDir::new().expect("temp home B");

    let node_a = NodeProcess::start(port_a, home_a, "token_a".to_string());
    let mut node_b = NodeProcess::start(port_b, home_b, "token_b".to_string());

    node_a.wait_until_ready().await;
    node_b.wait_until_ready().await;

    // ───────────────────────────────────────────────────────────────────────
    // SCENARIO 1: Simple sync A -> B
    // ───────────────────────────────────────────────────────────────────────

    // Add memory on A
    add_memory(
        &node_a,
        "episodic/tech/blueprints",
        "Secret blueprints of the warp drive",
    )
    .await;

    // Verify B does not have it initially
    let mut search_b = search_memory(&node_b, "warp drive").await;
    assert!(
        search_b.is_empty(),
        "Node B should not have A's blueprints before sync"
    );

    // Trigger sync push on A towards B
    let push_res = trigger_sync_push(&node_a, &node_b.url()).await;
    assert_eq!(push_res["status"], "ok");

    // Give a brief moment for database / cache updates if any
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Verify B now has the record
    search_b = search_memory(&node_b, "warp drive").await;
    assert!(
        !search_b.is_empty(),
        "Node B should have A's blueprints after sync push"
    );
    assert!(
        search_b[0]["content"]
            .as_str()
            .unwrap()
            .contains("Secret blueprints"),
        "Content should match"
    );

    // ───────────────────────────────────────────────────────────────────────
    // SCENARIO 2: Concurrent write conflict (LWW wins)
    // ───────────────────────────────────────────────────────────────────────

    // Create a conflict: add same path on both nodes
    // Node A's version
    add_memory(
        &node_a,
        "episodic/tech/fusion",
        "Nuclear energy (A version)",
    )
    .await;

    // Sleep briefly to ensure distinct timestamps
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Node B's version (newer)
    add_memory(
        &node_b,
        "episodic/tech/fusion",
        "Clean nuclear fusion (B version)",
    )
    .await;

    // Trigger pull on A from B (or push B -> A, which is the same as triggering pull on A from B)
    let pull_res = trigger_sync_pull(&node_a, &node_b.url()).await;
    assert_eq!(pull_res["status"], "ok");

    tokio::time::sleep(Duration::from_millis(500)).await;

    // Verify A has accepted B's newer version
    let search_a = search_memory(&node_a, "nuclear").await;
    assert!(!search_a.is_empty(), "Should find fusion record on A");
    assert!(
        search_a[0]["content"]
            .as_str()
            .unwrap()
            .contains("Clean nuclear fusion (B version)"),
        "A should have updated to the newer version from B"
    );

    // ───────────────────────────────────────────────────────────────────────
    // SCENARIO 3: Resync after partition
    // ───────────────────────────────────────────────────────────────────────

    // Stop node B (simulating partition / offline state)
    node_b.stop();

    // Add another memory on A
    add_memory(
        &node_a,
        "episodic/tech/quantum",
        "Quantum computing is revolutionary",
    )
    .await;

    // Try to sync from A to B (it should fail gracefully)
    let client = reqwest::Client::new();
    let sync_url = format!("{}/api/v1/memory/sync/push", node_a.url());
    let payload = serde_json::json!({
        "peer_url": node_b.url(),
        "workspace_id": "default",
        "since": "0",
    });
    let fail_resp = client.post(&sync_url).json(&payload).send().await;
    // We expect an error response status from A saying B is unreachable
    if let Ok(resp) = fail_resp {
        if resp.status().is_success() {
            let val: serde_json::Value = resp.json().await.unwrap();
            assert_eq!(
                val["status"], "error",
                "sync push should fail when peer is offline"
            );
        }
    }

    // Bring Node B back online (state and databases are persisted in home_b)
    node_b.resume();
    node_b.wait_until_ready().await;

    // B should still not have A's quantum record
    let mut search_b_quantum = search_memory(&node_b, "Quantum").await;
    assert!(
        search_b_quantum.is_empty(),
        "Node B should not have the quantum record yet"
    );

    // Trigger sync push on A towards B
    let push_res_2 = trigger_sync_push(&node_a, &node_b.url()).await;
    assert_eq!(
        push_res_2["status"], "ok",
        "sync should succeed after partition is healed"
    );

    tokio::time::sleep(Duration::from_millis(500)).await;

    // Verify B now has the quantum record
    search_b_quantum = search_memory(&node_b, "Quantum").await;
    assert!(
        !search_b_quantum.is_empty(),
        "Node B should have the quantum record after resync"
    );
    assert!(
        search_b_quantum[0]["content"]
            .as_str()
            .unwrap()
            .contains("Quantum computing"),
        "Content should match"
    );
}
