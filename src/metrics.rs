//! Step 9 of browser-multi-instance — per-instance Prometheus metrics.
//!
//! Process-wide singletons accessed via [`record_invocation`] +
//! [`set_chrome_alive`] + [`inc_chrome_restart`]. Scrape body via
//! [`scrape`] is published through the broker
//! `plugin.browser.metrics.scrape` topic (Stage 5).
//!
//! Cardinality: every series is labelled `instance="<label>"` so
//! operators with N declared instances see N-fold series count for
//! per-tool counters/histograms. The boot loop seeds
//! `browser_instances_configured` at configure time so dashboards
//! can plot N at a glance.

use std::sync::OnceLock;

use prometheus::{
    Encoder, HistogramOpts, HistogramVec, IntCounterVec, IntGauge, IntGaugeVec, Opts,
    Registry, TextEncoder,
};

struct Metrics {
    registry: Registry,
    tool_invocations_total: IntCounterVec,
    tool_latency_seconds: HistogramVec,
    chrome_alive: IntGaugeVec,
    chrome_restarts_total: IntCounterVec,
    instances_configured: IntGauge,
}

fn metrics() -> &'static Metrics {
    static CELL: OnceLock<Metrics> = OnceLock::new();
    CELL.get_or_init(|| {
        let registry = Registry::new();

        let tool_invocations_total = IntCounterVec::new(
            Opts::new(
                "browser_tool_invocations_total",
                "Total browser tool invocations dispatched.",
            ),
            &["instance", "tool", "ok"],
        )
        .expect("counter ctor");
        registry
            .register(Box::new(tool_invocations_total.clone()))
            .expect("register counter");

        let tool_latency_seconds = HistogramVec::new(
            HistogramOpts::new(
                "browser_tool_latency_seconds",
                "Per-tool dispatch latency in seconds.",
            )
            .buckets(vec![
                0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0,
            ]),
            &["instance", "tool"],
        )
        .expect("histogram ctor");
        registry
            .register(Box::new(tool_latency_seconds.clone()))
            .expect("register histogram");

        let chrome_alive = IntGaugeVec::new(
            Opts::new(
                "browser_chrome_alive",
                "1 when a live Chrome process is attached to the instance, 0 otherwise.",
            ),
            &["instance"],
        )
        .expect("gauge ctor");
        registry
            .register(Box::new(chrome_alive.clone()))
            .expect("register gauge");

        let chrome_restarts_total = IntCounterVec::new(
            Opts::new(
                "browser_chrome_restarts_total",
                "Total Chrome (re)launches for the instance.",
            ),
            &["instance"],
        )
        .expect("restarts ctor");
        registry
            .register(Box::new(chrome_restarts_total.clone()))
            .expect("register restarts");

        let instances_configured = IntGauge::new(
            "browser_instances_configured",
            "Number of operator-declared browser instances.",
        )
        .expect("instances ctor");
        registry
            .register(Box::new(instances_configured.clone()))
            .expect("register instances");

        Metrics {
            registry,
            tool_invocations_total,
            tool_latency_seconds,
            chrome_alive,
            chrome_restarts_total,
            instances_configured,
        }
    })
}

/// Record one dispatch outcome. `instance` is the resolved label
/// (`"legacy"` for the per-agent_id fallback path).
pub fn record_invocation(instance: &str, tool: &str, ok: bool, duration_secs: f64) {
    let m = metrics();
    let ok_str = if ok { "true" } else { "false" };
    m.tool_invocations_total
        .with_label_values(&[instance, tool, ok_str])
        .inc();
    m.tool_latency_seconds
        .with_label_values(&[instance, tool])
        .observe(duration_secs);
}

pub fn set_chrome_alive(instance: &str, alive: bool) {
    metrics()
        .chrome_alive
        .with_label_values(&[instance])
        .set(if alive { 1 } else { 0 });
}

pub fn inc_chrome_restart(instance: &str) {
    metrics()
        .chrome_restarts_total
        .with_label_values(&[instance])
        .inc();
}

pub fn set_instances_configured(n: i64) {
    metrics().instances_configured.set(n);
}

/// Encode the registry as Prometheus text-format. Used by
/// `auto_discovery::metrics_scrape` to fill the broker reply.
pub fn scrape() -> String {
    let encoder = TextEncoder::new();
    let metric_families = metrics().registry.gather();
    let mut buf = Vec::new();
    encoder
        .encode(&metric_families, &mut buf)
        .expect("encode metrics");
    String::from_utf8(buf).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    #[test]
    #[serial]
    fn record_invocation_increments_counter_and_observes_latency() {
        record_invocation("metricsT1", "browser_navigate", true, 0.123);
        let body = scrape();
        // Counter — one labelled series with the value `1`.
        assert!(
            body.contains(
                "browser_tool_invocations_total{instance=\"metricsT1\",ok=\"true\",tool=\"browser_navigate\"} 1"
            ),
            "counter line missing; got:\n{body}"
        );
        // Histogram — `_count`, `_sum`, and bucketed series.
        assert!(
            body.contains(
                "browser_tool_latency_seconds_count{instance=\"metricsT1\",tool=\"browser_navigate\"} 1"
            ),
            "histogram count line missing; got:\n{body}"
        );
    }

    #[test]
    #[serial]
    fn scrape_body_contains_help_and_type_lines() {
        // Prometheus emits HELP/TYPE only for metric families that
        // have at least one observation in the registry — seed each
        // family first so all 5 show up in the scrape body.
        record_invocation("seed", "browser_navigate", true, 0.1);
        set_chrome_alive("seed", true);
        inc_chrome_restart("seed");
        set_instances_configured(1);

        let body = scrape();
        for line in [
            "# HELP browser_tool_invocations_total",
            "# TYPE browser_tool_invocations_total counter",
            "# HELP browser_tool_latency_seconds",
            "# TYPE browser_tool_latency_seconds histogram",
            "# HELP browser_chrome_alive",
            "# TYPE browser_chrome_alive gauge",
            "# HELP browser_chrome_restarts_total",
            "# TYPE browser_chrome_restarts_total counter",
            "# HELP browser_instances_configured",
            "# TYPE browser_instances_configured gauge",
        ] {
            assert!(
                body.contains(line),
                "scrape body missing `{line}`; got:\n{body}"
            );
        }
    }

    #[test]
    #[serial]
    fn set_chrome_alive_toggles_gauge() {
        set_chrome_alive("toggleT1", true);
        let on = scrape();
        assert!(
            on.contains("browser_chrome_alive{instance=\"toggleT1\"} 1"),
            "alive=true line missing; got:\n{on}"
        );
        set_chrome_alive("toggleT1", false);
        let off = scrape();
        assert!(
            off.contains("browser_chrome_alive{instance=\"toggleT1\"} 0"),
            "alive=false line missing; got:\n{off}"
        );
    }

    #[test]
    #[serial]
    fn inc_chrome_restart_increments() {
        inc_chrome_restart("restartT1");
        inc_chrome_restart("restartT1");
        let body = scrape();
        assert!(
            body.contains("browser_chrome_restarts_total{instance=\"restartT1\"} 2"),
            "restarts line missing; got:\n{body}"
        );
    }

    #[test]
    #[serial]
    fn set_instances_configured_writes_gauge() {
        set_instances_configured(7);
        let body = scrape();
        assert!(
            body.contains("browser_instances_configured 7"),
            "instances_configured gauge missing; got:\n{body}"
        );
        set_instances_configured(0);
    }
}
