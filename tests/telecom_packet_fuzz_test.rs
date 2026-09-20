use proptest::prelude::*;
use xavier::telecom::crypto::EncryptedTelecomFrame;
use xavier::telecom::protocol::{
    decode_packet, encode_packet, TelecomPacket, TelecomProtocolError,
};

// Generate an initial valid frame to be modified
fn create_valid_packet() -> TelecomPacket {
    let frame = EncryptedTelecomFrame {
        nonce: [1u8; xavier::crypto::NONCE_SIZE],
        ciphertext: vec![10, 20, 30, 40],
        sequence: 42,
        channel_id: "test-room".to_string(),
    };
    TelecomPacket::new_chat_packet("node-alpha", "node-beta", 101, 1710000000, frame)
        .expect("valid packet created")
}

proptest! {
    #[test]
    fn test_fuzz_arbitrary_bytes(bytes in any::<Vec<u8>>()) {
        // We just ensure that decoding random bytes does NOT panic.
        // It might return a DecodeError, ChecksumMismatch, etc.
        let _ = decode_packet(&bytes);
    }
}

#[test]
fn test_truncated_header_fuzz() {
    let packet = create_valid_packet();
    let encoded = encode_packet(&packet).unwrap();

    // Test all possible truncations
    for i in 0..encoded.len() {
        let truncated = &encoded[0..i];
        let result = decode_packet(truncated);
        // Should gracefully fail without panicking
        assert!(matches!(result, Err(TelecomProtocolError::DecodeError(_))));
    }
}

#[test]
fn test_invalid_crc32_checksum() {
    let mut packet = create_valid_packet();
    // Intentionally corrupt the checksum
    packet.header.payload_checksum ^= 0xDEADBEEF;
    let encoded = encode_packet(&packet).unwrap();

    let result = decode_packet(&encoded);
    assert!(matches!(
        result,
        Err(TelecomProtocolError::ChecksumMismatch { .. })
    ));
}

#[test]
fn test_corrupted_chacha_tags() {
    let mut packet = create_valid_packet();
    // Simulate corrupted ChaCha tag by modifying the ciphertext of an EncryptedFrame
    if let xavier::telecom::protocol::PacketPayload::EncryptedFrame(ref mut frame) = packet.payload
    {
        if !frame.ciphertext.is_empty() {
            frame.ciphertext[0] ^= 0xFF;
        }
    }

    // We expect the protocol to decode this fine at the framing level,
    // but the payload checksum should catch the discrepancy because we mutated the inner payload.
    // However, if we recalculate the checksum, framing passes, but encryption layer would fail.
    // Since we are fuzzing telecom protocol, mutating payload WITHOUT mutating checksum tests ChecksumMismatch.
    let encoded = encode_packet(&packet).unwrap();
    let result = decode_packet(&encoded);

    assert!(matches!(
        result,
        Err(TelecomProtocolError::ChecksumMismatch { .. })
    ));
}

#[test]
fn test_oversized_lengths_graceful_error() {
    let mut packet = create_valid_packet();
    // Try to create a massive dummy payload string to simulate oversized length
    let giant_string = String::from_utf8(vec![b'A'; 10 * 1024 * 1024]).unwrap(); // 10MB string
    packet.header.sender_node_id = giant_string;

    let encoded = encode_packet(&packet).unwrap();

    // Decoding an unusually large string should either pass (if memory allows) or gracefully OOM/DecodeError
    // Serde JSON will try to parse it. We verify it doesn't hard panic.
    let _ = decode_packet(&encoded);
}
