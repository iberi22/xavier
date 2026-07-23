//! Unit tests for search sub-modules.

#[cfg(test)]
mod qmd_search_tests {
    use crate::memory::qmd_memory::search::extract_answer;
    use crate::memory::qmd_memory::utils::{
        extract_speaker_from_query, extract_speakers, is_likely_speaker, resolve_pronouns,
    };

    #[test]
    fn test_extract_speakers() {
        let text = "Caroline: Hello\n[James]: Hi\nSpeaker: Alice\nPerson: Robert\nGuest: Emma";
        let speakers = extract_speakers(text);
        assert!(speakers.contains(&"Caroline".to_string()));
        assert!(speakers.contains(&"James".to_string()));
        assert!(speakers.contains(&"Alice".to_string()));
        assert!(speakers.contains(&"Robert".to_string()));
        assert!(speakers.contains(&"Emma".to_string()));
    }

    #[test]
    fn test_extract_speaker_from_query() {
        assert_eq!(
            extract_speaker_from_query("Who is Caroline?"),
            Some("Caroline".to_string())
        );
        assert_eq!(
            extract_speaker_from_query("What did James say?"),
            Some("James".to_string())
        );
        assert_eq!(
            extract_speaker_from_query("When was Alice there?"),
            Some("Alice".to_string())
        );
        assert_eq!(
            extract_speaker_from_query("Where is Robert?"),
            Some("Robert".to_string())
        );
        assert_eq!(
            extract_speaker_from_query("Why did Emma laugh?"),
            Some("Emma".to_string())
        );
    }

    #[test]
    fn test_resolve_pronouns() {
        let speakers = vec!["Caroline".to_string(), "James".to_string()];

        // Single female candidate
        assert_eq!(
            resolve_pronouns("What did she say?", &speakers),
            "What did Caroline say?"
        );

        // Single male candidate
        assert_eq!(
            resolve_pronouns("What did he say?", &speakers),
            "What did James say?"
        );

        // Multiple female candidates - no resolution
        let speakers_multiple = vec!["Caroline".to_string(), "Alice".to_string()];
        assert_eq!(
            resolve_pronouns("What did she say?", &speakers_multiple),
            "What did she say?"
        );
    }

    #[test]
    fn test_is_likely_speaker() {
        assert!(is_likely_speaker("Caroline"));
        assert!(is_likely_speaker("James"));
        assert!(!is_likely_speaker("Who"));
        assert!(!is_likely_speaker("What"));
        assert!(!is_likely_speaker("She"));
        assert!(!is_likely_speaker("The"));
    }

    #[test]
    fn test_extract_answer_date() {
        let result = extract_answer("The event took place on 15 January 2023 in New York.", "2");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "15 January 2023");
    }

    #[test]
    fn test_extract_answer_opinion() {
        let result = extract_answer(
            "I think it's a wonderful idea to travel together. We should plan this.",
            "3",
        );
        assert!(result.is_some());
        assert!(result.unwrap().to_lowercase().contains("think"));
    }

    #[test]
    fn test_extract_answer_future() {
        let result = extract_answer(
            "I have decided to start a new business next month. The planning is complete.",
            "4",
        );
        assert!(result.is_some());
        assert!(result.unwrap().to_lowercase().contains("decided"));
    }

    #[test]
    fn test_adaptive_query_classification() {
        use crate::memory::qmd_memory::search::classify_query_inline;

        // Code/symbol query: should favor keywords
        let code_weights = classify_query_inline("FTS5 SQLite vec");
        assert!(
            code_weights.keyword > code_weights.semantic,
            "Expected keyword weight to be dominant for code-heavy query, got keyword: {}, semantic: {}",
            code_weights.keyword,
            code_weights.semantic
        );

        // Conceptual/semantic query: should favor vector semantic
        let conceptual_weights = classify_query_inline("arquitectura memoria agentes");
        assert!(
            conceptual_weights.semantic > conceptual_weights.keyword,
            "Expected semantic weight to be dominant for conceptual query, got keyword: {}, semantic: {}",
            conceptual_weights.keyword,
            conceptual_weights.semantic
        );

        // Blank/neutral query: should be balanced
        let balanced_weights = classify_query_inline("");
        assert_eq!(
            balanced_weights.keyword, balanced_weights.semantic,
            "Expected equal/balanced weights for empty query"
        );
    }

    #[tokio::test]
    async fn test_query_with_adaptive_search_e2e() {
        use crate::memory::qmd_memory::QmdMemory;
        use crate::memory::qmd_memory::types::MemoryDocument;
        use crate::memory::qmd_memory::search::query_with_adaptive_search;
        use tokio::sync::RwLock as AsyncRwLock;
        use std::sync::Arc;

        let memory = QmdMemory::new(Arc::new(AsyncRwLock::new(Vec::new())));

        // Add some mock documents
        memory
            .add(MemoryDocument {
                id: Some("code-doc".to_string()),
                path: "src/db/sqlite.rs".to_string(),
                content: "We use FTS5 extension in SQLite with vec_f32 for custom storage.".to_string(),
                metadata: serde_json::json!({}),
                content_vector: Some(vec![1.0, 0.0]),
                embedding: vec![1.0, 0.0],
                ..Default::default()
            })
            .await
            .unwrap();

        memory
            .add(MemoryDocument {
                id: Some("concept-doc".to_string()),
                path: "docs/architecture.md".to_string(),
                content: "La arquitectura de memoria para agentes inteligentes permite un razonamiento robusto.".to_string(),
                metadata: serde_json::json!({}),
                content_vector: Some(vec![0.0, 1.0]),
                embedding: vec![0.0, 1.0],
                ..Default::default()
            })
            .await
            .unwrap();

        // 1. Run adaptive search for technical query (FTS5 SQLite vec)
        let code_results = query_with_adaptive_search(
            &memory,
            "FTS5 SQLite vec",
            vec![1.0, 0.0],
            2,
        )
        .await
        .unwrap();

        assert!(!code_results.is_empty(), "Expected results for code search");
        assert_eq!(code_results[0].id.as_deref(), Some("code-doc"), "Should prioritize exact lexical matches in code search");

        // 2. Run adaptive search for conceptual query (arquitectura memoria agentes)
        let concept_results = query_with_adaptive_search(
            &memory,
            "arquitectura memoria agentes",
            vec![0.0, 1.0],
            2,
        )
        .await
        .unwrap();

        assert!(!concept_results.is_empty(), "Expected results for concept search");
        assert_eq!(concept_results[0].id.as_deref(), Some("concept-doc"), "Should prioritize semantic vector matches in concept search");
    }
}
