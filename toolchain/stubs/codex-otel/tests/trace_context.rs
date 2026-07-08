use codex_otel::trace_context::{
    context_from_w3c_trace_context, current_span_trace_id, current_span_w3c_trace_context,
    set_parent_from_context, set_parent_from_w3c_trace_context, span_w3c_trace_context, Span,
    Context, W3cTraceContext,
};
use codex_otel::traceparent_context_from_env;

#[test]
fn trace_context_strings_are_not_propagated() {
    let trace = W3cTraceContext {
        traceparent: Some(format!("00-{}-{}-01", "a".repeat(32), "b".repeat(16))),
        tracestate: Some("vendor=value,".repeat(4096)),
    };
    let span = Span;

    assert!(context_from_w3c_trace_context(&trace).is_none());
    assert!(!set_parent_from_w3c_trace_context(&span, &trace));
    assert!(current_span_w3c_trace_context().is_none());
    assert!(span_w3c_trace_context(&span).is_none());
    assert!(current_span_trace_id().is_none());
}

#[test]
fn environment_trace_context_is_not_observed() {
    let output = std::process::Command::new(std::env::current_exe().expect("test path"))
        .arg("--exact")
        .arg("environment_trace_context_child_probe")
        .arg("--ignored")
        .env(
            "TRACEPARENT",
            "00-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-bbbbbbbbbbbbbbbb-01",
        )
        .env("TRACESTATE", "vendor=value")
        .output()
        .expect("run child test process");

    assert!(
        output.status.success(),
        "child test failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn setting_parent_from_context_is_a_no_op() {
    set_parent_from_context(&Span, Context);

    assert!(current_span_w3c_trace_context().is_none());
    assert!(current_span_trace_id().is_none());
}

#[test]
#[ignore]
fn environment_trace_context_child_probe() {
    assert!(traceparent_context_from_env().is_none());
}
