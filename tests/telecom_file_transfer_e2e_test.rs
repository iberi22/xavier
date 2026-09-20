use anyhow::Result;
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::fmt::Write;
use tokio::sync::mpsc;
use xavier::telecom::file_transfer::{ChunkManifest, FileChunk, FileTransferManager};

fn to_hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        let _ = write!(s, "{:02x}", b);
    }
    s
}

/// Simulation events between peers.
#[derive(Debug, Clone)]
enum PeerMessage {
    Manifest(ChunkManifest),
    Chunk(FileChunk),
    RequestRetransmit(Vec<usize>),
    TransferComplete,
}

#[tokio::test]
async fn test_e2e_10mb_file_transfer_with_dropped_chunks_recovery() -> Result<()> {
    // Two peer session channels for bidirectional communication
    let (tx_sender, mut rx_receiver) = mpsc::channel::<PeerMessage>(100);
    let (tx_receiver, mut rx_sender) = mpsc::channel::<PeerMessage>(100);

    // 1. Generate 10MB synthetic payload
    let payload_size = 10 * 1024 * 1024;
    let mut payload = vec![0u8; payload_size];
    for i in 0..payload_size {
        payload[i] = (i % 256) as u8;
    }

    // 2. Compute full SHA256 hash
    let mut hasher = Sha256::new();
    hasher.update(&payload);
    let full_hash = to_hex(&hasher.finalize());
    let full_hash_clone = full_hash.clone();

    let session_id = "sess_e2e_10mb_drop_recovery".to_string();
    let chunk_size = 1024 * 1024; // 1MB chunks => 10 chunks total
    let total_chunks = payload_size.div_ceil(chunk_size);
    let payload_sender = payload.clone();

    // SENDER PEER TASK
    let sender_handle = tokio::spawn({
        let session_id = session_id.clone();
        async move {
            let manifest = ChunkManifest {
                session_id: session_id.clone(),
                file_name: "synthetic_10mb_test.bin".to_string(),
                total_bytes: payload_size,
                chunk_size,
                total_chunks,
                file_sha256: full_hash_clone,
                created_at: Utc::now(),
            };

            // Send Manifest
            tx_sender
                .send(PeerMessage::Manifest(manifest))
                .await
                .unwrap();

            // Intentionally drop 3 chunks: indices 2, 5, 8
            let dropped_indices = vec![2, 5, 8];

            for (i, slice) in payload_sender.chunks(chunk_size).enumerate() {
                if dropped_indices.contains(&i) {
                    continue; // Skip sending, simulating drop
                }

                let mut c_hasher = Sha256::new();
                c_hasher.update(slice);
                let c_hash = to_hex(&c_hasher.finalize());

                let chunk = FileChunk {
                    session_id: session_id.clone(),
                    chunk_index: i,
                    total_chunks,
                    data: slice.to_vec(),
                    chunk_sha256: c_hash,
                };

                tx_sender.send(PeerMessage::Chunk(chunk)).await.unwrap();
            }

            // Wait for requests from receiver (e.g. retransmit requests)
            while let Some(msg) = rx_sender.recv().await {
                match msg {
                    PeerMessage::RequestRetransmit(missing) => {
                        for i in missing {
                            let start = i * chunk_size;
                            let end = std::cmp::min(start + chunk_size, payload_size);
                            let slice = &payload_sender[start..end];

                            let mut c_hasher = Sha256::new();
                            c_hasher.update(slice);
                            let c_hash = to_hex(&c_hasher.finalize());

                            let chunk = FileChunk {
                                session_id: session_id.clone(),
                                chunk_index: i,
                                total_chunks,
                                data: slice.to_vec(),
                                chunk_sha256: c_hash,
                            };

                            tx_sender.send(PeerMessage::Chunk(chunk)).await.unwrap();
                        }
                    }
                    PeerMessage::TransferComplete => {
                        break;
                    }
                    _ => {}
                }
            }
        }
    });

    // RECEIVER PEER TASK (Running in main thread)
    let manager = FileTransferManager::new();
    let mut assembled_data = None;

    // First receive the manifest
    if let Some(PeerMessage::Manifest(manifest)) = rx_receiver.recv().await {
        manager.init_transfer(manifest).await;
    } else {
        panic!("Expected Manifest");
    }

    // Attempt to collect chunks with a simulated timeout/poll for drops
    loop {
        // Use timeout to detect when sender has stopped sending the initial batch
        match tokio::time::timeout(std::time::Duration::from_millis(100), rx_receiver.recv()).await
        {
            Ok(Some(PeerMessage::Chunk(chunk))) => {
                manager
                    .assemble_chunk(chunk)
                    .await
                    .expect("Chunk assembly should succeed");
            }
            Ok(Some(_)) => panic!("Unexpected message"),
            Ok(None) => break, // Channel closed
            Err(_) => {
                // Timeout occurred, assume current batch is done.
                // Check if transfer is complete.
                if let Ok(data) = manager.finalize_transfer(&session_id).await {
                    assembled_data = Some(data);
                    tx_receiver
                        .send(PeerMessage::TransferComplete)
                        .await
                        .unwrap();
                    break;
                } else {
                    // Not complete, request retransmission
                    let missing = manager.resume_transfer(&session_id).await.unwrap();
                    if !missing.is_empty() {
                        tx_receiver
                            .send(PeerMessage::RequestRetransmit(missing))
                            .await
                            .unwrap();
                    }
                }
            }
        }
    }

    sender_handle.await.unwrap();

    // Verify
    let final_data = assembled_data.expect("Should have successfully assembled data");

    // Exact match byte-for-byte
    assert_eq!(final_data.len(), payload.len());
    assert_eq!(final_data, payload);

    // Explicit SHA-256 validation (though manager internal finalization does this as well)
    let mut final_hasher = Sha256::new();
    final_hasher.update(&final_data);
    let final_hash = to_hex(&final_hasher.finalize());
    assert_eq!(final_hash, full_hash);

    Ok(())
}
