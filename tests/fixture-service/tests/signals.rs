//! Metrics and events reach the exporters with the registry's names, units
//! and attributes, the call site's scope, and the current trace context.

mod common;

use opentelemetry::Context;
use opentelemetry::logs::AnyValue;
use opentelemetry::trace::{SpanContext, SpanId, TraceContextExt, TraceFlags, TraceId, TraceState};
use opentelemetry_sdk::metrics::data::{AggregatedMetrics, MetricData};
use std::time::{Duration, SystemTime};

#[test]
fn metrics_and_events_are_exported() {
    let t = common::telemetry();
    semlex::init(t.config().enable("demo.cache.fill_ratio")).unwrap();

    fixture_service::record_request("/users/{id}", Duration::from_millis(1500));
    fixture_service::record_cache_fill(0.4);

    let trace_id = TraceId::from_hex("4bf92f3577b34da6a3ce929d0e0e4736").unwrap();
    let before = SystemTime::now();
    let cx = Context::current().with_remote_span_context(SpanContext::new(
        trace_id,
        SpanId::from_hex("00f067aa0ba902b7").unwrap(),
        TraceFlags::SAMPLED,
        true,
        TraceState::default(),
    ));
    {
        let _guard = cx.attach();
        fixture_service::start_session("s-42".into());
    }
    let after = SystemTime::now();

    // The histogram: name, unit, description, value, attributes, scope.
    let exported = t.exported();
    let (m, scope, version, schema_url) =
        common::find(&exported, "http.server.request.duration").expect("exported");
    assert_eq!(m.unit(), "s");
    assert_eq!(m.description(), "Duration of HTTP server requests.");
    let (count, sum, attrs) = common::f64_histogram(m);
    assert_eq!(
        (count, sum),
        (1, 1.5),
        "the Duration is recorded in seconds"
    );
    assert_eq!(
        attrs,
        [
            ("http.request.method".to_string(), "GET".to_string()),
            ("http.route".to_string(), "/users/{id}".to_string()),
            ("url.scheme".to_string(), "https".to_string()),
        ]
    );
    assert_eq!(scope, "demo.fixture-service");
    assert_eq!(version, env!("CARGO_PKG_VERSION"));
    assert_eq!(schema_url, "https://semlex.example/schemas/0.1.0");

    // The enabled opt-in gauge.
    let (gauge, ..) = common::find(&exported, "demo.cache.fill_ratio")
        .expect("enabled opt-in metric is exported");
    let AggregatedMetrics::F64(MetricData::Gauge(g)) = gauge.data() else {
        panic!("f64 gauge")
    };
    assert_eq!(g.data_points().next().unwrap().value(), 0.4);

    // The event: name, attributes, trace context, time.
    let logs = t.logs.get_emitted_logs().unwrap();
    assert_eq!(logs.len(), 1);
    let record = &logs[0].record;
    assert_eq!(record.event_name(), Some("session.start"));
    let attrs: Vec<(String, AnyValue)> = record
        .attributes_iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    assert_eq!(
        attrs,
        [("session.id".to_string(), AnyValue::String("s-42".into()))]
    );
    assert_eq!(record.trace_context().map(|tc| tc.trace_id), Some(trace_id));
    let time = record.timestamp().expect("the event has a timestamp");
    assert!(before <= time && time <= after, "{time:?}");
}
