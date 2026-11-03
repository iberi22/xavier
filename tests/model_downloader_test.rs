xavier::isolate_test_process!();

use sha2::{Digest, Sha256};
use std::fs;
use tempfile::tempdir;
use xavier::crypto::hex_encode;
use xavier::embedding::downloader::{DownloaderError, ModelDownloader};

#[tokio::test]
async fn test_mock_download_success_and_atomic_rename() {
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    // Create mock files to be downloaded
    let onnx_data = b"mock onnx content";
    let json_data = b"mock tokenizer content";
    let license_data = b"mock license content";

    // We compute what the SHA256 should be
    let mut onnx_hasher = Sha256::new();
    onnx_hasher.update(onnx_data);
    let onnx_hash = hex_encode(onnx_hasher.finalize());

    let mut json_hasher = Sha256::new();
    json_hasher.update(json_data);
    let json_hash = hex_encode(json_hasher.finalize());

    let mut license_hasher = Sha256::new();
    license_hasher.update(license_data);
    let license_hash = hex_encode(license_hasher.finalize());

    let sums_content = format!(
        "{}  model.onnx\n{}  tokenizer.json\n{}  LICENSE\n",
        onnx_hash, json_hash, license_hash
    );

    let _m1 = server
        .mock("GET", "/model.onnx")
        .with_body(onnx_data)
        .create_async()
        .await;
    let _m2 = server
        .mock("GET", "/tokenizer.json")
        .with_body(json_data)
        .create_async()
        .await;
    let _m3 = server
        .mock("GET", "/LICENSE")
        .with_body(license_data)
        .create_async()
        .await;
    let _m4 = server
        .mock("GET", "/SHA256SUMS")
        .with_body(sums_content)
        .create_async()
        .await;

    let dir = tempdir().unwrap();
    let target_dir = dir.path().join("embeddinggemma-2");

    let downloader = ModelDownloader::with_base_url(&url);
    let report = downloader
        .fetch_model("embeddinggemma-2", &target_dir)
        .await
        .expect("Download should succeed");

    assert_eq!(report.model_name, "embeddinggemma-2");
    assert_eq!(report.target_dir, target_dir);

    assert!(target_dir.join("model.onnx").exists());
    assert!(target_dir.join("tokenizer.json").exists());
    assert!(target_dir.join("LICENSE").exists());
    assert!(target_dir.join("SHA256SUMS").exists());

    assert!(!target_dir.join("model.onnx.tmp").exists());
}

#[tokio::test]
async fn test_sha256_mismatch() {
    let dir = tempdir().unwrap();
    let target_dir = dir.path().join("embeddinggemma-2");
    fs::create_dir_all(&target_dir).unwrap();

    let invalid_sums = "invalid_hash  model.onnx\n";
    fs::write(target_dir.join("SHA256SUMS"), invalid_sums).unwrap();
    fs::write(target_dir.join("model.onnx.tmp"), b"some data").unwrap();

    let downloader = ModelDownloader::new();
    let result = downloader.verify_integrity(&target_dir);

    match result {
        Err(DownloaderError::ChecksumMismatch(file)) => {
            assert_eq!(file, "model.onnx");
        }
        _ => panic!("Expected ChecksumMismatch error"),
    }

    assert!(!target_dir.join("model.onnx.tmp").exists());
}

#[tokio::test]
async fn test_missing_directory_creation() {
    let dir = tempdir().unwrap();
    let target_dir = dir.path().join("deep").join("nested").join("dir");

    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    // Mock empty files to pass hash tests trivially (with empty hashes) or just expect error if sums fail
    let _m = server
        .mock("GET", mockito::Matcher::Any)
        .with_status(404)
        .create_async()
        .await;

    let downloader = ModelDownloader::with_base_url(&url);
    // It should create the directory and then fail network or verification
    let _ = downloader
        .fetch_model("embeddinggemma-2", &target_dir)
        .await;

    assert!(target_dir.exists());
}

#[tokio::test]
async fn test_insufficient_disk_space() {
    let dir = tempdir().unwrap();
    let target_dir = dir.path().join("full_disk").join("model");

    let downloader = ModelDownloader::new();
    let result = downloader
        .fetch_model("embeddinggemma-2", &target_dir)
        .await;

    match result {
        Err(DownloaderError::InsufficientDiskSpace) => (),
        _ => panic!("Expected InsufficientDiskSpace error"),
    }
}
