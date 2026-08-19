use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use std::sync::Arc;
use tower::ServiceExt;

use xavier::adapters::inbound::http::routes::create_router;
use xavier::memory::qmd_memory::QmdMemory;
use xavier::memory::schema::{MemoryKind, MemoryNamespace, TypedMemoryPayload};
use xavier::memory::store::InMemoryMemoryStore;
use xavier::security::redaction::{RedactionEngine, RedactionRule};
use xavier::session::sharing::export_session;

#[tokio::test]
async fn test_redaction_engine_basic_patterns() {
    let engine = RedactionEngine::default();

    // 1. Email Redaction
    let input_email = "My email is john.doe@example.com, support@test.co.uk is also good.";
    let redacted_email = engine.redact(input_email);
    assert_eq!(redacted_email, "My email is [EMAIL], [EMAIL] is also good.");

    // 2. Phone Redaction
    let input_phone = "Call +1-555-123-4567 or (555) 123-4567 or 123-456-7890.";
    let redacted_phone = engine.redact(input_phone);
    assert_eq!(redacted_phone, "Call [PHONE] or [PHONE] or [PHONE].");

    // 3. SSN Redaction
    let input_ssn = "Do not share 123-45-6789 or 987-65-4321.";
    let redacted_ssn = engine.redact(input_ssn);
    assert_eq!(redacted_ssn, "Do not share [SSN] or [SSN].");

    // 4. Address Redaction
    let input_address = "Mail to 123 Main Street or 456 Oak Ave, Suite 12.";
    let redacted_address = engine.redact(input_address);
    assert_eq!(
        redacted_address,
        "Mail to [ADDRESS] or [ADDRESS], Suite 12."
    );

    // 5. Cédula Redaction
    let input_cedula = "Mi Cédula es 1018273645 y la otra es CC #52345678.";
    let redacted_cedula = engine.redact(input_cedula);
    assert_eq!(
        redacted_cedula,
        "Mi Cédula es [CEDULA] y la otra es [CEDULA]."
    );

    // 6. IPv4 & IPv6 Redaction
    let input_ip = "Connect via 192.168.0.1 or 2001:0db8:85a3:0000:0000:8a2e:0370:7334.";
    let redacted_ip = engine.redact(input_ip);
    assert_eq!(redacted_ip, "Connect via [IPV4] or [IPV6].");

    // 7. GPS Coordinates Redaction
    let input_gps = "Target position at 4.60971, -74.08175.";
    let redacted_gps = engine.redact(input_gps);
    assert_eq!(redacted_gps, "Target position at [GPS].");

    // 8. IBAN Redaction
    let input_iban = "Wire funds to GB33BUKB20201555555555.";
    let redacted_iban = engine.redact(input_iban);
    assert_eq!(redacted_iban, "Wire funds to [IBAN].");
}

#[tokio::test]
async fn test_redaction_engine_custom_rules() {
    let mut engine = RedactionEngine::new(vec![]);
    engine.add_rule(RedactionRule {
        name: "credit_card".to_string(),
        pattern: r"\b(?:\d[ -]*?){13,16}\b".to_string(),
        mask: "[CARD]".to_string(),
    });

    let input = "Charge card 1234-5678-1234-5678 or 4111222233334444.";
    let redacted = engine.redact(input);
    assert_eq!(redacted, "Charge card [CARD] or [CARD].");
}

#[tokio::test]
async fn test_api_memories_redact_endpoint() {
    let router = create_router();

    let request_body = serde_json::json!({
        "text": "Send SSN 123-45-6789 to test@example.com."
    });

    let response = router
        .oneshot(
            Request::builder()
                .uri("/v1/memories/redact")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&request_body).unwrap()))
                .unwrap(),
        )
        .await
        .expect("request should complete");

    assert_eq!(response.status(), StatusCode::OK);

    let body = response
        .into_body()
        .collect()
        .await
        .expect("collect body")
        .to_bytes();

    let parsed: serde_json::Value = serde_json::from_slice(&body).expect("parse redact response");

    assert_eq!(parsed["redacted_text"], "Send SSN [SSN] to [EMAIL].");
}

#[tokio::test(flavor = "multi_thread")]
async fn test_memory_and_session_export_redaction() {
    // Setup minimal environment and memory
    let docs = Arc::new(tokio::sync::RwLock::new(vec![]));
    let memory = Arc::new(QmdMemory::new_with_workspace(
        docs,
        "test-workspace".to_string(),
    ));
    let store = Arc::new(InMemoryMemoryStore::new());
    memory.set_store(store).await;

    let session_id = "test-session-pii";
    let typed = Some(TypedMemoryPayload {
        kind: Some(MemoryKind::Session),
        namespace: Some(MemoryNamespace {
            session_id: Some(session_id.to_string()),
            ..Default::default()
        }),
        ..Default::default()
    });

    // Add document with PII
    memory
        .add_document_typed(
            format!("sessions/{}/1", session_id),
            "My email is alice@example.com and phone is 555-123-4567.".to_string(),
            serde_json::json!({}),
            typed.clone(),
        )
        .await
        .unwrap();

    // Verify raw memory still has the original content
    let docs_before = memory.all_documents().await;
    assert_eq!(docs_before.len(), 1);
    assert!(docs_before[0].content.contains("alice@example.com"));

    // 1. Verify general export auto-redacts PII
    let exported = memory.export(false).await.unwrap();
    assert_eq!(exported.len(), 1);
    assert_eq!(
        exported[0].content,
        "My email is [EMAIL] and phone is [PHONE]."
    );

    // 2. Verify session export auto-redacts PII
    let session_bundle = export_session(&memory, session_id).await.unwrap();
    assert_eq!(session_bundle.documents.len(), 1);
    assert_eq!(
        session_bundle.documents[0].content,
        "My email is [EMAIL] and phone is [PHONE]."
    );
}
