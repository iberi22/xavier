//! Verification test suite for XTSP Token Savings & Recall Evaluation Benchmark Runner

use xavier::cli::commands::benchmark_runner::{
    default_xtsp_test_cases, run_xtsp_benchmark, XtspDocument, XtspTestCase,
};

#[test]
fn test_xtsp_default_benchmark_runner() {
    let result = run_xtsp_benchmark(None, 10);

    assert!(result.total_queries > 0, "Should evaluate at least 1 query");
    assert!(
        result.total_documents > 0,
        "Should evaluate at least 1 document"
    );
    assert!(
        result.full_mode_bytes > result.snippet_mode_bytes,
        "Full mode payload bytes ({}) should be strictly larger than snippet mode ({})",
        result.full_mode_bytes,
        result.snippet_mode_bytes
    );
    assert!(
        result.snippet_mode_bytes > result.ids_mode_bytes,
        "Snippet mode payload bytes ({}) should be strictly larger than IDs mode ({})",
        result.snippet_mode_bytes,
        result.ids_mode_bytes
    );

    assert!(
        result.token_savings_pct > 0.0,
        "Token savings percentage should be positive, got {}",
        result.token_savings_pct
    );
    assert!(
        result.snippet_compression_ratio < 1.0,
        "Snippet compression ratio should be < 1.0, got {}",
        result.snippet_compression_ratio
    );
    assert!(
        result.ids_compression_ratio < result.snippet_compression_ratio,
        "IDs compression ratio ({}) should be smaller than snippet compression ratio ({})",
        result.ids_compression_ratio,
        result.snippet_compression_ratio
    );

    assert!(
        result.recall_at_k > 0.0,
        "Recall@k should be > 0.0, got {}",
        result.recall_at_k
    );
    assert!(
        result.precision_at_k > 0.0,
        "Precision@k should be > 0.0, got {}",
        result.precision_at_k
    );
}

#[test]
fn test_xtsp_custom_dataset_benchmark() {
    let custom_cases = vec![
        XtspTestCase {
            query: "distributed consensus raft".to_string(),
            ground_truth_ids: vec!["doc_raft_1".to_string()],
            documents: vec![
                XtspDocument {
                    id: "doc_raft_1".to_string(),
                    title: "Raft Consensus Algorithm Spec".to_string(),
                    content: "Raft is a consensus algorithm designed to be easy to understand. It decomposes consensus into leader election, log replication, and safety. In a Raft cluster, servers adopt one of three states: leader, follower, or candidate. The leader manages log entries and coordinates state machine replication across all peers in the cluster.".to_string(),
                },
                XtspDocument {
                    id: "doc_kv_2".to_string(),
                    title: "Key-Value Store Cache".to_string(),
                    content: "An in-memory LRU cache storing session tokens and ephemeral key-value mappings. Features thread-safe locking mechanisms and sliding window TTL eviction.".to_string(),
                },
            ],
        },
        XtspTestCase {
            query: "zero knowledge proof snark".to_string(),
            ground_truth_ids: vec!["doc_zk_1".to_string()],
            documents: vec![
                XtspDocument {
                    id: "doc_zk_1".to_string(),
                    title: "zk-SNARK Verification".to_string(),
                    content: "Zero-Knowledge Succinct Non-Interactive Argument of Knowledge allows a prover to demonstrate knowledge without revealing secret inputs. It utilizes elliptic curve cryptography and polynomial commitment schemes to guarantee non-malleable cryptographic proof verification in sub-linear time.".to_string(),
                },
            ],
        },
    ];

    let result = run_xtsp_benchmark(Some(&custom_cases), 1);

    assert_eq!(result.total_queries, 2);
    assert_eq!(result.total_documents, 3);
    assert_eq!(result.recall_at_k, 1.0);
    assert_eq!(result.precision_at_k, 1.0);
    assert!(result.token_savings_pct > 20.0);

    let json_report = result.to_json().expect("JSON serialization must succeed");
    assert!(json_report.contains("\"token_savings_pct\":"));
    assert!(json_report.contains("\"recall_at_k\":"));
}

#[test]
fn test_xtsp_default_cases_integrity() {
    let cases = default_xtsp_test_cases();
    assert!(!cases.is_empty(), "Default test cases should not be empty");
    for case in &cases {
        assert!(!case.query.is_empty());
        assert!(!case.ground_truth_ids.is_empty());
        assert!(!case.documents.is_empty());
    }
}
