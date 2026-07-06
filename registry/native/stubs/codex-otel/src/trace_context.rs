//! Trace context helpers (stub — all functions return None or no-op).
//!
//! The real implementation uses opentelemetry Context/Span types.
//! This stub defines local placeholder types since opentelemetry
//! cannot compile for WASI.


/// W3C trace context (stub — matches codex-protocol W3cTraceContext shape).
#[derive(Debug, Clone, Default)]
pub struct W3cTraceContext {
    /// The traceparent header value.
    pub traceparent: Option<String>,
    /// The tracestate header value.
    pub tracestate: Option<String>,
}

/// Opaque trace context (stub for opentelemetry::Context).
#[derive(Debug, Clone)]
pub struct Context;

/// Opaque span (stub for tracing::Span).
#[derive(Debug)]
pub struct Span;

/// Get the W3C trace context for the current span (stub — always None).
pub fn current_span_w3c_trace_context() -> Option<W3cTraceContext> {
    None
}

/// Get the W3C trace context for a given span (stub — always None).
pub fn span_w3c_trace_context(_span: &Span) -> Option<W3cTraceContext> {
    None
}

/// Get the trace ID for the current span (stub — always None).
pub fn current_span_trace_id() -> Option<String> {
    None
}

/// Parse a W3C trace context into an opentelemetry Context (stub — always None).
pub fn context_from_w3c_trace_context(_trace: &W3cTraceContext) -> Option<Context> {
    None
}

/// Set a span's parent from a W3C trace context (stub — always false).
pub fn set_parent_from_w3c_trace_context(_span: &Span, _trace: &W3cTraceContext) -> bool {
    false
}

/// Set a span's parent from a Context (stub — no-op).
pub fn set_parent_from_context(_span: &Span, _context: Context) {}

/// Get a trace context from environment variables (stub — always None).
pub fn traceparent_context_from_env() -> Option<Context> {
    None
}
