//! Library mode: a crate instrumented with semlex, in an application that
//! never calls `semlex::init` but installs OpenTelemetry's global providers
//! itself. Spans and metrics go through whatever global provider is installed
//! when they're recorded, so nothing is stuck on the no-op provider that's
//! global at startup. Events need `semlex::init` (OpenTelemetry Rust has no
//! global logger provider).

mod common;

use opentelemetry::global;
use std::time::Duration;

#[test]
fn spans_and_metrics_use_the_global_providers_without_init() {
    // Before the application installs providers: the no-op globals.
    fixture_service::handle_request("/users/1", "/users/{id}", 200);
    fixture_service::record_request("/before", Duration::from_secs(1));
    fixture_service::start_session("s-before".into());

    // The application installs its providers; it never calls `semlex::init`.
    let t = common::telemetry();
    global::set_tracer_provider(t.tracers.clone());
    global::set_meter_provider(t.meters.clone());
    assert!(!semlex::is_initialized());

    // The same call sites now record.
    fixture_service::handle_request("/users/2", "/users/{id}", 200);
    fixture_service::record_request("/after", Duration::from_secs(2));
    fixture_service::start_session("s-after".into());

    let spans = t.spans.get_finished_spans().unwrap();
    let paths: Vec<String> = spans
        .iter()
        .flat_map(|s| s.attributes.iter())
        .filter(|kv| kv.key.as_str() == "url.path")
        .map(|kv| kv.value.as_str().into_owned())
        .collect();
    assert_eq!(
        paths,
        ["/users/2"],
        "only spans after the providers were installed"
    );
    let names: Vec<&str> = spans.iter().map(|s| s.name.as_ref()).collect();
    assert!(names.contains(&"GET /users/{id}"), "{names:?}");
    assert!(
        names.contains(&"SELECT users"),
        "child spans too: {names:?}"
    );
    let scope = spans[0].instrumentation_scope.name();
    assert_eq!(scope, "demo.fixture-service", "same scope as with init");

    let exported = t.exported();
    let (m, ..) = common::find(&exported, "http.server.request.duration").expect("metric exported");
    let (count, sum, _) = common::f64_histogram(m);
    assert_eq!((count, sum), (1, 2.0), "only the measurement after install");

    // Events have no global provider to use.
    t.loggers.force_flush().unwrap();
    assert!(t.logs.get_emitted_logs().unwrap().is_empty());
}
