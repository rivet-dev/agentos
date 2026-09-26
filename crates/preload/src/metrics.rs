//! Process-level package metrics exported through RivetKit's /metrics route.
//! Every label value is fixed here; URLs and package identities are never
//! metric labels.

use std::future::Future;
use std::sync::{LazyLock, Mutex};

use agentos_client::ProcessPackageCacheStats;
use anyhow::{anyhow, Result};
use rivet_metrics::prometheus::{
    register_int_counter_vec_with_registry, register_int_counter_with_registry,
    register_int_gauge_vec_with_registry, IntCounter, IntCounterVec, IntGaugeVec, Registry,
};

use crate::ProcessPreloadReport;

struct PackageMetrics {
    cache_gauges: IntGaugeVec,
    cache_totals: IntCounterVec,
    preload_artifacts: IntCounterVec,
    preload_warmed_bytes: IntCounter,
    preload_deadlines: IntCounter,
    preload_coordinator_unavailable: IntCounter,
    last_cache: Mutex<Option<ProcessPackageCacheStats>>,
    sample: tokio::sync::Mutex<()>,
}

static PACKAGE_METRICS: LazyLock<std::result::Result<PackageMetrics, String>> =
    LazyLock::new(|| {
        PackageMetrics::new(&rivet_metrics::REGISTRY).map_err(|error| error.to_string())
    });

impl PackageMetrics {
    fn new(registry: &Registry) -> Result<Self> {
        let cache_gauges = register_int_gauge_vec_with_registry!(
            "agentos_package_cache_current",
            "Current process package-cache size and in-flight work.",
            &["measure"],
            registry
        )?;
        let cache_totals = register_int_counter_vec_with_registry!(
            "agentos_package_cache_operations_total",
            "Cumulative package-cache operations in this actor worker process.",
            &["operation"],
            registry
        )?;
        let preload_artifacts = register_int_counter_vec_with_registry!(
            "agentos_preload_artifacts_total",
            "Cumulative advisory preload artifact outcomes in this actor worker process.",
            &["outcome"],
            registry
        )?;
        let preload_warmed_bytes = register_int_counter_with_registry!(
            "agentos_preload_warmed_bytes_total",
            "Bytes warmed by advisory preloading in this actor worker process.",
            registry
        )?;
        let preload_deadlines = register_int_counter_with_registry!(
            "agentos_preload_deadlines_total",
            "Advisory preload runs that reached their startup deadline.",
            registry
        )?;
        let preload_coordinator_unavailable = register_int_counter_with_registry!(
            "agentos_preload_coordinator_unavailable_total",
            "Advisory preload runs without a usable coordinator plan.",
            registry
        )?;
        Ok(PackageMetrics {
            cache_gauges,
            cache_totals,
            preload_artifacts,
            preload_warmed_bytes,
            preload_deadlines,
            preload_coordinator_unavailable,
            last_cache: Mutex::new(None),
            sample: tokio::sync::Mutex::new(()),
        })
    }

    async fn sample_cache<F, Fut>(&self, sample: F) -> Result<()>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<ProcessPackageCacheStats>>,
    {
        // Snapshot and publication must share one ordering. Otherwise a slower
        // old snapshot can lower last_cache and double-count the next delta.
        let _sample = self.sample.lock().await;
        self.record_cache_stats(sample().await?)
    }

    fn record_cache_stats(&self, stats: ProcessPackageCacheStats) -> Result<()> {
        let metrics = self;
        let mut last = self
            .last_cache
            .lock()
            .map_err(|_| anyhow!("agentOS package metrics lock was poisoned"))?;
        for (measure, value) in [
            ("entries", stats.entries as u64),
            ("source_entries", stats.source_entries as u64),
            ("bytes", stats.bytes),
            ("pinned_entries", stats.pinned_entries as u64),
            ("pending_acquisitions", stats.pending_acquisitions as u64),
        ] {
            metrics
                .cache_gauges
                .get_metric_with_label_values(&[measure])?
                .set(i64::try_from(value).unwrap_or(i64::MAX));
        }
        let previous = last.as_ref();
        for (operation, current, prior) in [
            ("hits", stats.hits, previous.map_or(0, |last| last.hits)),
            (
                "misses",
                stats.misses,
                previous.map_or(0, |last| last.misses),
            ),
            (
                "coalesced_waiters",
                stats.coalesced_waiters,
                previous.map_or(0, |last| last.coalesced_waiters),
            ),
            (
                "acquisitions",
                stats.acquisitions,
                previous.map_or(0, |last| last.acquisitions),
            ),
            (
                "evictions",
                stats.evictions,
                previous.map_or(0, |last| last.evictions),
            ),
            (
                "capacity_failures",
                stats.capacity_failures,
                previous.map_or(0, |last| last.capacity_failures),
            ),
            (
                "cancelled_acquisitions",
                stats.cancelled_acquisitions,
                previous.map_or(0, |last| last.cancelled_acquisitions),
            ),
        ] {
            metrics
                .cache_totals
                .get_metric_with_label_values(&[operation])?
                .inc_by(current.saturating_sub(prior));
        }
        *last = Some(stats);
        Ok(())
    }

    fn record_preload_report(&self, report: &ProcessPreloadReport) -> Result<()> {
        let metrics = self;
        for (outcome, count) in [
            ("ready", report.ready),
            ("failed", report.failed),
            ("skipped", report.skipped),
        ] {
            metrics
                .preload_artifacts
                .get_metric_with_label_values(&[outcome])?
                .inc_by(u64::from(count));
        }
        metrics.preload_warmed_bytes.inc_by(report.warmed_bytes);
        if report.deadline_hit {
            metrics.preload_deadlines.inc();
        }
        if !report.coordinator_available {
            metrics.preload_coordinator_unavailable.inc();
        }
        Ok(())
    }
}

fn package_metrics() -> Result<&'static PackageMetrics> {
    PACKAGE_METRICS
        .as_ref()
        .map_err(|error| anyhow!("register agentOS package metrics: {error}"))
}

pub(crate) async fn record_process_cache_stats(stats: ProcessPackageCacheStats) -> Result<()> {
    package_metrics()?
        .sample_cache(|| async { Ok(stats) })
        .await
}

pub(crate) fn record_preload_report(report: &ProcessPreloadReport) -> Result<()> {
    package_metrics()?.record_preload_report(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_metrics_have_bounded_labels_and_export_through_rivetkit() {
        let registry = Registry::new();
        let metrics = PackageMetrics::new(&registry).expect("register local test metrics");
        let stats = ProcessPackageCacheStats {
            entries: 2,
            source_entries: 1,
            bytes: 4096,
            pinned_entries: 1,
            pending_acquisitions: 0,
            hits: 3,
            misses: 2,
            coalesced_waiters: 1,
            acquisitions: 2,
            evictions: 0,
            capacity_failures: 0,
            cancelled_acquisitions: 0,
        };
        metrics
            .record_cache_stats(stats)
            .expect("record cache metrics");
        metrics
            .record_cache_stats(ProcessPackageCacheStats { hits: 5, ..stats })
            .expect("record updated cache metrics");
        metrics
            .record_preload_report(&ProcessPreloadReport {
                total: 2,
                ready: 1,
                skipped: 1,
                warmed_bytes: 4096,
                deadline_hit: true,
                coordinator_available: true,
                ..ProcessPreloadReport::default()
            })
            .expect("record preload metrics");

        use rivet_metrics::prometheus::{Encoder, TextEncoder};
        let mut output = Vec::new();
        TextEncoder::new()
            .encode(&registry.gather(), &mut output)
            .expect("encode test metrics");
        let body = String::from_utf8(output).expect("UTF-8 Prometheus metrics");
        assert!(body.contains("agentos_package_cache_current{measure=\"bytes\"} 4096"));
        assert!(body.contains("agentos_package_cache_operations_total{operation=\"hits\"} 5"));
        assert!(body.contains("agentos_preload_deadlines_total 1"));
        assert!(body
            .lines()
            .filter(|line| line.starts_with("agentos_"))
            .all(|line| !line.contains("https://")));

        package_metrics().expect("register global metrics");
        let rendered = rivetkit_core::metrics_endpoint::render_prometheus_metrics()
            .expect("render RivetKit metrics");
        assert!(String::from_utf8(rendered.body)
            .unwrap()
            .contains("agentos_preload_deadlines_total"));
    }

    #[tokio::test]
    async fn concurrent_cache_samples_do_not_regress_or_double_count() {
        let metrics = PackageMetrics::new(&Registry::new()).unwrap();
        let stats = ProcessPackageCacheStats {
            entries: 1,
            source_entries: 1,
            bytes: 10,
            pinned_entries: 0,
            pending_acquisitions: 0,
            hits: 3,
            misses: 0,
            coalesced_waiters: 0,
            acquisitions: 0,
            evictions: 0,
            capacity_failures: 0,
            cancelled_acquisitions: 0,
        };
        let next = ProcessPackageCacheStats {
            hits: 5,
            bytes: 20,
            ..stats
        };
        let (first, second) = futures::join!(
            metrics.sample_cache(|| async {
                tokio::task::yield_now().await;
                Ok(stats)
            }),
            metrics.sample_cache(|| async { Ok(next) }),
        );
        first.unwrap();
        second.unwrap();
        metrics.sample_cache(|| async { Ok(next) }).await.unwrap();
        assert_eq!(metrics.cache_totals.with_label_values(&["hits"]).get(), 5);
        assert_eq!(metrics.cache_gauges.with_label_values(&["bytes"]).get(), 20);
    }
}
