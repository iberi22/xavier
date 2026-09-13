//! E2E — Multimodal RAG & Expert Training Pipeline Test Suite.
//!
//! Validates the end-to-end flow:
//! 1. Hardware scan & allocation curves detection.
//! 2. Structural legal document chunking with breadcrumb anchors & audio transcription merging.
//! 3. Privacy scrubbing pass through PrivacyPipeline (redacting emails, API keys, and file paths).
//! 4. Multimodal vector store storage (SQLite vec) and query retrieval without PII leaks.
//! 5. Training export preparation with privacy scrubbing and differential privacy (Laplace noise).

use rusqlite::Connection;
use serde_json::json;
use xavier::data_commons::privacy::{PrivacyLevel, PrivacyPipeline};
use xavier::documents::audio_transcriber::{AudioChunkCombiner, AudioTranscriptSegment};
use xavier::documents::legal_chunker::LegalHierarchicalChunker;
use xavier::memory::multimodal_vec_store::{DataTypeKind, MultimodalVecManager};
use xavier::system::hardware_scanner::{HardwareMetrics, HostTier};

#[test]
fn test_e2e_hardware_scan_and_allocation_curves() {
    // 1. Detect current host metrics
    let detected = HardwareMetrics::detect_current().expect("Hardware detection should succeed");
    assert!(detected.cpu_cores >= 1);

    let tier = detected.host_tier();
    let alloc = detected.recommended_allocation();

    // Verify allocation bounds
    assert!(alloc.max_vector_cache_mb >= 64);
    assert!(alloc.batch_size >= 16);
    assert!(alloc.parallel_chunkers >= 1);

    match tier {
        HostTier::Constrained => {
            assert!(alloc.quantization_recommended);
        }
        HostTier::Standard => {
            assert!(alloc.max_vector_cache_mb >= 256);
        }
        HostTier::Powerhouse => {
            assert!(alloc.max_vector_cache_mb >= 1024);
            assert_eq!(alloc.batch_size, 128);
            assert!(!alloc.quantization_recommended);
        }
    }

    // 2. Synthetic metrics edge case verification
    let synthetic_constrained = HardwareMetrics {
        cpu_cores: 2,
        total_ram_bytes: 4 * 1024 * 1024 * 1024,
        available_ram_bytes: 1024 * 1024 * 1024,
        has_gpu: false,
        gpu_vram_bytes: None,
        disk_available_bytes: 10 * 1024 * 1024 * 1024,
    };
    assert_eq!(synthetic_constrained.host_tier(), HostTier::Constrained);
    let syn_alloc = synthetic_constrained.recommended_allocation();
    assert_eq!(syn_alloc.max_vector_cache_mb, 128);
    assert_eq!(syn_alloc.batch_size, 16);
    assert!(syn_alloc.quantization_recommended);
}

#[test]
fn test_e2e_legal_chunking_and_privacy_redaction() {
    let raw_contract = r#"
CAPÍTULO I. DE LAS OBLIGACIONES
ARTÍCULO 1. PARTES Y CONTACTO
El contratante con correo legal@swal.org se compromete a proveer acceso a los servidores en /etc/config/secret.key.

CLÁUSULA SEGUNDA. CREDENCIALES DE API
Cualquier integración con el LLM utilizará la clave de API sk-1234567890123456789012345 para llamadas autenticadas.

PARÁGRAFO 1. NOTIFICACIONES
Todas las comunicaciones formales deben ser dirigidas a admin@company.com antes de vencer el plazo.
"#;

    let chunker = LegalHierarchicalChunker::new(500, 50);
    let chunks = chunker.chunk_document("doc_e2e_contract_01", raw_contract);

    assert!(
        chunks.len() >= 2,
        "Expected at least 2 structured chunks, got {}",
        chunks.len()
    );

    // Verify breadcrumb full_path hierarchy and clause numbers
    let art1 = chunks.iter().find(|c| c.clause_number == Some(1)).unwrap();
    assert!(art1.full_path.contains("ARTÍCULO 1"));
    assert_eq!(art1.document_id, "doc_e2e_contract_01");

    let clause2 = chunks.iter().find(|c| c.clause_number == Some(2)).unwrap();
    assert!(clause2.full_path.contains("CLÁUSULA SEGUNDA"));

    // Apply Privacy Redaction Pass
    let pipeline = PrivacyPipeline::new();
    let scrubbed_chunks: Vec<_> = chunks
        .into_iter()
        .map(|mut chunk| {
            chunk.text = pipeline.scrub_string(&chunk.text);
            chunk
        })
        .collect();

    for chunk in &scrubbed_chunks {
        assert!(
            !chunk.text.contains("legal@swal.org"),
            "PII email leaked in chunk text"
        );
        assert!(
            !chunk.text.contains("admin@company.com"),
            "PII email leaked in chunk text"
        );
        assert!(
            !chunk.text.contains("sk-1234567890123456789012345"),
            "API key leaked in chunk text"
        );
        assert!(
            !chunk.text.contains("/etc/config/secret.key"),
            "File path leaked in chunk text"
        );
    }

    // Assert replacement tokens are present
    let text_combined = scrubbed_chunks
        .iter()
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(text_combined.contains("[EMAIL]"));
    assert!(text_combined.contains("[API_KEY]"));
    assert!(text_combined.contains("[PATH]"));
}

#[test]
fn test_e2e_multimodal_vector_store_indexing_and_retrieval() {
    let conn = Connection::open_in_memory().expect("In-memory SQLite connection should succeed");
    let manager = MultimodalVecManager::new();

    // Setup isolated modality virtual vector tables
    manager
        .ensure_modality_table(&conn, DataTypeKind::Legal, 1536)
        .expect("Legal table initialization should succeed");
    manager
        .ensure_modality_table(&conn, DataTypeKind::Audio, 512)
        .expect("Audio table initialization should succeed");

    // 1. Process synthetic legal chunk
    let pipeline = PrivacyPipeline::new();
    let original_text = "Legal consultation for client contact@swal.ai regarding license keys sk-999988887777666655554444 in /opt/app/license.txt";
    let scrubbed_text = pipeline.scrub_string(original_text);

    let legal_vector: Vec<f32> = vec![0.1f32; 1536];
    let legal_payload = json!({
        "doc_id": "doc_legal_01",
        "text": scrubbed_text,
        "modality": "legal"
    })
    .to_string();

    manager
        .insert_modality_embedding(
            &conn,
            DataTypeKind::Legal,
            "legal_item_1",
            &legal_vector,
            &legal_payload,
        )
        .expect("Inserting legal vector should succeed");

    // 2. Process synthetic audio segment
    let segments = vec![
        AudioTranscriptSegment::new(
            Some("Deponent".to_string()),
            0.0,
            10.0,
            "I agree to the terms listed in the contract.",
            0.98,
        ),
        AudioTranscriptSegment::new(
            Some("Lawyer".to_string()),
            10.5,
            20.0,
            "Thank you. We will send confirmation to user@swal.org now.",
            0.95,
        ),
    ];
    let combiner = AudioChunkCombiner::new().with_min_words(5);
    let audio_chunks = combiner.combine_segments("audio_session_1", &segments);
    assert_eq!(audio_chunks.len(), 1);

    let audio_scrubbed_text = pipeline.scrub_string(&audio_chunks[0].formatted_text);
    let audio_vector: Vec<f32> = vec![0.2f32; 512];
    let audio_payload = json!({
        "audio_id": audio_chunks[0].audio_id,
        "text": audio_scrubbed_text,
        "modality": "audio"
    })
    .to_string();

    manager
        .insert_modality_embedding(
            &conn,
            DataTypeKind::Audio,
            "audio_item_1",
            &audio_vector,
            &audio_payload,
        )
        .expect("Inserting audio vector should succeed");

    // 3. Perform search on legal modality
    let search_legal_query: Vec<f32> = vec![0.1f32; 1536];
    let legal_results = manager
        .search_modality(&conn, DataTypeKind::Legal, &search_legal_query, 5)
        .expect("Searching legal modality should succeed");

    assert_eq!(legal_results.len(), 1);
    assert_eq!(legal_results[0].item_id, "legal_item_1");
    assert!(!legal_results[0].payload_json.contains("contact@swal.ai"));
    assert!(legal_results[0].payload_json.contains("[EMAIL]"));

    // 4. Perform search on audio modality
    let search_audio_query: Vec<f32> = vec![0.2f32; 512];
    let audio_results = manager
        .search_modality(&conn, DataTypeKind::Audio, &search_audio_query, 5)
        .expect("Searching audio modality should succeed");

    assert_eq!(audio_results.len(), 1);
    assert_eq!(audio_results[0].item_id, "audio_item_1");
    assert!(!audio_results[0].payload_json.contains("user@swal.org"));
    assert!(audio_results[0].payload_json.contains("[EMAIL]"));
}

#[test]
fn test_e2e_expert_training_export_preparation() {
    let pipeline = PrivacyPipeline::new();

    let raw_records = vec![
        (
            "sample_1".to_string(),
            "Export for dev@swal.org with key sk-112233445566778899001122 in /tmp/data".to_string(),
            vec!["rag".to_string(), "legal".to_string()],
            "contract_classification".to_string(),
            0.90,
        ),
        (
            "sample_2".to_string(),
            "Standard clean training record without PII".to_string(),
            vec!["audio".to_string()],
            "audio_transcription".to_string(),
            0.99,
        ),
    ];

    // P2 Level: Redaction without noise
    let p2_records = pipeline.process_tuple_batch(raw_records.clone(), PrivacyLevel::P2);
    assert_eq!(p2_records.len(), 2);
    assert!(p2_records[0].1.contains("[EMAIL]"));
    assert!(p2_records[0].1.contains("[API_KEY]"));
    assert!(p2_records[0].1.contains("[PATH]"));
    assert_eq!(p2_records[0].4, 0.90);

    // P3 Level: Redaction + Laplace noise on metrics
    let p3_records = pipeline.process_tuple_batch(raw_records, PrivacyLevel::P3);
    assert_eq!(p3_records.len(), 2);
    assert!(p3_records[0].1.contains("[EMAIL]"));
    // Confidence score should have differential privacy noise added
    assert!(
        p3_records[0].4 != 0.90 || p3_records[1].4 != 0.99,
        "Laplace noise should alter confidence metrics in P3"
    );
}
