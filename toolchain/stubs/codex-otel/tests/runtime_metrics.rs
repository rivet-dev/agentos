use codex_otel::{RuntimeMetricTotals, RuntimeMetricsSummary};

#[test]
fn runtime_metric_totals_merge_saturates() {
    let mut totals = RuntimeMetricTotals {
        input_tokens: u64::MAX - 1,
        output_tokens: 7,
    };

    totals.merge(RuntimeMetricTotals {
        input_tokens: 10,
        output_tokens: u64::MAX,
    });

    assert_eq!(totals.input_tokens, u64::MAX);
    assert_eq!(totals.output_tokens, u64::MAX);
}

#[test]
fn runtime_metrics_summary_merge_saturates() {
    let mut summary = RuntimeMetricsSummary {
        input_tokens: 12,
        output_tokens: u64::MAX - 2,
    };

    summary.merge(RuntimeMetricsSummary {
        input_tokens: u64::MAX,
        output_tokens: 10,
    });

    assert_eq!(summary.input_tokens, u64::MAX);
    assert_eq!(summary.output_tokens, u64::MAX);
}
