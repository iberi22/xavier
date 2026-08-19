use xavier::observability::distributed_tracer::{
    DistributedTracer, OtelExportPayload, SpanKind, SpanStatus, Traceparent, TraceparentError,
};
use std::thread;
use std::time::Duration;

#[test]
fn test_traceparent_parse_and_format_valid() {
    let header = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
    let parsed = Traceparent::parse(header).expect("valid traceparent parsing failed");

    assert_eq!(parsed.version, "00");
    assert_eq!(parsed.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
    assert_eq!(parsed.parent_id, "00f067aa0ba902b7");
    assert_eq!(parsed.trace_flags, "01");

    assert_eq!(parsed.format(), header);
    assert_eq!(parsed.to_string(), header);
}

#[test]
fn test_traceparent_parse_invalid_formats() {
    // Missing parts
    assert_eq!(
        Traceparent::parse("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7"),
        Err(TraceparentError::InvalidFormat)
    );

    // Unsupported version
    assert_eq!(
        Traceparent::parse("01-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"),
        Err(TraceparentError::UnsupportedVersion("01".to_string()))
    );

    // Invalid trace_id length
    assert_eq!(
        Traceparent::parse("00-4bf92f3577b34da6a3ce929d0e0e473-00f067aa0ba902b7-01"),
        Err(TraceparentError::InvalidTraceIdLength(31))
    );

    // Zero trace_id
    assert_eq!(
        Traceparent::parse("00-00000000000000000000000000000000-00f067aa0ba902b7-01"),
        Err(TraceparentError::InvalidTraceId)
    );

    // Invalid parent_id length
    assert_eq!(
        Traceparent::parse("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b-01"),
        Err(TraceparentError::InvalidParentIdLength(15))
    );

    // Zero parent_id
    assert_eq!(
        Traceparent::parse("00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01"),
        Err(TraceparentError::InvalidParentId)
    );

    // Invalid trace_flags length
    assert_eq!(
        Traceparent::parse("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-001"),
        Err(TraceparentError::InvalidTraceFlagsLength(3))
    );
}

#[test]
fn test_traceparent_child_derivation() {
    let parent = Traceparent::new();
    let child = parent.child();

    assert_eq!(child.version, parent.version);
    assert_eq!(child.trace_id, parent.trace_id);
    assert_ne!(child.parent_id, parent.parent_id);
    assert_eq!(child.trace_flags, parent.trace_flags);
}

#[test]
fn test_parent_child_span_linking_and_recording() {
    let tracer = DistributedTracer::new();
    assert!(tracer.get_spans().is_empty());

    let incoming_tp = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

    {
        let mut root_guard = tracer.start_span("UserPromptRequest", Some(incoming_tp));
        root_guard.set_attribute("user.id", "usr_1234");
        root_guard.set_status(SpanStatus::Ok);

        assert_eq!(root_guard.trace_id(), "4bf92f3577b34da6a3ce929d0e0e4736");

        {
            let mut agent_guard =
                tracer.start_child_span("AgentExecution", &root_guard, SpanKind::Agent);
            agent_guard.set_attribute("agent.role", "orchestrator");
            thread::sleep(Duration::from_millis(15));

            {
                let mut mcp_guard =
                    tracer.start_child_span("McpToolCall", &agent_guard, SpanKind::Mcp);
                mcp_guard.set_attribute("tool.name", "sqlite_query");
                thread::sleep(Duration::from_millis(10));
                mcp_guard.set_status(SpanStatus::Ok);
                mcp_guard.finish();
            }

            {
                let mut memory_guard =
                    tracer.start_child_span("XavierMemorySearch", &agent_guard, SpanKind::Memory);
                memory_guard.set_attribute("query.namespace", "jules");
                thread::sleep(Duration::from_millis(5));
                memory_guard.set_status(SpanStatus::Ok);
                // Dropped automatically
            }

            {
                let mut llm_guard =
                    tracer.start_child_span("LlmProviderInvoke", &agent_guard, SpanKind::Llm);
                llm_guard.set_attribute("provider", "ollama");
                llm_guard.set_attribute("model", "qwen3-4b");
                llm_guard.set_status(SpanStatus::Error("Timeout waiting for LLM".to_string()));
            }

            agent_guard.set_status(SpanStatus::Ok);
        }

        root_guard.finish();
    }

    let spans = tracer.get_spans();
    assert_eq!(spans.len(), 5);

    // Verify root span
    let root = spans.iter().find(|s| s.name == "UserPromptRequest").expect("root span found");
    assert_eq!(root.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
    assert_eq!(root.parent_span_id, Some("00f067aa0ba902b7".to_string()));
    assert_eq!(root.attributes.get("user.id"), Some(&"usr_1234".to_string()));
    assert_eq!(root.status, SpanStatus::Ok);

    // Verify hierarchy
    let agent = spans.iter().find(|s| s.name == "AgentExecution").expect("agent span found");
    assert_eq!(agent.trace_id, root.trace_id);
    assert_eq!(agent.parent_span_id, Some(root.span_id.clone()));

    let mcp = spans.iter().find(|s| s.name == "McpToolCall").expect("mcp span found");
    assert_eq!(mcp.trace_id, root.trace_id);
    assert_eq!(mcp.parent_span_id, Some(agent.span_id.clone()));
    assert!(mcp.duration_ms.unwrap_or(0) >= 10);

    let llm = spans.iter().find(|s| s.name == "LlmProviderInvoke").expect("llm span found");
    assert_eq!(llm.trace_id, root.trace_id);
    assert_eq!(llm.parent_span_id, Some(agent.span_id.clone()));
    assert_eq!(
        llm.status,
        SpanStatus::Error("Timeout waiting for LLM".to_string())
    );
}

#[test]
fn test_otel_json_export_structure() {
    let tracer = DistributedTracer::new();
    let mut guard = tracer.start_span("AgentPipeline", None);
    guard.set_attribute("component", "agent");
    guard.set_status(SpanStatus::Ok);
    guard.finish();

    let json_str = tracer.export_otel_json();
    assert!(!json_str.is_empty());

    let export: OtelExportPayload =
        serde_json::from_str(&json_str).expect("Valid OpenTelemetry OTLP JSON serialization");
    assert_eq!(export.resource_spans.len(), 1);

    let scope_spans = &export.resource_spans[0].scope_spans;
    assert_eq!(scope_spans.len(), 1);

    let otel_span = &scope_spans[0].spans[0];
    assert_eq!(otel_span.name, "AgentPipeline");
    assert_eq!(otel_span.status.code, 1); // 1 = Ok
}
