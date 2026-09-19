//! 🌐 Multi-Node Telecom & Autonomous Agent Conversation Test
//!
//! E2E Simulation testing:
//! 1. Encrypted 1-to-1 Node-to-Node Chat
//! 2. Autonomous Agent Auto-Responder Loop
//! 3. RBAC & Clearance Level Security Blocking

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::{mpsc, RwLock};

use xavier::security::clearance::{can_access, ClearanceLevel};
use xavier::storage::pragma::apply_pragmas;
use xavier::telecom::chat::{
    acknowledge_receipt, create_room, decrypt_chat_message, init_chat_db, mark_read,
    send_direct_message, ChatMessage, ReceiptStatus,
};

/// Errors encountered during telecom mesh simulation.
#[derive(Debug, Error)]
pub enum SimulationError {
    #[error("Node '{0}' not found in network")]
    NodeNotFound(String),

    #[error("Access denied: clearance requirement {required:?} exceeds node level {provided:?}")]
    ClearanceDenied {
        required: ClearanceLevel,
        provided: ClearanceLevel,
    },

    #[error(
        "Wallet isolation violation: node '{0}' wallet '{1}' does not match target wallet '{2}'"
    )]
    WalletMismatch(String, String, String),

    #[error("Message delivery failed for recipient '{0}'")]
    DeliveryFailed(String),

    #[error("Database error: {0}")]
    DatabaseError(#[from] rusqlite::Error),

    #[error("Internal error: {0}")]
    Internal(String),
}

/// Simulation metrics collected across nodes and network interactions.
#[derive(Debug, Default)]
pub struct SimulationMetrics {
    pub total_messages_sent: AtomicU64,
    pub total_messages_received: AtomicU64,
    pub blocked_clearance_messages: AtomicU64,
    pub agent_auto_responses: AtomicU64,
    pub encryption_successes: AtomicU64,
    pub receipts_acknowledged: AtomicU64,
}

impl SimulationMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> SimulationMetricsSnapshot {
        SimulationMetricsSnapshot {
            total_messages_sent: self.total_messages_sent.load(Ordering::Relaxed),
            total_messages_received: self.total_messages_received.load(Ordering::Relaxed),
            blocked_clearance_messages: self.blocked_clearance_messages.load(Ordering::Relaxed),
            agent_auto_responses: self.agent_auto_responses.load(Ordering::Relaxed),
            encryption_successes: self.encryption_successes.load(Ordering::Relaxed),
            receipts_acknowledged: self.receipts_acknowledged.load(Ordering::Relaxed),
        }
    }
}

/// Immutable snapshot of simulation metrics for assertions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SimulationMetricsSnapshot {
    pub total_messages_sent: u64,
    pub total_messages_received: u64,
    pub blocked_clearance_messages: u64,
    pub agent_auto_responses: u64,
    pub encryption_successes: u64,
    pub receipts_acknowledged: u64,
}

/// Represents an individual virtual node in the telecom simulation network.
pub struct VirtualNode {
    pub node_id: String,
    pub wallet_id: String,
    pub clearance_level: ClearanceLevel,
    pub is_agent: bool,
    pub db: Arc<std::sync::Mutex<Connection>>,
    pub tx: mpsc::Sender<ChatMessage>,
    pub rx: Arc<RwLock<Option<mpsc::Receiver<ChatMessage>>>>,
}

impl VirtualNode {
    /// Creates a new virtual node initialized with an in-memory SQLite database applying PRAGMAs.
    pub fn new(
        node_id: &str,
        wallet_id: &str,
        clearance_level: ClearanceLevel,
        is_agent: bool,
    ) -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        apply_pragmas(&conn)?;

        let (tx, rx) = mpsc::channel(100);

        Ok(Self {
            node_id: node_id.to_string(),
            wallet_id: wallet_id.to_string(),
            clearance_level,
            is_agent,
            db: Arc::new(std::sync::Mutex::new(conn)),
            tx,
            rx: Arc::new(RwLock::new(Some(rx))),
        })
    }

    /// Takes the receiver channel for node-driven message processing loops.
    pub async fn take_receiver(&self) -> Option<mpsc::Receiver<ChatMessage>> {
        let mut rx_guard = self.rx.write().await;
        rx_guard.take()
    }
}

/// Simulated multi-node telecom network managing routing, gating, and agent auto-responders.
#[derive(Default)]
pub struct SimulatedNodeNetwork {
    nodes: Arc<RwLock<HashMap<String, Arc<VirtualNode>>>>,
    metrics: Arc<SimulationMetrics>,
}

impl SimulatedNodeNetwork {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn metrics(&self) -> &Arc<SimulationMetrics> {
        &self.metrics
    }

    /// Registers a virtual node into the network.
    pub async fn add_node(&self, node: VirtualNode) -> Arc<VirtualNode> {
        let node_arc = Arc::new(node);
        let mut guard = self.nodes.write().await;
        guard.insert(node_arc.node_id.clone(), node_arc.clone());
        node_arc
    }

    /// Gets a reference to a registered node.
    pub async fn get_node(&self, node_id: &str) -> Option<Arc<VirtualNode>> {
        let guard = self.nodes.read().await;
        guard.get(node_id).cloned()
    }

    /// Dispatches a message from sender to recipient, validating wallet isolation and clearance.
    pub async fn send_message(
        &self,
        sender_id: &str,
        recipient_id: &str,
        content: &str,
        clearance: ClearanceLevel,
        ephemeral: bool,
    ) -> Result<(ChatMessage, String)> {
        let sender = self
            .get_node(sender_id)
            .await
            .ok_or_else(|| SimulationError::NodeNotFound(sender_id.to_string()))?;
        let recipient = self
            .get_node(recipient_id)
            .await
            .ok_or_else(|| SimulationError::NodeNotFound(recipient_id.to_string()))?;

        // 1. Enforce wallet isolation check
        if sender.wallet_id != recipient.wallet_id {
            return Err(anyhow!(SimulationError::WalletMismatch(
                sender_id.to_string(),
                sender.wallet_id.clone(),
                recipient.wallet_id.clone()
            )));
        }

        // 2. Security Clearance Gate Check: Recipient clearance must be >= message clearance
        if !can_access(recipient.clearance_level, clearance) {
            self.metrics
                .blocked_clearance_messages
                .fetch_add(1, Ordering::Relaxed);
            return Err(anyhow!(SimulationError::ClearanceDenied {
                required: clearance,
                provided: recipient.clearance_level,
            }));
        }

        // 3. Create or fetch chat room in sender DB
        let room = {
            let conn = sender.db.lock().unwrap();
            create_room(&conn, sender_id, recipient_id)?
        };

        // Ensure room exists with identical room_id in recipient DB
        {
            let conn = recipient.db.lock().unwrap();
            init_chat_db(&conn)?;
            conn.execute(
                "INSERT OR IGNORE INTO direct_rooms (room_id, participant_a, participant_b, created_at, status) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    room.room_id,
                    room.participant_a,
                    room.participant_b,
                    room.created_at.to_rfc3339(),
                    room.status.as_str(),
                ],
            )?;
        }

        // 4. Encrypt and persist message payload in sender DB
        let msg = {
            let conn = sender.db.lock().unwrap();
            send_direct_message(&conn, &room, sender_id, content, ephemeral, clearance)?
        };

        // Persist message payload in recipient DB if non-ephemeral for receipt/read tracking
        if !ephemeral {
            let conn = recipient.db.lock().unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO chat_messages (message_id, room_id, sender_id, content_ciphertext_hex, nonce_hex, ephemeral, timestamp, clearance_level, delivered, read) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    msg.message_id,
                    msg.room_id,
                    msg.sender_id,
                    msg.content_ciphertext_hex,
                    msg.nonce_hex,
                    if msg.ephemeral { 1 } else { 0 },
                    msg.timestamp.to_rfc3339(),
                    msg.clearance_level.as_str(),
                    0,
                    0,
                ],
            )?;
        }

        self.metrics
            .total_messages_sent
            .fetch_add(1, Ordering::Relaxed);
        self.metrics
            .encryption_successes
            .fetch_add(1, Ordering::Relaxed);

        // 5. Deliver message payload to recipient Tokio channel
        if recipient.tx.send(msg.clone()).await.is_err() {
            return Err(anyhow!(SimulationError::DeliveryFailed(
                recipient_id.to_string()
            )));
        }

        self.metrics
            .total_messages_received
            .fetch_add(1, Ordering::Relaxed);

        Ok((msg, room.room_id))
    }

    /// Spawns an autonomous agent responder task for an agent node.
    pub async fn start_agent_auto_responder(
        &self,
        agent_id: &str,
    ) -> Result<tokio::task::JoinHandle<()>> {
        let agent = self
            .get_node(agent_id)
            .await
            .ok_or_else(|| SimulationError::NodeNotFound(agent_id.to_string()))?;

        if !agent.is_agent {
            return Err(anyhow!(SimulationError::Internal(format!(
                "Node '{}' is not marked as an agent",
                agent_id
            ))));
        }

        let mut rx = agent.take_receiver().await.ok_or_else(|| {
            SimulationError::Internal("Receiver channel already taken".to_string())
        })?;

        let network_ref = self.nodes.clone();
        let metrics_ref = self.metrics.clone();
        let agent_node_id = agent.node_id.clone();

        let handle = tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                // Decrypt incoming message using room_id from payload
                let decrypted = {
                    let agent_node = match network_ref.read().await.get(&agent_node_id).cloned() {
                        Some(n) => n,
                        None => break,
                    };
                    let _conn = agent_node.db.lock().unwrap();
                    decrypt_chat_message(&msg.room_id, &msg)
                };

                if let Ok(prompt) = decrypted {
                    let response_text = format!("Xavier Agent ACK: Processed input '{}'", prompt);

                    let target_sender = msg.sender_id.clone();
                    let agent_clearance = {
                        let guard = network_ref.read().await;
                        guard
                            .get(&agent_node_id)
                            .map(|a| a.clearance_level)
                            .unwrap_or(ClearanceLevel::Internal)
                    };

                    let sender_node = network_ref.read().await.get(&target_sender).cloned();
                    let agent_node = network_ref.read().await.get(&agent_node_id).cloned();

                    if let (Some(sender_node), Some(agent_node)) = (sender_node, agent_node) {
                        let room = {
                            let conn = agent_node.db.lock().unwrap();
                            create_room(&conn, &agent_node_id, &target_sender).ok()
                        };

                        if let Some(room) = room {
                            // Synchronize room in sender DB
                            {
                                let conn = sender_node.db.lock().unwrap();
                                let _ = init_chat_db(&conn);
                                let _ = conn.execute(
                                    "INSERT OR IGNORE INTO direct_rooms (room_id, participant_a, participant_b, created_at, status) \
                                     VALUES (?1, ?2, ?3, ?4, ?5)",
                                    params![
                                        room.room_id,
                                        room.participant_a,
                                        room.participant_b,
                                        room.created_at.to_rfc3339(),
                                        room.status.as_str(),
                                    ],
                                );
                            }

                            let reply_msg = {
                                let conn = agent_node.db.lock().unwrap();
                                send_direct_message(
                                    &conn,
                                    &room,
                                    &agent_node_id,
                                    &response_text,
                                    false,
                                    agent_clearance,
                                )
                                .ok()
                            };

                            if let Some(reply_msg) = reply_msg {
                                // Persist reply in sender DB for receipt tracking
                                {
                                    let conn = sender_node.db.lock().unwrap();
                                    let _ = conn.execute(
                                        "INSERT OR IGNORE INTO chat_messages (message_id, room_id, sender_id, content_ciphertext_hex, nonce_hex, ephemeral, timestamp, clearance_level, delivered, read) \
                                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                                        params![
                                            reply_msg.message_id,
                                            reply_msg.room_id,
                                            reply_msg.sender_id,
                                            reply_msg.content_ciphertext_hex,
                                            reply_msg.nonce_hex,
                                            if reply_msg.ephemeral { 1 } else { 0 },
                                            reply_msg.timestamp.to_rfc3339(),
                                            reply_msg.clearance_level.as_str(),
                                            0,
                                            0,
                                        ],
                                    );
                                }

                                metrics_ref
                                    .total_messages_sent
                                    .fetch_add(1, Ordering::Relaxed);
                                metrics_ref
                                    .agent_auto_responses
                                    .fetch_add(1, Ordering::Relaxed);
                                metrics_ref
                                    .encryption_successes
                                    .fetch_add(1, Ordering::Relaxed);

                                if sender_node.tx.send(reply_msg).await.is_ok() {
                                    metrics_ref
                                        .total_messages_received
                                        .fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        }
                    }
                }
            }
        });

        Ok(handle)
    }
}

/// Standalone test function: E2E 1-to-1 Node-to-Node Chat simulation.
pub async fn test_e2e_node_to_node_chat() -> Result<SimulationMetricsSnapshot> {
    let network = SimulatedNodeNetwork::new();
    let wallet_id = "wallet_telecom_e2e_001";

    let alice = VirtualNode::new("alice", wallet_id, ClearanceLevel::Confidential, false)?;
    let bob = VirtualNode::new("bob", wallet_id, ClearanceLevel::Confidential, false)?;

    let alice_node = network.add_node(alice).await;
    let bob_node = network.add_node(bob).await;

    let mut bob_rx = bob_node.take_receiver().await.unwrap();

    // Alice sends encrypted message to Bob
    let secret_payload = "Encrypted telemetry status: operational";
    let (msg, room_id) = network
        .send_message(
            &alice_node.node_id,
            &bob_node.node_id,
            secret_payload,
            ClearanceLevel::Confidential,
            false,
        )
        .await?;

    // Bob receives message from Tokio channel
    let received_msg = bob_rx
        .recv()
        .await
        .ok_or_else(|| anyhow!("Bob failed to receive message"))?;
    assert_eq!(received_msg.message_id, msg.message_id);

    // Decrypt and verify payload content
    let decrypted = decrypt_chat_message(&room_id, &received_msg)?;
    assert_eq!(decrypted, secret_payload);

    // Bob sends delivery receipt acknowledgement
    let receipt = {
        let conn = bob_node.db.lock().unwrap();
        acknowledge_receipt(&conn, &received_msg.message_id, "bob")?
    };
    assert_eq!(receipt.status, ReceiptStatus::Delivered);
    network
        .metrics()
        .receipts_acknowledged
        .fetch_add(1, Ordering::Relaxed);

    // Bob marks message as read
    let read_rcpt = {
        let conn = bob_node.db.lock().unwrap();
        mark_read(&conn, &room_id, &received_msg.message_id, "bob")?
    };
    assert_eq!(read_rcpt.status, ReceiptStatus::Read);
    network
        .metrics()
        .receipts_acknowledged
        .fetch_add(1, Ordering::Relaxed);

    Ok(network.metrics().snapshot())
}

/// Standalone test function: E2E Autonomous Agent Auto-Responder simulation.
pub async fn test_e2e_agent_auto_responder() -> Result<SimulationMetricsSnapshot> {
    let network = SimulatedNodeNetwork::new();
    let wallet_id = "wallet_agent_e2e_002";

    let alice = VirtualNode::new("alice", wallet_id, ClearanceLevel::Secret, false)?;
    let agent = VirtualNode::new("xavier-agent", wallet_id, ClearanceLevel::Secret, true)?;

    let alice_node = network.add_node(alice).await;
    network.add_node(agent).await;

    let mut alice_rx = alice_node.take_receiver().await.unwrap();

    // Start Xavier Agent auto-responder loop
    let handle = network.start_agent_auto_responder("xavier-agent").await?;

    // Alice sends prompt message to Agent
    let prompt = "Query memory node status for segment 10";
    let (_msg, _room_id) = network
        .send_message(
            "alice",
            "xavier-agent",
            prompt,
            ClearanceLevel::Secret,
            false,
        )
        .await?;

    // Alice receives Agent's auto-reply
    let reply = tokio::time::timeout(std::time::Duration::from_secs(2), alice_rx.recv())
        .await?
        .ok_or_else(|| anyhow!("Timeout waiting for agent reply"))?;

    let decrypted_reply = decrypt_chat_message(&reply.room_id, &reply)?;
    assert!(decrypted_reply.contains("Xavier Agent ACK"));
    assert!(decrypted_reply.contains(prompt));

    handle.abort();
    Ok(network.metrics().snapshot())
}

/// Standalone test function: E2E Security Clearance Blocking simulation.
pub async fn test_e2e_clearance_blocking() -> Result<SimulationMetricsSnapshot> {
    let network = SimulatedNodeNetwork::new();
    let wallet_id = "wallet_security_e2e_003";

    // Alice has TopSecret clearance, Bob only has Unclassified clearance
    let alice = VirtualNode::new("alice", wallet_id, ClearanceLevel::TopSecret, false)?;
    let bob = VirtualNode::new("bob", wallet_id, ClearanceLevel::Unclassified, false)?;

    network.add_node(alice).await;
    network.add_node(bob).await;

    // Alice attempts to send TopSecret payload to Bob
    let res = network
        .send_message(
            "alice",
            "bob",
            "TOPSECRET military mesh key",
            ClearanceLevel::TopSecret,
            false,
        )
        .await;

    assert!(
        res.is_err(),
        "Clearance gate must block message delivery to under-cleared node"
    );
    let err_str = res.unwrap_err().to_string();
    assert!(err_str.contains("Access denied") || err_str.contains("ClearanceDenied"));

    let snapshot = network.metrics().snapshot();
    assert_eq!(snapshot.blocked_clearance_messages, 1);

    Ok(snapshot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_simulation_network_setup_and_wallet_isolation() {
        let network = SimulatedNodeNetwork::new();

        let node_a =
            VirtualNode::new("node-a", "wallet-1", ClearanceLevel::Internal, false).unwrap();
        let node_b =
            VirtualNode::new("node-b", "wallet-2", ClearanceLevel::Internal, false).unwrap();

        network.add_node(node_a).await;
        network.add_node(node_b).await;

        // Cross-wallet send must be rejected with WalletMismatch
        let res = network
            .send_message(
                "node-a",
                "node-b",
                "Cross wallet payload",
                ClearanceLevel::Internal,
                false,
            )
            .await;

        assert!(res.is_err());
        let err_msg = res.unwrap_err().to_string();
        assert!(
            err_msg.contains("Wallet isolation violation") || err_msg.contains("WalletMismatch")
        );
    }

    #[tokio::test]
    async fn test_e2e_node_to_node_chat_flow() {
        let snapshot = test_e2e_node_to_node_chat()
            .await
            .expect("Node to node chat simulation should succeed");

        assert_eq!(snapshot.total_messages_sent, 1);
        assert_eq!(snapshot.total_messages_received, 1);
        assert_eq!(snapshot.encryption_successes, 1);
        assert_eq!(snapshot.receipts_acknowledged, 2);
    }

    #[tokio::test]
    async fn test_e2e_agent_auto_responder_flow() {
        let snapshot = test_e2e_agent_auto_responder()
            .await
            .expect("Agent auto-responder simulation should succeed");

        assert_eq!(snapshot.total_messages_sent, 2); // 1 prompt + 1 response
        assert_eq!(snapshot.total_messages_received, 2);
        assert_eq!(snapshot.agent_auto_responses, 1);
        assert_eq!(snapshot.encryption_successes, 2);
    }

    #[tokio::test]
    async fn test_e2e_clearance_blocking_flow() {
        let snapshot = test_e2e_clearance_blocking()
            .await
            .expect("Clearance blocking simulation should complete");

        assert_eq!(snapshot.blocked_clearance_messages, 1);
        assert_eq!(snapshot.total_messages_sent, 0);
    }
}
