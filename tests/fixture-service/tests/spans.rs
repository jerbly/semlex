//! Spans: names from the registry's templates (re-evaluated at `end()`),
//! kinds from the registry, attributes, and parent/child relationships.

mod common;

use opentelemetry::trace::SpanKind;

#[test]
fn handler_spans_are_exported() {
    let t = common::telemetry();
    semlex::init(t.config()).unwrap();

    fixture_service::handle_request("/users/42", "/users/{id}", 200);

    let spans = t.spans.get_finished_spans().unwrap();
    assert_eq!(spans.len(), 2, "{spans:#?}");
    let db = spans
        .iter()
        .find(|s| s.span_kind == SpanKind::Client)
        .expect("db span");
    let server = spans
        .iter()
        .find(|s| s.span_kind == SpanKind::Server)
        .expect("server span");

    // Name: `{http.request.method}` at start, `{http.request.method} {http.route}`
    // once the route was recorded.
    assert_eq!(server.name, "GET /users/{id}");
    assert_eq!(
        db.name, "SELECT users",
        "explicit `name:` for a prose-named span"
    );

    let attrs = common::attrs(server.attributes.iter());
    assert_eq!(
        attrs,
        [
            ("demo.retry.attempt", "0"),
            ("demo.tenant.code", "ACME"),
            ("http.request.method", "GET"),
            ("http.response.status_code", "200"),
            ("http.route", "/users/{id}"),
            ("url.path", "/users/42"),
            ("url.scheme", "https"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string()))
    );

    // The db span is a child of the server span.
    assert_eq!(db.span_context.trace_id(), server.span_context.trace_id());
    assert_eq!(db.parent_span_id, server.span_context.span_id());
    assert_eq!(server.instrumentation_scope.name(), "demo.fixture-service");
}
