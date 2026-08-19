//! # Distributed Tracer & W3C Trace Context
//!
//! Provides W3C Trace Context (`traceparent`) parsing/formatting and a lightweight,
//! thread-safe in-memory recorder for multi-agent distributed tracing across Agent,
//! MCP tool calls, Xavier Memory, and LLM Provider invocations. Supports exporting
//! recorded spans in OpenTelemetry OTLP JSON format.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};

/// Error types for `traceparent` header parsing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TraceparentError {
    #[error("Invalid traceparent format: expected 4 hyphen-separated fields")]
    InvalidFormat,
    #[error("Unsupported version: expected '00', found '{0}'")]
    UnsupportedVersion(String),
    #[error("Invalid trace_id length: expected 32 hex chars, found {0}")]
    InvalidTraceIdLength(usize),
    #[error("Invalid trace_id: must be non-zero hex")]
    InvalidTraceId,
    #[error("Invalid parent_id length: expected 16 hex chars, found {0}")]
    InvalidParentIdLength(usize),
    #[error("Invalid parent_id: must be non-zero hex")]
    InvalidParentId,
    #[error("Invalid trace_flags length: expected 2 hex chars, found {0}")]
    InvalidTraceFlagsLength(usize),
    #[error("Invalid trace_flags hex")]
    InvalidTraceFlags,
}

/// Represents a parsed W3C `traceparent` header value according to W3C Trace Context spec.
///
/// Format: `version-trace_id-parent_id-trace_flags`
/// Example: `00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Traceparent {
    pub version: String,
    pub trace_id: String,
    pub parent_id: String,
    pub trace_flags: String,
}

impl Traceparent {
    /// Creates a new `Traceparent` with a random trace_id and parent_id.
    pub fn new() -> Self {
        Self {
            version: "00".to_string(),
            trace_id: generate_hex_id(16),
            parent_id: generate_hex_id(8),
            trace_flags: "01".to_string(),
        }
    }

    /// Creates a child `Traceparent` using current `parent_id` as context, generating a new `parent_id` (span_id).
    pub fn child(&self) -> Self {
        Self {
            version: self.version.clone(),
            trace_id: self.trace_id.clone(),
            parent_id: generate_hex_id(8),
            trace_flags: self.trace_flags.clone(),
        }
    }

    /// Parses a W3C `traceparent` header string.
    pub fn parse(header: &str) -> Result<Self, TraceparentError> {
        let parts: Vec<&str> = header.trim().split('-').collect();
        if parts.len() != 4 {
            return Err(TraceparentError::InvalidFormat);
        }

        let version = parts[0];
        if version != "00" {
            return Err(TraceparentError::UnsupportedVersion(version.to_string()));
        }

        let trace_id = parts[1];
        if trace_id.len() != 32 {
            return Err(TraceparentError::InvalidTraceIdLength(trace_id.len()));
        }
        if !is_valid_hex(trace_id) || trace_id.chars().all(|c| c == '0') {
            return Err(TraceparentError::InvalidTraceId);
        }

        let parent_id = parts[2];
        if parent_id.len() != 16 {
            return Err(TraceparentError::InvalidParentIdLength(parent_id.len()));
        }
        if !is_valid_hex(parent_id) || parent_id.chars().all(|c| c == '0') {
            return Err(TraceparentError::InvalidParentId);
        }

        let trace_flags = parts[3];
        if trace_flags.len() != 2 {
            return Err(TraceparentError::InvalidTraceFlagsLength(trace_flags.len()));
        }
        if !is_valid_hex(trace_flags) {
            return Err(TraceparentError::InvalidTraceFlags);
        }

        Ok(Self {
            version: version.to_string(),
            trace_id: trace_id.to_string(),
            parent_id: parent_id.to_string(),
            trace_flags: trace_flags.to_string(),
        })
    }

    /// Formats the `Traceparent` as a W3C traceparent header string.
    pub fn format(&self) -> String {
        format!(
            "{}-{}-{}-{}",
            self.version, self.trace_id, self.parent_id, self.trace_flags
        )
    }
}

impl Default for Traceparent {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for Traceparent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format())
    }
}

/// Helper function to check if a string is valid lower-case or upper-case hex.
fn is_valid_hex(s: &str) -> bool {
    s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Helper function to generate hex random IDs.
fn generate_hex_id(num_bytes: usize) -> String {
    use rand::RngCore;
    let mut bytes = vec![0u8; num_bytes];
    rand::thread_rng().fill_bytes(&mut bytes);
    // Ensure non-zero
    if bytes.iter().all(|&b| b == 0) {
        bytes[0] = 1;
    }
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Span category kind in multi-agent tracing.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanKind {
    Agent,
    Mcp,
    Memory,
    Llm,
    #[default]
    Internal,
    Client,
    Server,
}

/// Span execution status.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanStatus {
    #[default]
    Unset,
    Ok,
    Error(String),
}

/// Represents an individual recorded span in the tracing pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    pub name: String,
    pub kind: SpanKind,
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub start_time_unix_nano: u64,
    pub end_time_unix_nano: Option<u64>,
    pub duration_ms: Option<u64>,
    pub attributes: HashMap<String, String>,
    pub status: SpanStatus,
}

/// OpenTelemetry OTLP JSON export structure formats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtelExportPayload {
    #[serde(rename = "resourceSpans")]
    pub resource_spans: Vec<OtelResourceSpans>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtelResourceSpans {
    pub resource: OtelResource,
    #[serde(rename = "scopeSpans")]
    pub scope_spans: Vec<OtelScopeSpans>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtelResource {
    pub attributes: Vec<OtelKeyValue>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtelScopeSpans {
    pub scope: OtelScope,
    pub spans: Vec<OtelSpan>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtelScope {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtelSpan {
    #[serde(rename = "traceId")]
    pub trace_id: String,
    #[serde(rename = "spanId")]
    pub span_id: String,
    #[serde(rename = "parentSpanId", skip_serializing_if = "Option::is_none")]
    pub parent_span_id: Option<String>,
    pub name: String,
    pub kind: u32,
    #[serde(rename = "startTimeUnixNano")]
    pub start_time_unix_nano: String,
    #[serde(rename = "endTimeUnixNano")]
    pub end_time_unix_nano: String,
    pub attributes: Vec<OtelKeyValue>,
    pub status: OtelStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtelKeyValue {
    pub key: String,
    pub value: OtelAnyValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtelAnyValue {
    #[serde(rename = "stringValue")]
    pub string_value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtelStatus {
    pub code: u32, // 0 = Unset, 1 = Ok, 2 = Error
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// RAII Guard for managing span lifetime and auto-recording on drop or explicit finish.
pub struct SpanGuard {
    tracer: DistributedTracer,
    span: Option<Span>,
    start_system_time: SystemTime,
}

impl SpanGuard {
    fn new(tracer: DistributedTracer, span: Span) -> Self {
        Self {
            tracer,
            span: Some(span),
            start_system_time: SystemTime::now(),
        }
    }

    /// Set an attribute on the active span.
    pub fn set_attribute(&mut self, key: impl Into<String>, value: impl Into<String>) {
        if let Some(ref mut span) = self.span {
            span.attributes.insert(key.into(), value.into());
        }
    }

    /// Set status for the active span.
    pub fn set_status(&mut self, status: SpanStatus) {
        if let Some(ref mut span) = self.span {
            span.status = status;
        }
    }

    /// Access active span's traceparent header formatted string.
    pub fn traceparent(&self) -> String {
        if let Some(ref span) = self.span {
            format!("00-{}-{}-01", span.trace_id, span.span_id)
        } else {
            String::new()
        }
    }

    /// Access active span's trace_id.
    pub fn trace_id(&self) -> &str {
        self.span.as_ref().map(|s| s.trace_id.as_str()).unwrap_or("")
    }

    /// Access active span's span_id.
    pub fn span_id(&self) -> &str {
        self.span.as_ref().map(|s| s.span_id.as_str()).unwrap_or("")
    }

    /// Explicitly finish and record the span.
    pub fn finish(mut self) {
        self.record_span();
    }

    fn record_span(&mut self) {
        if let Some(mut span) = self.span.take() {
            let now = SystemTime::now();
            let end_nano = now
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_nanos() as u64;
            let elapsed_ms = now
                .duration_since(self.start_system_time)
                .unwrap_or(Duration::ZERO)
                .as_millis() as u64;

            span.end_time_unix_nano = Some(end_nano);
            span.duration_ms = Some(elapsed_ms);
            self.tracer.record_span(span);
        }
    }
}

impl Drop for SpanGuard {
    fn drop(&mut self) {
        self.record_span();
    }
}

/// Thread-safe in-memory recorder for distributed tracing.
#[derive(Debug, Clone)]
pub struct DistributedTracer {
    spans: Arc<RwLock<Vec<Span>>>,
}

impl DistributedTracer {
    /// Creates a new `DistributedTracer`.
    pub fn new() -> Self {
        Self {
            spans: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Start a new root or incoming span from optional traceparent string.
    pub fn start_span(&self, name: impl Into<String>, traceparent_str: Option<&str>) -> SpanGuard {
        let name = name.into();
        let parsed = traceparent_str.and_then(|tp| Traceparent::parse(tp).ok());

        let (trace_id, parent_span_id, span_id) = match parsed {
            Some(tp) => (tp.trace_id, Some(tp.parent_id), generate_hex_id(8)),
            None => (generate_hex_id(16), None, generate_hex_id(8)),
        };

        let start_time_unix_nano = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_nanos() as u64;

        let span = Span {
            name,
            kind: SpanKind::Internal,
            trace_id,
            span_id,
            parent_span_id,
            start_time_unix_nano,
            end_time_unix_nano: None,
            duration_ms: None,
            attributes: HashMap::new(),
            status: SpanStatus::Unset,
        };

        SpanGuard::new(self.clone(), span)
    }

    /// Start a child span under a parent guard/context.
    pub fn start_child_span(&self, name: impl Into<String>, parent_guard: &SpanGuard, kind: SpanKind) -> SpanGuard {
        let name = name.into();
        let start_time_unix_nano = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_nanos() as u64;

        let span = Span {
            name,
            kind,
            trace_id: parent_guard.trace_id().to_string(),
            span_id: generate_hex_id(8),
            parent_span_id: Some(parent_guard.span_id().to_string()),
            start_time_unix_nano,
            end_time_unix_nano: None,
            duration_ms: None,
            attributes: HashMap::new(),
            status: SpanStatus::Unset,
        };

        SpanGuard::new(self.clone(), span)
    }

    /// Internal method to store recorded span.
    fn record_span(&self, span: Span) {
        if let Ok(mut guard) = self.spans.write() {
            guard.push(span);
        }
    }

    /// Returns a copy of all recorded spans.
    pub fn get_spans(&self) -> Vec<Span> {
        self.spans.read().map(|g| g.clone()).unwrap_or_default()
    }

    /// Clears all recorded spans in memory.
    pub fn clear(&self) {
        if let Ok(mut guard) = self.spans.write() {
            guard.clear();
        }
    }

    /// Export recorded spans in standard OpenTelemetry OTLP JSON format.
    pub fn export_otel_json(&self) -> String {
        let spans = self.get_spans();

        let otel_spans: Vec<OtelSpan> = spans
            .into_iter()
            .map(|s| {
                let kind_val = match s.kind {
                    SpanKind::Internal => 1,
                    SpanKind::Server => 2,
                    SpanKind::Client => 3,
                    SpanKind::Agent => 4,
                    SpanKind::Mcp => 5,
                    SpanKind::Memory => 6,
                    SpanKind::Llm => 7,
                };

                let mut attrs: Vec<OtelKeyValue> = s
                    .attributes
                    .into_iter()
                    .map(|(k, v)| OtelKeyValue {
                        key: k,
                        value: OtelAnyValue { string_value: v },
                    })
                    .collect();

                if let Some(dur) = s.duration_ms {
                    attrs.push(OtelKeyValue {
                        key: "duration_ms".to_string(),
                        value: OtelAnyValue {
                            string_value: dur.to_string(),
                        },
                    });
                }

                let (status_code, status_msg) = match s.status {
                    SpanStatus::Unset => (0, None),
                    SpanStatus::Ok => (1, None),
                    SpanStatus::Error(msg) => (2, Some(msg)),
                };

                OtelSpan {
                    trace_id: s.trace_id,
                    span_id: s.span_id,
                    parent_span_id: s.parent_span_id,
                    name: s.name,
                    kind: kind_val,
                    start_time_unix_nano: s.start_time_unix_nano.to_string(),
                    end_time_unix_nano: s.end_time_unix_nano.unwrap_or(s.start_time_unix_nano).to_string(),
                    attributes: attrs,
                    status: OtelStatus {
                        code: status_code,
                        message: status_msg,
                    },
                }
            })
            .collect();

        let payload = OtelExportPayload {
            resource_spans: vec![OtelResourceSpans {
                resource: OtelResource {
                    attributes: vec![OtelKeyValue {
                        key: "service.name".to_string(),
                        value: OtelAnyValue {
                            string_value: "xavier-agent-runtime".to_string(),
                        },
                    }],
                },
                scope_spans: vec![OtelScopeSpans {
                    scope: OtelScope {
                        name: "xavier.observability.distributed_tracer".to_string(),
                        version: env!("CARGO_PKG_VERSION").to_string(),
                    },
                    spans: otel_spans,
                }],
            }],
        };

        serde_json::to_string_pretty(&payload).unwrap_or_else(|_| "{}".to_string())
    }
}

impl Default for DistributedTracer {
    fn default() -> Self {
        Self::new()
    }
}
