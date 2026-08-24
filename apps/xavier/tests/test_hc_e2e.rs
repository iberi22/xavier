//! End-to-End Cognitive Challenge Generation & Semantic Verification Test (`test_hc_e2e`)
//!
//! Tests the full cognitive alignment loop:
//! Session generation → SessionScanner (5 Challenge Types) → HumanChallengeStore persistence
//! → Response submission with embedding cosine similarity evaluation → X2 Farming summary update.

use chrono::Utc;
use tempfile::TempDir;
use xavier::humanchallenge::{
    ChallengeStatus, ChallengeType, HumanChallengeCron, HumanChallengeCronConfig,
    HumanChallengeStore, SessionScanner,
};
use xavier::memory::qmd::utils::cosine_similarity;
use xavier::session::types::{SessionEvent, SessionEventType};

/// Helper to generate synthetic session history containing all 5 canonical challenge types:
/// Contradiction, Decision, Execution, Assumption, Clarification.
fn generate_synthetic_session_events(session_id: &str) -> Vec<SessionEvent> {
    vec![
        // 1. Contradiction
        SessionEvent {
            session_id: session_id.to_string(),
            event_type: SessionEventType::Message,
            timestamp: Utc::now(),
            content: Some("Sin embargo, esta afirmación contradice el requisito previamente expresado.".to_string()),
            metadata: None,
        },
        // 2. Decision
        SessionEvent {
            session_id: session_id.to_string(),
            event_type: SessionEventType::Message,
            timestamp: Utc::now(),
            content: Some("Acordamos que la arquitectura elegida utilizará SQLite para el almacenamiento local.".to_string()),
            metadata: None,
        },
        // 3. Execution
        SessionEvent {
            session_id: session_id.to_string(),
            event_type: SessionEventType::ToolCall,
            timestamp: Utc::now(),
            content: Some("sudo systemctl restart xavier-service".to_string()),
            metadata: None,
        },
        // 4. Assumption
        SessionEvent {
            session_id: session_id.to_string(),
            event_type: SessionEventType::Message,
            timestamp: Utc::now(),
            content: Some("Asumiendo que la red P2P se encuentra accesible sin latencia.".to_string()),
            metadata: None,
        },
        // 5. Clarification
        SessionEvent {
            session_id: session_id.to_string(),
            event_type: SessionEventType::Message,
            timestamp: Utc::now(),
            content: Some("Por favor aclara los parámetros de configuración del rate limiter.".to_string()),
            metadata: None,
        },
    ]
}

/// Helper function producing a simple toy vector embedding for testing semantic similarity grading.
fn toy_embed(text: &str) -> Vec<f32> {
    let lower = text.to_lowercase();
    let mut vec = vec![0.0f32; 8];
    if lower.contains("sqlite") || lower.contains("bd") || lower.contains("db") || lower.contains("base de datos") || lower.contains("arquitectura") {
        vec[0] += 0.9;
        vec[1] += 0.4;
    }
    if lower.contains("acordamos") || lower.contains("decidimos") || lower.contains("confirmado") || lower.contains("elección") {
        vec[0] += 0.3;
        vec[2] += 0.8;
    }
    if lower.contains("banana") || lower.contains("fruta") || lower.contains("cielo") || lower.contains("amarillo") {
        vec[5] += 0.9;
        vec[6] += 0.9;
    }

    // Normalize vector
    let norm: f32 = vec.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for val in &mut vec {
            *val /= norm;
        }
    }
    vec
}

#[tokio::test]
async fn test_hc_e2e_full_cognitive_alignment_loop() {
    let temp_dir = TempDir::new().expect("create temp dir");
    let db_path = temp_dir.path().join("humanchallenge.db");

    let store = HumanChallengeStore::new(&db_path).expect("init store");
    let config = HumanChallengeCronConfig {
        db_path: db_path.clone(),
        scan_interval: std::time::Duration::from_secs(60),
        enabled: true,
    };
    let cron = HumanChallengeCron::with_store(config, store);

    let session_id = "test_hc_e2e_session_001";
    let events = generate_synthetic_session_events(session_id);

    // 1. Scan session events and extract all 5 challenge types
    let scanner = SessionScanner::new();
    let candidates = scanner.scan_session_events(&events);
    assert_eq!(candidates.len(), 5, "SessionScanner must harvest all 5 canonical challenge types");

    let found_types: std::collections::HashSet<ChallengeType> = candidates.iter().map(|c| c.challenge_type).collect();
    assert!(found_types.contains(&ChallengeType::Contradiction));
    assert!(found_types.contains(&ChallengeType::Decision));
    assert!(found_types.contains(&ChallengeType::Execution));
    assert!(found_types.contains(&ChallengeType::Assumption));
    assert!(found_types.contains(&ChallengeType::Clarification));

    // 2. Process events into cron and save to SQLite store
    let processed_count = cron.process_events(&events).expect("process events in cron");
    assert_eq!(processed_count, 5);

    // Retrieve decision challenge from store
    let store_ref = HumanChallengeStore::new(&db_path).expect("reopen store");
    let stored_events = store_ref.list_events(Some(ChallengeStatus::Candidate), 10).expect("list candidates");
    assert_eq!(stored_events.len(), 5);

    let decision_challenge = stored_events
        .iter()
        .find(|e| e.challenge_type == ChallengeType::Decision)
        .expect("must contain decision challenge");

    assert_eq!(decision_challenge.status, ChallengeStatus::Candidate);
    assert!(decision_challenge.privacy_p4_local_only);

    // 3. Semantic Verification of Answers (Semantically Similar vs Orthogonal)
    let raw_content_emb = toy_embed(&decision_challenge.raw_content);

    let semantically_similar_response = "Confirmado, acordamos la elección de SQLite como base de datos local.";
    let orthogonal_response = "El cielo es amarillo y me gustan las bananas.";

    let similar_emb = toy_embed(semantically_similar_response);
    let orthogonal_emb = toy_embed(orthogonal_response);

    let sim_high = cosine_similarity(&raw_content_emb, &similar_emb);
    let sim_low = cosine_similarity(&raw_content_emb, &orthogonal_emb);

    assert!(sim_high > 0.6, "Semantically similar response must have high cosine similarity: {}", sim_high);
    assert!(sim_low < 0.2, "Orthogonal response must have low cosine similarity: {}", sim_low);

    // 4. Submit Answer and Award X2 Farming Points for Semantically Valid Response
    let wallet_id = "0x_test_reputation_wallet_123";
    let base_points = 10u32;
    let awarded = cron
        .answer_and_award(&decision_challenge.id, semantically_similar_response, base_points, wallet_id)
        .expect("answer and award challenge");

    assert!(awarded, "Challenge answer update must return true");

    // 5. Assert SQLite Store Updates & Monthly Farming Summary
    let updated_event = store_ref
        .get_event_by_id(&decision_challenge.id)
        .expect("get updated event")
        .expect("event exists");

    assert_eq!(updated_event.status, ChallengeStatus::Answered);
    assert_eq!(updated_event.response.as_deref(), Some(semantically_similar_response));
    assert!(updated_event.points_awarded >= base_points);

    let current_month = Utc::now().format("%Y-%m").to_string();
    let summary = cron.get_farming_summary(&current_month).expect("get farming summary");
    assert_eq!(summary.answered_count, 1);
    assert!(summary.total_points >= base_points);

    // Verify Privacy P4 compliant Mesh score payload generation
    let mesh_scores = cron.prepare_mesh_scores(&current_month).expect("prepare mesh scores");
    assert_eq!(mesh_scores.len(), 1);
    assert_eq!(mesh_scores[0].challenge_type, ChallengeType::Decision);
    assert!(mesh_scores[0].points >= base_points);
}
