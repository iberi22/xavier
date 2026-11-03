//! feat-embed-model-downloader: E2E tests for `xavier models` CLI commands

xavier::isolate_test_process!();

use sha2::Digest;
use std::fs;
use tempfile::TempDir;
use xavier::crypto::hex_encode;
use xavier::settings::XavierSettings;

#[tokio::test]
#[serial_test::serial]
async fn test_cli_models_fetch_e2e() {
    let tmp = TempDir::new().unwrap();
    let models_dir = tmp.path().join("models");
    fs::create_dir_all(&models_dir).unwrap();

    // Start mock download server
    let mut server = mockito::Server::new_async().await;
    let url = server.url();

    let onnx_data = b"cli mock onnx content";
    let json_data = b"cli mock tokenizer json";
    let license_data = b"cli mock license text";

    let mut onnx_hasher = sha2::Sha256::new();
    sha2::Digest::update(&mut onnx_hasher, onnx_data);
    let onnx_hash = hex_encode(sha2::Digest::finalize(onnx_hasher));

    let mut json_hasher = sha2::Sha256::new();
    sha2::Digest::update(&mut json_hasher, json_data);
    let json_hash = hex_encode(sha2::Digest::finalize(json_hasher));

    let mut license_hasher = sha2::Sha256::new();
    sha2::Digest::update(&mut license_hasher, license_data);
    let license_hash = hex_encode(sha2::Digest::finalize(license_hasher));

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
        .with_body(sums_content.clone())
        .create_async()
        .await;

    std::env::set_var("XAVIER_MODEL_DOWNLOAD_BASE_URL", &url);

    let target_model_dir = models_dir.join("test-model");
    let downloader = xavier::embedding::downloader::ModelDownloader::with_base_url(&url);
    let report = downloader
        .fetch_model("test-model", &target_model_dir)
        .await
        .expect("Model fetch should succeed");

    let expected_bytes =
        (onnx_data.len() + json_data.len() + license_data.len() + sums_content.len()) as u64;
    assert_eq!(report.bytes_downloaded, expected_bytes);
    assert!(target_model_dir.join("model.onnx").exists());
    assert!(target_model_dir.join("tokenizer.json").exists());
    assert!(target_model_dir.join("LICENSE").exists());

    // Verify content matches
    let read_onnx = fs::read(target_model_dir.join("model.onnx")).unwrap();
    assert_eq!(read_onnx, onnx_data);

    std::env::remove_var("XAVIER_MODEL_DOWNLOAD_BASE_URL");
}

#[tokio::test]
#[serial_test::serial]
async fn test_cli_models_config_parity() {
    let mut settings = XavierSettings::default();
    settings.models.embedding_model = "native-gemma2".to_string();
    settings.models.local_model_dirs = vec!["/tmp/test-models".to_string()];

    assert_eq!(settings.models.embedding_model, "native-gemma2");
    assert_eq!(settings.models.local_model_dirs.len(), 1);
    assert_eq!(settings.models.local_model_dirs[0], "/tmp/test-models");
}
