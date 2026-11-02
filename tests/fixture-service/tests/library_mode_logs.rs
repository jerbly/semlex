//! Library mode: without `semlex::init`, semlex has no logger provider, so
//! `event!` and a span's `exception` event go out as `tracing` events named
//! after the event, which the application's bridge turns into log-based
//! events. Its own test binary: it installs the process-wide global tracer
//! provider.

mod common;

use opentelemetry::global;

#[test]
fn exceptions_are_log_based_events_through_the_tracing_bridge() {
    use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
    use tracing_subscriber::prelude::*;

    // Library mode: no `semlex::init`, so no logger provider in semlex. The
    // application's `tracing` bridge carries the `exception` event instead.
    let t = common::telemetry();
    global::set_tracer_provider(t.tracers.clone());
    let subscriber =
        tracing_subscriber::registry().with(OpenTelemetryTracingBridge::new(&t.loggers));
    tracing::subscriber::with_default(subscriber, || {
        let span = fixture_semconv::span!(http.server.request { http.request.method: "GET", url.scheme: "https" });
        {
            // `event!` in the span's context, through the `tracing` fallback.
            let _in_span = span.child_context().attach();
            fixture_service::start_session("s-lib".into());
        }
        span.end_with_error(&std::io::Error::other("disk full"));
    });

    let spans = t.spans.get_finished_spans().unwrap();
    let span = spans.iter().find(|s| s.name == "GET").expect("the span");
    assert!(span.events.is_empty(), "no span events");
    t.loggers.force_flush().unwrap();
    let logs = t.logs.get_emitted_logs().unwrap();
    let record = &logs
        .iter()
        .find(|l| l.record.event_name() == Some("exception"))
        .expect("an exception log record")
        .record;
    let cx = record.trace_context().expect("in a trace");
    assert_eq!(
        cx.span_id,
        span.span_context.span_id(),
        "in the span's context"
    );
    assert!(record
        .attributes_iter()
        .any(|(k, v)| k.as_str() == "exception.message"
            && matches!(v, opentelemetry::logs::AnyValue::String(s) if s.as_str() == "disk full")));

    // `event!`: the registry event's name, its attributes, the span's trace.
    let event = &logs
        .iter()
        .find(|l| l.record.event_name() == Some("session.start"))
        .expect("the session.start event via the bridge")
        .record;
    assert_eq!(
        event.trace_context().expect("in a trace").span_id,
        span.span_context.span_id()
    );
    assert!(
        event
            .attributes_iter()
            .any(|(k, v)| k.as_str() == "session.id"
                && matches!(v, opentelemetry::logs::AnyValue::String(s) if s.as_str() == "s-lib"))
    );
}
