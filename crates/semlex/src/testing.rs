//! Telemetry assertion tests: in-memory exporters and
//! assertions that a signal was emitted with given attributes.
//!
//! ```ignore
//! #[test]
//! fn request_is_traced() {
//!     let t = semlex::testing::Telemetry::init();
//!     my_service::handle_request("/users/42");
//!     crate::assert_span!(t, http.server.request { http.route: "/users/{id}" });
//!     crate::assert_metric!(t, http.server.request.duration { http.request.method: "GET" });
//! }
//! ```
//!
//! [`Telemetry::init`] installs the providers once per process (it calls
//! `semlex::init`). Tests in one binary share the exporters, so assertions
//! ask whether a matching signal *exists*; they don't count signals.
//!
//! Requires the `testing` feature of `semlex`.

use crate::Config;
use crate::span::SpanType;
use opentelemetry::KeyValue;
use opentelemetry_sdk::logs::{InMemoryLogExporter, SdkLoggerProvider};
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, Metric, MetricData};
use opentelemetry_sdk::metrics::{InMemoryMetricExporter, PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider, SpanData};
use std::sync::OnceLock;

/// In-memory exporters behind semlex's providers.
pub struct Telemetry {
    meters: SdkMeterProvider,
    metrics: InMemoryMetricExporter,
    loggers: SdkLoggerProvider,
    logs: InMemoryLogExporter,
    tracers: SdkTracerProvider,
    spans: InMemorySpanExporter,
}

static TELEMETRY: OnceLock<Telemetry> = OnceLock::new();

impl Telemetry {
    /// Installs in-memory exporters and calls `semlex::init`, once per
    /// process; later calls return the same instance.
    pub fn init() -> &'static Telemetry {
        Self::init_with(|config| config)
    }

    /// Like [`init`](Self::init), with extra config (e.g. opt-in signals:
    /// `|c| c.enable("demo.cache.fill_ratio")`). Only the first call's
    /// config applies.
    pub fn init_with(configure: impl FnOnce(Config) -> Config) -> &'static Telemetry {
        TELEMETRY.get_or_init(|| {
            let metrics = InMemoryMetricExporter::default();
            let meters =
                SdkMeterProvider::builder().with_reader(PeriodicReader::builder(metrics.clone()).build()).build();
            let logs = InMemoryLogExporter::default();
            let loggers = SdkLoggerProvider::builder().with_simple_exporter(logs.clone()).build();
            let spans = InMemorySpanExporter::default();
            let tracers = SdkTracerProvider::builder().with_simple_exporter(spans.clone()).build();
            let config = Config::new()
                .with_meter_provider(meters.clone())
                .with_logger_provider(loggers.clone())
                .with_tracer_provider(tracers.clone());
            crate::init(configure(config))
                .expect("semlex::init was already called; use Telemetry::init before any other init in tests");
            Telemetry { meters, metrics, loggers, logs, tracers, spans }
        })
    }

    /// Every span finished so far.
    pub fn spans(&self) -> Vec<SpanData> {
        let _ = self.tracers.force_flush();
        self.spans.get_finished_spans().unwrap_or_default()
    }

    /// Every metric exported so far (after a flush).
    pub fn metrics(&self) -> Vec<opentelemetry_sdk::metrics::data::ResourceMetrics> {
        let _ = self.meters.force_flush();
        self.metrics.get_finished_metrics().unwrap_or_default()
    }

    /// Every log record (events) emitted so far.
    pub fn logs(&self) -> Vec<opentelemetry_sdk::logs::in_memory_exporter::LogDataWithResource> {
        let _ = self.loggers.force_flush();
        self.logs.get_emitted_logs().unwrap_or_default()
    }

    #[doc(hidden)]
    #[track_caller]
    pub fn assert_span(&self, ty: &SpanType, expected: &[KeyValue]) {
        let spans = self.spans();
        let found = spans
            .iter()
            .any(|s| s.span_kind == ty.kind && contains(s.attributes.iter(), expected));
        if !found {
            let seen: Vec<String> = spans
                .iter()
                .map(|s| {
                    format!(
                        "  - {:?} `{}` ({:?}): {}",
                        s.span_kind,
                        s.name,
                        s.status,
                        show(s.attributes.iter())
                    )
                })
                .collect();
            panic!(
                "no finished `{}` span ({:?}) with {}\nfinished spans:\n{}",
                ty.id,
                ty.kind,
                show(expected.iter()),
                if seen.is_empty() {
                    "  (none: is the span ended? was Telemetry::init called first?)".into()
                } else {
                    seen.join("\n")
                }
            );
        }
    }

    #[doc(hidden)]
    #[track_caller]
    pub fn assert_metric(&self, name: &str, unit: &str, expected: &[KeyValue]) {
        let exported = self.metrics();
        let metrics: Vec<&Metric> = exported
            .iter()
            .flat_map(|rm| rm.scope_metrics())
            .flat_map(|sm| sm.metrics())
            .filter(|m| m.name() == name)
            .collect();
        let points: Vec<Vec<KeyValue>> = metrics.iter().flat_map(|m| point_attributes(m)).collect();
        let found = metrics.iter().any(|m| m.unit() == unit)
            && points.iter().any(|p| contains(p.iter(), expected));
        if !found {
            let seen: Vec<String> = points
                .iter()
                .map(|p| format!("  - {}", show(p.iter())))
                .collect();
            panic!(
                "no `{name}` measurement (unit `{unit}`) with {}\n`{name}` data points:\n{}",
                show(expected.iter()),
                if seen.is_empty() {
                    "  (none: was the metric recorded? is it opt-in and not enabled?)".into()
                } else {
                    seen.join("\n")
                }
            );
        }
    }

    #[doc(hidden)]
    #[track_caller]
    pub fn assert_event(&self, event_name: &str, expected: &[KeyValue]) {
        let logs = self.logs();
        let events: Vec<_> = logs
            .iter()
            .filter(|l| l.record.event_name() == Some(event_name))
            .collect();
        let attrs = |l: &opentelemetry_sdk::logs::in_memory_exporter::LogDataWithResource| -> Vec<KeyValue> {
            l.record.attributes_iter().map(|(k, v)| KeyValue::new(k.clone(), any_text(v))).collect()
        };
        let found = events.iter().any(|l| {
            let a = attrs(l);
            expected.iter().all(|e| {
                a.iter()
                    .any(|kv| kv.key == e.key && kv.value.as_str() == e.value.as_str())
            })
        });
        if !found {
            let seen: Vec<String> = events
                .iter()
                .map(|l| format!("  - {}", show(attrs(l).iter())))
                .collect();
            panic!(
                "no `{event_name}` event with {}\n`{event_name}` events:\n{}",
                show(expected.iter()),
                if seen.is_empty() {
                    "  (none: was the event emitted? is a logger provider configured?)".into()
                } else {
                    seen.join("\n")
                }
            );
        }
    }
}

fn contains<'a>(actual: impl Iterator<Item = &'a KeyValue> + Clone, expected: &[KeyValue]) -> bool {
    expected
        .iter()
        .all(|e| actual.clone().any(|a| a.key == e.key && a.value == e.value))
}

/// `"text"` for strings, plain for numbers and booleans, as written in code.
fn value(v: &opentelemetry::Value) -> String {
    match v {
        opentelemetry::Value::String(s) => format!("{:?}", s.as_str()),
        other => other.as_str().into_owned(),
    }
}

fn show<'a>(kvs: impl Iterator<Item = &'a KeyValue>) -> String {
    let v: Vec<String> = kvs
        .map(|kv| format!("{} = {}", kv.key, value(&kv.value)))
        .collect();
    if v.is_empty() {
        "no attributes".into()
    } else {
        v.join(", ")
    }
}

fn any_text(v: &opentelemetry::logs::AnyValue) -> opentelemetry::Value {
    use opentelemetry::logs::AnyValue as A;
    match v {
        A::Int(i) => opentelemetry::Value::I64(*i),
        A::Double(d) => opentelemetry::Value::F64(*d),
        A::Boolean(b) => opentelemetry::Value::Bool(*b),
        A::String(s) => opentelemetry::Value::String(s.clone()),
        other => opentelemetry::Value::from(format!("{other:?}")),
    }
}

fn point_attributes(m: &Metric) -> Vec<Vec<KeyValue>> {
    fn of<T>(d: &MetricData<T>) -> Vec<Vec<KeyValue>> {
        match d {
            MetricData::Gauge(g) => g
                .data_points()
                .map(|p| p.attributes().cloned().collect())
                .collect(),
            MetricData::Sum(s) => s
                .data_points()
                .map(|p| p.attributes().cloned().collect())
                .collect(),
            MetricData::Histogram(h) => h
                .data_points()
                .map(|p| p.attributes().cloned().collect())
                .collect(),
            MetricData::ExponentialHistogram(h) => h
                .data_points()
                .map(|p| p.attributes().cloned().collect())
                .collect(),
        }
    }
    match m.data() {
        AggregatedMetrics::F64(d) => of(d),
        AggregatedMetrics::U64(d) => of(d),
        AggregatedMetrics::I64(d) => of(d),
    }
}
