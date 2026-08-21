//! Integration and edge coverage test suite for SQLite WAL Streamer & Backup Recovery.

use std::io::{Cursor, ErrorKind, Write};
use tempfile::NamedTempFile;
use xavier::storage::backup::wal_streamer::{
    wal_checksum, SimulatedIoReader, WalError, WalFrame, WalFrameHeader, WalHeader,
    WalRecoveryManager, WalStreamer, WalStreamerConfig, WAL_MAGIC_BE, WAL_MAGIC_LE,
};

fn helper_create_valid_wal_bytes(
    page_size: u32,
    num_frames: usize,
    is_big_endian: bool,
) -> (Vec<u8>, WalHeader, Vec<WalFrame>) {
    let salt_1 = 0x12345678;
    let salt_2 = 0x9abcdef0;
    let checkpoint_seq = 1;

    let magic = if is_big_endian {
        WAL_MAGIC_BE
    } else {
        WAL_MAGIC_LE
    };
    let file_format: u32 = 3000000;

    let mut header_buf = [0u8; 32];
    let put_u32 = |val: u32, dst: &mut [u8]| {
        if is_big_endian {
            dst.copy_from_slice(&val.to_be_bytes());
        } else {
            dst.copy_from_slice(&val.to_le_bytes());
        }
    };

    header_buf[0..4].copy_from_slice(&magic.to_be_bytes());
    put_u32(file_format, &mut header_buf[4..8]);
    put_u32(page_size, &mut header_buf[8..12]);
    put_u32(checkpoint_seq, &mut header_buf[12..16]);
    put_u32(salt_1, &mut header_buf[16..20]);
    put_u32(salt_2, &mut header_buf[20..24]);

    let (c1, c2) = wal_checksum(&header_buf[0..24], is_big_endian, 0, 0);
    put_u32(c1, &mut header_buf[24..28]);
    put_u32(c2, &mut header_buf[28..32]);

    let wal_header = WalHeader::parse(&header_buf).expect("Header parse failed");

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&header_buf);

    let mut frames = Vec::new();
    let mut prev_cksum = (c1, c2);

    for idx in 0..num_frames {
        let page_no = (idx + 1) as u32;
        let db_pages = if idx == num_frames - 1 {
            num_frames as u32
        } else {
            0
        };
        let mut page_data = vec![idx as u8 + 1; page_size as usize];
        page_data[0] = page_no as u8;

        let (frame_hdr, next_cksum) = WalFrameHeader::create_valid(
            page_no,
            db_pages,
            salt_1,
            salt_2,
            prev_cksum,
            &page_data,
            is_big_endian,
        );

        prev_cksum = next_cksum;

        bytes.extend_from_slice(&frame_hdr.to_bytes(is_big_endian));
        bytes.extend_from_slice(&page_data);

        frames.push(WalFrame {
            header: frame_hdr,
            page_data,
            frame_index: idx,
        });
    }

    (bytes, wal_header, frames)
}

#[test]
fn test_wal_header_valid_little_endian() {
    let header = WalHeader::new(4096, 1, 0x11223344, 0x55667788).unwrap();
    assert_eq!(header.page_size, 4096);
    assert!(!header.is_big_endian());

    let bytes = header.to_bytes();
    let parsed = WalHeader::parse(&bytes).unwrap();
    assert_eq!(header, parsed);
}

#[test]
fn test_wal_header_valid_big_endian() {
    let (bytes, header, _) = helper_create_valid_wal_bytes(4096, 0, true);
    let buf: [u8; 32] = bytes[0..32].try_into().unwrap();
    let parsed = WalHeader::parse(&buf).unwrap();

    assert!(parsed.is_big_endian());
    assert_eq!(parsed.page_size, 4096);
    assert_eq!(header, parsed);
}

#[test]
fn test_wal_header_invalid_magic() {
    let mut buf = [0u8; 32];
    buf[0..4].copy_from_slice(&0xdeadbeefu32.to_be_bytes());
    let err = WalHeader::parse(&buf).unwrap_err();
    assert_eq!(err, WalError::InvalidMagicHeader(0xdeadbeef));
}

#[test]
fn test_wal_header_invalid_page_size() {
    // Too small < 512
    let err = WalHeader::new(256, 1, 10, 20).unwrap_err();
    assert_eq!(err, WalError::InvalidPageSize(256));

    // Not power of two
    let err = WalHeader::new(3000, 1, 10, 20).unwrap_err();
    assert_eq!(err, WalError::InvalidPageSize(3000));

    // Too large > 65536
    let err = WalHeader::new(131072, 1, 10, 20).unwrap_err();
    assert_eq!(err, WalError::InvalidPageSize(131072));
}

#[test]
fn test_wal_header_checksum_mismatch() {
    let header = WalHeader::new(4096, 1, 0x11223344, 0x55667788).unwrap();
    let mut bytes = header.to_bytes();
    bytes[28] ^= 0xff; // Corrupt checksum

    let err = WalHeader::parse(&bytes).unwrap_err();
    assert!(matches!(err, WalError::HeaderChecksumMismatch { .. }));
}

#[test]
fn test_wal_frame_valid_sync() {
    let (bytes, _hdr, frames) = helper_create_valid_wal_bytes(4096, 3, false);
    let cursor = Cursor::new(bytes);
    let mut streamer = WalStreamer::new(cursor, WalStreamerConfig::default());

    let header = streamer.read_header_sync().unwrap();
    assert_eq!(header.page_size, 4096);

    let read_frames = streamer.read_all_frames_sync().unwrap();
    assert_eq!(read_frames.len(), 3);
    assert_eq!(read_frames, frames);
    assert_eq!(streamer.current_frame_index(), 3);
    assert_eq!(streamer.bytes_read(), (32 + 3 * (24 + 4096)) as u64);
}

#[tokio::test]
async fn test_wal_frame_valid_async() {
    let (bytes, _hdr, frames) = helper_create_valid_wal_bytes(4096, 2, false);
    let cursor = Cursor::new(bytes);
    let mut streamer = WalStreamer::new(cursor, WalStreamerConfig::default());

    let header = streamer.read_header_async().await.unwrap();
    assert_eq!(header.page_size, 4096);

    let read_frames = streamer.read_all_frames_async().await.unwrap();
    assert_eq!(read_frames.len(), 2);
    assert_eq!(read_frames, frames);
    assert_eq!(streamer.current_frame_index(), 2);
}

#[test]
fn test_wal_frame_salt_mismatch() {
    let (mut bytes, _hdr, _frames) = helper_create_valid_wal_bytes(4096, 2, false);
    // Frame 1 header starts at byte 32. Salt is at offset 8 (bytes 40..48)
    bytes[40] ^= 0xff;

    let cursor = Cursor::new(bytes);
    let mut streamer = WalStreamer::new(cursor, WalStreamerConfig::default());
    let err = streamer.next_frame_sync().unwrap_err();

    assert!(matches!(err, WalError::SaltMismatch { frame_index: 0, .. }));
}

#[test]
fn test_wal_frame_checksum_mismatch() {
    let (mut bytes, _hdr, _frames) = helper_create_valid_wal_bytes(4096, 2, false);
    // Corrupt page data of frame 1 (starts at byte 32 + 24 = 56)
    bytes[60] ^= 0xff;

    let cursor = Cursor::new(bytes);
    let mut streamer = WalStreamer::new(cursor, WalStreamerConfig::default());
    let err = streamer.next_frame_sync().unwrap_err();

    assert!(matches!(
        err,
        WalError::FrameChecksumMismatch { frame_index: 0, .. }
    ));
}

#[test]
fn test_wal_frame_incomplete_page_data() {
    let (mut bytes, _hdr, _frames) = helper_create_valid_wal_bytes(4096, 1, false);
    // Truncate bytes in the middle of page payload
    bytes.truncate(32 + 24 + 100);

    let cursor = Cursor::new(bytes);
    let mut streamer = WalStreamer::new(cursor, WalStreamerConfig::default());
    let err = streamer.next_frame_sync().unwrap_err();

    assert!(matches!(
        err,
        WalError::CorruptedFrameHeader { frame_index: 0, .. }
    ));
}

#[test]
fn test_simulated_io_disconnect_header_sync() {
    let (bytes, _hdr, _frames) = helper_create_valid_wal_bytes(4096, 1, false);
    let simulated = SimulatedIoReader::new(Cursor::new(bytes)).with_error_after_bytes(
        10,
        ErrorKind::ConnectionReset,
        "Simulated network drop",
    );

    let mut streamer = WalStreamer::new(simulated, WalStreamerConfig::default());
    let err = streamer.read_header_sync().unwrap_err();

    assert!(matches!(err, WalError::StreamInterrupted(_)));
}

#[test]
fn test_simulated_io_disconnect_frame_sync() {
    let (bytes, _hdr, _frames) = helper_create_valid_wal_bytes(4096, 2, false);
    // Fail after header (32 bytes) + frame 1 header (24 bytes) + partial page (100 bytes)
    let simulated = SimulatedIoReader::new(Cursor::new(bytes)).with_error_after_bytes(
        32 + 24 + 100,
        ErrorKind::ConnectionReset,
        "Simulated connection reset",
    );

    let mut streamer = WalStreamer::new(simulated, WalStreamerConfig::default());
    streamer.read_header_sync().unwrap();

    let err = streamer.next_frame_sync().unwrap_err();
    assert!(matches!(err, WalError::StreamInterrupted(_)));
}

#[tokio::test]
async fn test_simulated_io_disconnect_frame_async() {
    let (bytes, _hdr, _frames) = helper_create_valid_wal_bytes(4096, 2, false);
    // Fail during frame 1 reading
    let simulated = SimulatedIoReader::new(Cursor::new(bytes)).with_error_after_bytes(
        40,
        ErrorKind::ConnectionReset,
        "Async stream reset",
    );

    let mut streamer = WalStreamer::new(simulated, WalStreamerConfig::default());
    streamer.read_header_async().await.unwrap();

    let err = streamer.next_frame_async().await.unwrap_err();
    assert!(matches!(err, WalError::StreamInterrupted(_)));
}

#[test]
fn test_simulated_io_lock_timeout() {
    let (bytes, _hdr, _frames) = helper_create_valid_wal_bytes(4096, 1, false);
    let simulated = SimulatedIoReader::new(Cursor::new(bytes)).with_error_after_bytes(
        0,
        ErrorKind::WouldBlock,
        "SQLite WAL lock timeout",
    );

    let mut streamer = WalStreamer::new(simulated, WalStreamerConfig::default());
    let err = streamer.read_header_sync().unwrap_err();

    assert!(matches!(err, WalError::LockTimeout(_)));
}

#[test]
fn test_wal_recovery_manager_bytes_recovery() {
    let (mut bytes, _hdr, valid_frames) = helper_create_valid_wal_bytes(4096, 3, false);
    // Corrupt frame 3 header (frame 3 header starts at 32 + 2*(24+4096) = 32 + 8240 = 8272)
    let frame_3_start = 32 + 2 * (24 + 4096);
    bytes[frame_3_start + 10] ^= 0xff; // corrupt salt

    let (recovered_frames, summary) =
        WalRecoveryManager::recover_wal_bytes(&bytes, WalStreamerConfig::default()).unwrap();

    assert_eq!(recovered_frames.len(), 2);
    assert_eq!(recovered_frames, valid_frames[0..2]);
    assert_eq!(summary.valid_frames_count, 2);
    assert_eq!(summary.corrupted_frames_count, 1);
    assert_eq!(summary.last_valid_frame_index, Some(1));
    assert_eq!(summary.bytes_processed, (32 + 2 * (24 + 4096)) as u64);
}

#[test]
fn test_wal_recovery_manager_apply_frames_to_db() {
    let (_bytes, _hdr, frames) = helper_create_valid_wal_bytes(4096, 2, false);

    let temp_file = NamedTempFile::new().unwrap();
    let db_path = temp_file.path();

    let applied = WalRecoveryManager::apply_frames_to_db(db_path, &frames, 4096).unwrap();
    assert_eq!(applied, 2);

    let file_data = std::fs::read(db_path).unwrap();
    assert_eq!(file_data.len(), 2 * 4096);
    assert_eq!(&file_data[0..4096], &frames[0].page_data);
    assert_eq!(&file_data[4096..8192], &frames[1].page_data);
}

#[test]
fn test_wal_recovery_manager_repair_corrupt_wal_file() {
    let (mut bytes, _hdr, _) = helper_create_valid_wal_bytes(4096, 3, false);
    let corrupt_offset = 32 + 2 * (24 + 4096);
    bytes[corrupt_offset] ^= 0xff; // Corrupt frame 3

    let mut temp_wal = NamedTempFile::new().unwrap();
    temp_wal.write_all(&bytes).unwrap();
    temp_wal.flush().unwrap();

    let wal_path = temp_wal.path();
    let summary =
        WalRecoveryManager::repair_corrupt_wal_file(wal_path, WalStreamerConfig::default())
            .unwrap();

    assert_eq!(summary.valid_frames_count, 2);
    let repaired_bytes = std::fs::read(wal_path).unwrap();
    assert_eq!(repaired_bytes.len() as u64, summary.bytes_processed);
}

#[test]
fn test_wal_streamer_non_strict_checksum() {
    let (mut bytes, _hdr, _frames) = helper_create_valid_wal_bytes(4096, 1, false);
    // Corrupt page data of frame 1
    bytes[60] ^= 0xff;

    let cursor = Cursor::new(bytes);
    let mut config = WalStreamerConfig::default();
    config.strict_checksum = false;

    let mut streamer = WalStreamer::new(cursor, config);
    let frame = streamer.next_frame_sync().unwrap().unwrap();
    assert_eq!(frame.frame_index, 0);
}

#[test]
fn test_wal_checksum_edge_cases() {
    // Non-multiple of 8
    let unaligned = [1u8, 2, 3, 4, 5];
    let (s1, s2) = wal_checksum(&unaligned, false, 10, 20);
    assert_eq!((s1, s2), (10, 20));
}

#[test]
fn test_wal_error_formatting_and_equality() {
    let err1 = WalError::InvalidMagicHeader(0x12345678);
    let err2 = WalError::InvalidMagicHeader(0x12345678);
    assert_eq!(err1, err2);
    assert_eq!(format!("{}", err1), "Invalid WAL magic header: 0x12345678");

    let err_eof = WalError::UnexpectedEof;
    assert_eq!(format!("{}", err_eof), "Unexpected end of WAL stream");
}
