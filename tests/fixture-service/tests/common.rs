//! In-memory exporters for telemetry assertions.

#![allow(dead_code, missing_docs)]

use opentelemetry_sdk::logs::{InMemoryLogExporter, SdkLoggerProvider};
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, Metric, ResourceMetrics};
use opentelemetry_sdk::metrics::{InMemoryMetricExporter, PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider};

pub struct Telemetry {
    pub meters: SdkMeterProvider,
    pub metrics: InMemoryMetricExporter,
    pub loggers: SdkLoggerProvider,
    pub logs: InMemoryLogExporter,
    pub tracers: SdkTracerProvider,
    pub spans: InMemorySpanExporter,
}

pub fn telemetry() -> Telemetry {
    let metrics = InMemoryMetricExporter::default();
    let meters = SdkMeterProvider::builder()
        .with_reader(PeriodicReader::builder(metrics.clone()).build())
        .build();
    let logs = InMemoryLogExporter::default();
    let loggers = SdkLoggerProvider::builder()
        .with_simple_exporter(logs.clone())
        .build();
    let spans = InMemorySpanExporter::default();
    let tracers = SdkTracerProvider::builder()
        .with_simple_exporter(spans.clone())
        .build();
    Telemetry {
        meters,
        metrics,
        loggers,
        logs,
        tracers,
        spans,
    }
}

impl Telemetry {
    /// semlex config using these providers.
    pub fn config(&self) -> semlex::Config {
        semlex::Config::new()
            .with_meter_provider(self.meters.clone())
            .with_logger_provider(self.loggers.clone())
            .with_tracer_provider(self.tracers.clone())
    }

    /// Every exported metric, after a flush.
    pub fn exported(&self) -> Vec<ResourceMetrics> {
        self.meters.force_flush().unwrap();
        self.metrics.get_finished_metrics().unwrap()
    }
}

/// The exported metric with this name, and its scope name/version/schema URL.
pub fn find<'a>(
    exported: &'a [ResourceMetrics],
    name: &str,
) -> Option<(&'a Metric, String, String, String)> {
    exported
        .iter()
        .flat_map(|rm| rm.scope_metrics())
        .find_map(|sm| {
            let s = sm.scope();
            sm.metrics().find(|m| m.name() == name).map(|m| {
                (
                    m,
                    s.name().to_string(),
                    s.version().unwrap_or_default().to_string(),
                    s.schema_url().unwrap_or_default().to_string(),
                )
            })
        })
}

/// A metric's single histogram data point: (count, sum, attributes).
pub fn f64_histogram(m: &Metric) -> (u64, f64, Vec<(String, String)>) {
    let AggregatedMetrics::F64(opentelemetry_sdk::metrics::data::MetricData::Histogram(h)) =
        m.data()
    else {
        panic!("not an f64 histogram: {:?}", m.data());
    };
    let p = h.data_points().next().expect("one data point");
    (p.count(), p.sum(), attrs(p.attributes()))
}

pub fn attrs<'a>(kvs: impl Iterator<Item = &'a opentelemetry::KeyValue>) -> Vec<(String, String)> {
    let mut v: Vec<_> = kvs
        .map(|kv| (kv.key.to_string(), kv.value.as_str().into_owned()))
        .collect();
    v.sort();
    v
}
