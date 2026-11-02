use serial_test::serial;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::net::TcpListener;
use xavier::mesh::node::NodeIdentity;
use xavier::server::f12_routes::{self, F12State};

// Sandbox HOME/XDG/XAVIER_* dirs before main: tests must never touch the real ~/.xavier.
xavier::isolate_test_process!();

#[cfg(feature = "mesh")]
#[tokio::test]
#[serial]
async fn test_p2p_same_wallet_peer_handshake() {
    let owner = NodeIdentity::generate();
    let wallet = xavier::mesh::derive_wallet_id(&owner.public_key);
    let identity_a = Arc::new(NodeIdentity::generate());
    let identity_b = Arc::new(NodeIdentity::generate());
    let identity_c = Arc::new(NodeIdentity::generate());
    let wallet_c = xavier::mesh::derive_wallet_id(&identity_c.public_key);

    let previous = std::env::var("XAVIER_MESH_LOCALHOST_ONLY").ok();
    std::env::set_var("XAVIER_MESH_LOCALHOST_ONLY", "1");
    let dir_a = tempdir().unwrap();
    let dir_b = tempdir().unwrap();
    let store_a: Arc<dyn xavier::memory::MemoryStore> =
        Arc::new(xavier::memory::store::InMemoryMemoryStore::default());
    let store_b: Arc<dyn xavier::memory::MemoryStore> =
        Arc::new(xavier::memory::store::InMemoryMemoryStore::default());
    let state_a = F12State::for_server(
        dir_a.path().to_path_buf(),
        Some(identity_a.clone()),
        store_a.clone(),
    );
    let state_b = F12State::for_server(
        dir_b.path().to_path_buf(),
        Some(identity_b.clone()),
        store_b.clone(),
    );
    match previous {
        Some(v) => std::env::set_var("XAVIER_MESH_LOCALHOST_ONLY", v),
        None => std::env::remove_var("XAVIER_MESH_LOCALHOST_ONLY"),
    }

    let serve = |state: F12State| async move {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = f12_routes::router(state);
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (url, server)
    };
    let (url_a, server_a) = serve(state_a).await;
    let (url_b, server_b) = serve(state_b).await;

    let client = reqwest::Client::new();
    let challenge = |client: reqwest::Client, url: String, wallet: String| async move {
        client
            .get(format!(
                "{}/v1/f12/private-mesh/challenge?wallet_id={}",
                url, wallet
            ))
            .send()
            .await
            .unwrap()
            .json::<xavier::mesh::wallet_ownership::WalletChallenge>()
            .await
            .unwrap()
    };
    let register = |url: String,
                    identity: Arc<NodeIdentity>,
                    wallet: String,
                    owner: &NodeIdentity,
                    iroh_addr: Option<String>| {
        let client = client.clone();
        async move {
            let node = identity.node_id.as_str().to_string();
            let reg_challenge = challenge(client.clone(), url.clone(), wallet.clone()).await;
            let reg_proof = xavier::mesh::wallet_ownership::sign_wallet_registration(
                owner,
                &reg_challenge,
                &node,
            );
            let reg = client
                .post(format!("{}/v1/f12/private-mesh/nodes", url))
                .json(&serde_json::json!({
                    "node_id": node,
                    "wallet_id": wallet,
                    "name": node,
                    "iroh_addr": iroh_addr,
                    "public_key_hex": xavier::crypto::hex_encode(&identity.public_key),
                    "proof": reg_proof,
                }))
                .send()
                .await
                .unwrap();
            reg.status()
        }
    };

    // Node A and B register under same wallet.
    assert_eq!(
        register(
            url_a.clone(),
            identity_a.clone(),
            wallet.clone(),
            &owner,
            None
        )
        .await,
        reqwest::StatusCode::CREATED
    );
    assert_eq!(
        register(
            url_a.clone(),
            identity_b.clone(),
            wallet.clone(),
            &owner,
            None
        )
        .await,
        reqwest::StatusCode::CREATED
    );
    assert_eq!(
        register(
            url_b.clone(),
            identity_b.clone(),
            wallet.clone(),
            &owner,
            None
        )
        .await,
        reqwest::StatusCode::CREATED
    );
    assert_eq!(
        register(
            url_b.clone(),
            identity_a.clone(),
            wallet.clone(),
            &owner,
            None
        )
        .await,
        reqwest::StatusCode::CREATED
    );

    // Assert that a third node with a different wallet ID is strictly rejected during handshake
    assert_eq!(
        register(
            url_a.clone(),
            identity_c.clone(),
            wallet_c.clone(),
            &identity_c,
            None
        )
        .await,
        reqwest::StatusCode::CREATED
    ); // Node 3 registers to its own wallet OK.

    // Now Node 3 tries to register on Node A under Node A's wallet, using Node 3's key. Should fail.
    assert_eq!(
        register(
            url_a.clone(),
            identity_c.clone(),
            wallet.clone(),
            &identity_c,
            None
        )
        .await,
        reqwest::StatusCode::UNAUTHORIZED
    );

    server_a.abort();
    server_b.abort();
}

#[cfg(feature = "mesh")]
#[tokio::test]
#[serial]
async fn test_bidirectional_memory_replication() {
    let owner = NodeIdentity::generate();
    let wallet = xavier::mesh::derive_wallet_id(&owner.public_key);
    let identity_a = Arc::new(NodeIdentity::generate());
    let identity_b = Arc::new(NodeIdentity::generate());

    let previous = std::env::var("XAVIER_MESH_LOCALHOST_ONLY").ok();
    std::env::set_var("XAVIER_MESH_LOCALHOST_ONLY", "1");
    let dir_a = tempdir().unwrap();
    let dir_b = tempdir().unwrap();
    let store_a: Arc<dyn xavier::memory::MemoryStore> =
        Arc::new(xavier::memory::store::InMemoryMemoryStore::default());
    let store_b: Arc<dyn xavier::memory::MemoryStore> =
        Arc::new(xavier::memory::store::InMemoryMemoryStore::default());
    let state_a = F12State::for_server(
        dir_a.path().to_path_buf(),
        Some(identity_a.clone()),
        store_a.clone(),
    );
    let state_b = F12State::for_server(
        dir_b.path().to_path_buf(),
        Some(identity_b.clone()),
        store_b.clone(),
    );
    match previous {
        Some(v) => std::env::set_var("XAVIER_MESH_LOCALHOST_ONLY", v),
        None => std::env::remove_var("XAVIER_MESH_LOCALHOST_ONLY"),
    }

    let serve = |state: F12State| async move {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = f12_routes::router(state);
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (url, server)
    };
    let (url_a, server_a) = serve(state_a).await;
    let (url_b, server_b) = serve(state_b).await;

    let client = reqwest::Client::new();
    let challenge = |client: reqwest::Client, url: String, wallet: String| async move {
        client
            .get(format!(
                "{}/v1/f12/private-mesh/challenge?wallet_id={}",
                url, wallet
            ))
            .send()
            .await
            .unwrap()
            .json::<xavier::mesh::wallet_ownership::WalletChallenge>()
            .await
            .unwrap()
    };
    let register = |url: String, identity: Arc<NodeIdentity>, iroh_addr: Option<String>| {
        let client = client.clone();
        let wallet = wallet.clone();
        let owner = &owner;
        async move {
            let node = identity.node_id.as_str().to_string();
            let reg_challenge = challenge(client.clone(), url.clone(), wallet.clone()).await;
            let reg_proof = xavier::mesh::wallet_ownership::sign_wallet_registration(
                owner,
                &reg_challenge,
                &node,
            );
            let reg = client
                .post(format!("{}/v1/f12/private-mesh/nodes", url))
                .json(&serde_json::json!({
                    "node_id": node,
                    "wallet_id": wallet,
                    "name": node,
                    "iroh_addr": iroh_addr,
                    "public_key_hex": xavier::crypto::hex_encode(&identity.public_key),
                    "proof": reg_proof,
                }))
                .send()
                .await
                .unwrap();
            assert_eq!(reg.status(), reqwest::StatusCode::CREATED);
        }
    };

    let receiver_b: serde_json::Value = client
        .get(format!("{}/v1/f12/private-mesh/receiver", url_b))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let addr_b = receiver_b["iroh_addr"].as_str().unwrap().to_string();

    let receiver_a: serde_json::Value = client
        .get(format!("{}/v1/f12/private-mesh/receiver", url_a))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let addr_a = receiver_a["iroh_addr"].as_str().unwrap().to_string();

    register(url_b.clone(), identity_b.clone(), None).await;
    register(url_b.clone(), identity_a.clone(), None).await;
    register(url_a.clone(), identity_a.clone(), None).await;
    register(url_a.clone(), identity_b.clone(), Some(addr_b)).await;
    register(url_b.clone(), identity_a.clone(), Some(addr_a)).await;

    // Node 1 writes memory record and triggers sync to Node 2
    let node_b = identity_b.node_id.as_str().to_string();
    let sync_challenge_a = challenge(client.clone(), url_a.clone(), wallet.clone()).await;
    let sync_proof_a = xavier::mesh::wallet_ownership::sign_wallet_action(
        &owner,
        &sync_challenge_a,
        "sync",
        &node_b,
    );
    let resp = client
        .post(format!("{}/v1/f12/private-mesh/sync", url_a))
        .json(&serde_json::json!({
            "wallet_id": wallet,
            "target_node": node_b,
            "proof": sync_proof_a,
            "memories": [{
                "path": "doc/node1_entry",
                "content": "delivered to node B",
                "metadata": {},
                "created_at": chrono::Utc::now().timestamp(),
            }],
            "snapshots": [],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    let applied_b = store_b
        .get("mesh-sync", "doc/node1_entry")
        .await
        .unwrap()
        .expect("memory applied on node B");
    assert_eq!(applied_b.content, "delivered to node B");

    // Node 2 writes memory record and triggers sync to Node 1
    let node_a = identity_a.node_id.as_str().to_string();
    let sync_challenge_b = challenge(client.clone(), url_b.clone(), wallet.clone()).await;
    let sync_proof_b = xavier::mesh::wallet_ownership::sign_wallet_action(
        &owner,
        &sync_challenge_b,
        "sync",
        &node_a,
    );
    let resp_b = client
        .post(format!("{}/v1/f12/private-mesh/sync", url_b))
        .json(&serde_json::json!({
            "wallet_id": wallet,
            "target_node": node_a,
            "proof": sync_proof_b,
            "memories": [{
                "path": "doc/node2_entry",
                "content": "delivered to node A",
                "metadata": {},
                "created_at": chrono::Utc::now().timestamp(),
            }],
            "snapshots": [],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp_b.status(), reqwest::StatusCode::OK);

    let applied_a = store_a
        .get("mesh-sync", "doc/node2_entry")
        .await
        .unwrap()
        .expect("memory applied on node A");
    assert_eq!(applied_a.content, "delivered to node A");

    server_a.abort();
    server_b.abort();
}

#[cfg(feature = "mesh")]
#[tokio::test]
#[serial]
#[ignore = "bug: vectors are dropped from PrivateSyncApiRequest"]
async fn test_private_vector_delta_replication() {
    let owner = NodeIdentity::generate();
    let wallet = xavier::mesh::derive_wallet_id(&owner.public_key);
    let identity_a = Arc::new(NodeIdentity::generate());
    let identity_b = Arc::new(NodeIdentity::generate());

    let previous = std::env::var("XAVIER_MESH_LOCALHOST_ONLY").ok();
    std::env::set_var("XAVIER_MESH_LOCALHOST_ONLY", "1");
    let dir_a = tempdir().unwrap();
    let dir_b = tempdir().unwrap();
    let store_a: Arc<dyn xavier::memory::MemoryStore> =
        Arc::new(xavier::memory::store::InMemoryMemoryStore::default());
    let store_b: Arc<dyn xavier::memory::MemoryStore> =
        Arc::new(xavier::memory::store::InMemoryMemoryStore::default());
    let state_a = F12State::for_server(
        dir_a.path().to_path_buf(),
        Some(identity_a.clone()),
        store_a.clone(),
    );
    let state_b = F12State::for_server(
        dir_b.path().to_path_buf(),
        Some(identity_b.clone()),
        store_b.clone(),
    );
    match previous {
        Some(v) => std::env::set_var("XAVIER_MESH_LOCALHOST_ONLY", v),
        None => std::env::remove_var("XAVIER_MESH_LOCALHOST_ONLY"),
    }

    let serve = |state: F12State| async move {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = f12_routes::router(state);
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (url, server)
    };
    let (url_a, server_a) = serve(state_a).await;
    let (url_b, server_b) = serve(state_b).await;

    let client = reqwest::Client::new();
    let challenge = |client: reqwest::Client, url: String, wallet: String| async move {
        client
            .get(format!(
                "{}/v1/f12/private-mesh/challenge?wallet_id={}",
                url, wallet
            ))
            .send()
            .await
            .unwrap()
            .json::<xavier::mesh::wallet_ownership::WalletChallenge>()
            .await
            .unwrap()
    };
    let register = |url: String, identity: Arc<NodeIdentity>, iroh_addr: Option<String>| {
        let client = client.clone();
        let wallet = wallet.clone();
        let owner = &owner;
        async move {
            let node = identity.node_id.as_str().to_string();
            let reg_challenge = challenge(client.clone(), url.clone(), wallet.clone()).await;
            let reg_proof = xavier::mesh::wallet_ownership::sign_wallet_registration(
                owner,
                &reg_challenge,
                &node,
            );
            let reg = client
                .post(format!("{}/v1/f12/private-mesh/nodes", url))
                .json(&serde_json::json!({
                    "node_id": node,
                    "wallet_id": wallet,
                    "name": node,
                    "iroh_addr": iroh_addr,
                    "public_key_hex": xavier::crypto::hex_encode(&identity.public_key),
                    "proof": reg_proof,
                }))
                .send()
                .await
                .unwrap();
            assert_eq!(reg.status(), reqwest::StatusCode::CREATED);
        }
    };

    let receiver_b: serde_json::Value = client
        .get(format!("{}/v1/f12/private-mesh/receiver", url_b))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let addr_b = receiver_b["iroh_addr"].as_str().unwrap().to_string();

    register(url_b.clone(), identity_b.clone(), None).await;
    register(url_b.clone(), identity_a.clone(), None).await;
    register(url_a.clone(), identity_b.clone(), Some(addr_b)).await;

    let node_b = identity_b.node_id.as_str().to_string();
    let sync_challenge_a = challenge(client.clone(), url_a.clone(), wallet.clone()).await;
    let sync_proof_a = xavier::mesh::wallet_ownership::sign_wallet_action(
        &owner,
        &sync_challenge_a,
        "sync",
        &node_b,
    );

    // Assert vector is processed properly. It will fail.
    let resp = client
        .post(format!("{}/v1/f12/private-mesh/sync", url_a))
        .json(&serde_json::json!({
            "wallet_id": wallet,
            "target_node": node_b,
            "proof": sync_proof_a,
            "memories": [],
            "snapshots": [],
            "vectors": [{
                "path": "vector/node1_entry",
                "embedding": [0.1, 0.2, 0.3],
                "metadata": {}
            }]
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), reqwest::StatusCode::OK);

    server_a.abort();
    server_b.abort();

    // Test fails because vectors isn't parsed into PrivateSyncApiRequest and passed on.
    panic!("Vectors are dropped and not synced!");
}

#[cfg(feature = "mesh")]
#[tokio::test]
#[serial]
#[ignore = "bug: no offline replay queue drain mechanism"]
async fn test_offline_disconnect_replay_queue() {
    let owner = NodeIdentity::generate();
    let wallet = xavier::mesh::derive_wallet_id(&owner.public_key);
    let identity_a = Arc::new(NodeIdentity::generate());
    let identity_b = Arc::new(NodeIdentity::generate());

    let previous = std::env::var("XAVIER_MESH_LOCALHOST_ONLY").ok();
    std::env::set_var("XAVIER_MESH_LOCALHOST_ONLY", "1");
    let dir_a = tempdir().unwrap();
    let dir_b = tempdir().unwrap();
    let store_a: Arc<dyn xavier::memory::MemoryStore> =
        Arc::new(xavier::memory::store::InMemoryMemoryStore::default());
    let store_b: Arc<dyn xavier::memory::MemoryStore> =
        Arc::new(xavier::memory::store::InMemoryMemoryStore::default());
    let state_a = F12State::for_server(
        dir_a.path().to_path_buf(),
        Some(identity_a.clone()),
        store_a.clone(),
    );
    let state_b = F12State::for_server(
        dir_b.path().to_path_buf(),
        Some(identity_b.clone()),
        store_b.clone(),
    );
    match previous {
        Some(v) => std::env::set_var("XAVIER_MESH_LOCALHOST_ONLY", v),
        None => std::env::remove_var("XAVIER_MESH_LOCALHOST_ONLY"),
    }

    let serve = |state: F12State| async move {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = f12_routes::router(state);
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (url, server)
    };
    let (url_a, server_a) = serve(state_a).await;
    let (url_b, server_b) = serve(state_b).await;

    let client = reqwest::Client::new();
    let challenge = |client: reqwest::Client, url: String, wallet: String| async move {
        client
            .get(format!(
                "{}/v1/f12/private-mesh/challenge?wallet_id={}",
                url, wallet
            ))
            .send()
            .await
            .unwrap()
            .json::<xavier::mesh::wallet_ownership::WalletChallenge>()
            .await
            .unwrap()
    };
    let register = |url: String, identity: Arc<NodeIdentity>, iroh_addr: Option<String>| {
        let client = client.clone();
        let wallet = wallet.clone();
        let owner = &owner;
        async move {
            let node = identity.node_id.as_str().to_string();
            let reg_challenge = challenge(client.clone(), url.clone(), wallet.clone()).await;
            let reg_proof = xavier::mesh::wallet_ownership::sign_wallet_registration(
                owner,
                &reg_challenge,
                &node,
            );
            let reg = client
                .post(format!("{}/v1/f12/private-mesh/nodes", url))
                .json(&serde_json::json!({
                    "node_id": node,
                    "wallet_id": wallet,
                    "name": node,
                    "iroh_addr": iroh_addr,
                    "public_key_hex": xavier::crypto::hex_encode(&identity.public_key),
                    "proof": reg_proof,
                }))
                .send()
                .await
                .unwrap();
            assert_eq!(reg.status(), reqwest::StatusCode::CREATED);
        }
    };

    let receiver_b: serde_json::Value = client
        .get(format!("{}/v1/f12/private-mesh/receiver", url_b))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let addr_b = receiver_b["iroh_addr"].as_str().unwrap().to_string();

    register(url_b.clone(), identity_b.clone(), None).await;
    register(url_b.clone(), identity_a.clone(), None).await;
    register(url_a.clone(), identity_b.clone(), Some(addr_b)).await;

    // Terminate Node 2
    server_b.abort();

    let node_b = identity_b.node_id.as_str().to_string();
    let sync_challenge_a = challenge(client.clone(), url_a.clone(), wallet.clone()).await;
    let sync_proof_a = xavier::mesh::wallet_ownership::sign_wallet_action(
        &owner,
        &sync_challenge_a,
        "sync",
        &node_b,
    );
    let resp = client
        .post(format!("{}/v1/f12/private-mesh/sync", url_a))
        .json(&serde_json::json!({
            "wallet_id": wallet,
            "target_node": node_b,
            "proof": sync_proof_a,
            "memories": [{
                "path": "doc/queued_entry",
                "content": "offline queued",
                "metadata": {},
                "created_at": chrono::Utc::now().timestamp(),
            }],
            "snapshots": [],
        }))
        .send()
        .await
        .unwrap();

    // If it queues properly, the request could either 202 Accepted, or Node 1 runs a background queue.
    // In our system it currently just fails.
    assert!(resp.status().is_success() || resp.status() == reqwest::StatusCode::ACCEPTED);

    // Restart Node 2 listener
    let state_b_restarted = F12State::for_server(
        dir_b.path().to_path_buf(),
        Some(identity_b.clone()),
        store_b.clone(),
    );
    let (_url_b_new, server_b_new) = serve(state_b_restarted).await;

    // We should assert that Node 1 drained the offline queue and Node 2 applied the memory.
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    let applied_b = store_b
        .get("mesh-sync", "doc/queued_entry")
        .await
        .unwrap()
        .expect("memory applied on node B after restart");
    assert_eq!(applied_b.content, "offline queued");

    server_a.abort();
    server_b_new.abort();

    panic!("Offline replay queue drain is unimplemented!");
}
