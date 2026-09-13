//! Unit tests for IntentQueryRouter and query routing decisions.

use crate::context::query_router::{route_query, DataTypeKind, IntentQueryRouter};

#[test]
fn test_legal_query_spanish() {
    let query = "Qué dice la cláusula penal del contrato de arrendamiento?";
    let decision = route_query(query);

    assert_eq!(decision.primary_modality, DataTypeKind::Legal);
    assert!(
        decision.confidence >= 0.60,
        "Expected confidence >= 0.60, got {}",
        decision.confidence
    );
    assert!(
        decision.confidence <= 1.0,
        "Expected confidence <= 1.0, got {}",
        decision.confidence
    );
}

#[test]
fn test_legal_query_english() {
    let router = IntentQueryRouter::new();
    let query = "What are the breach of contract and liability terms in the lease agreement?";
    let decision = router.route_query(query);

    assert_eq!(decision.primary_modality, DataTypeKind::Legal);
    assert!(decision.confidence >= 0.60);
}

#[test]
fn test_code_query_with_syntax_and_filter() {
    let query = "pub fn route_query(query: &str) -> QueryRoutingDecision ext:rs lang:rust";
    let decision = route_query(query);

    assert_eq!(decision.primary_modality, DataTypeKind::Code);
    assert!(decision.confidence >= 0.70);
    assert_eq!(
        decision.extracted_filters.get("ext").map(|s| s.as_str()),
        Some("rs")
    );
    assert_eq!(
        decision.extracted_filters.get("lang").map(|s| s.as_str()),
        Some("rust")
    );
}

#[test]
fn test_image_ocr_query() {
    let query = "Buscar captura de pantalla del diagrama de arquitectura con OCR text";
    let decision = route_query(query);

    assert_eq!(decision.primary_modality, DataTypeKind::Image);
    assert!(decision.confidence >= 0.60);
}

#[test]
fn test_audio_transcription_query() {
    let query = "Encontrar la nota de voz o transcripción de audio de la llamada ext:wav";
    let decision = route_query(query);

    assert_eq!(decision.primary_modality, DataTypeKind::Audio);
    assert!(decision.confidence >= 0.60);
    assert_eq!(
        decision.extracted_filters.get("ext").map(|s| s.as_str()),
        Some("wav")
    );
}

#[test]
fn test_ambiguous_query_fallback() {
    let query = "hello world general question";
    let decision = route_query(query);

    assert_eq!(decision.primary_modality, DataTypeKind::Text);
    assert!(
        decision.confidence < 0.60,
        "Expected low confidence for ambiguous query, got {}",
        decision.confidence
    );
    // Should fallback to secondary modalities including cross-domain kinds
    assert!(
        !decision.secondary_modalities.is_empty(),
        "Secondary modalities should not be empty on fallback"
    );
    assert!(
        decision.secondary_modalities.contains(&DataTypeKind::Legal)
            || decision.secondary_modalities.contains(&DataTypeKind::Code)
    );
}
