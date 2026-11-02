//! Telemetry assertion tests with `semlex::testing`: the
//! handler emits the signals the registry defines, with these attributes.

use std::time::Duration;

#[test]
fn handler_emits_its_signals() {
    let t = semlex::testing::Telemetry::init();
    fixture_service::handle_request("/users/42", "/users/{id}", 200);
    fixture_service::record_request("/users/{id}", Duration::from_millis(3));
    fixture_service::start_session("s-1".into());

    fixture_semconv::assert_span!(t, http.server.request { http.route: "/users/{id}", http.response.status_code: 200 });
    fixture_semconv::assert_span!(t, db.client { db.namespace: "app" });
    fixture_semconv::assert_metric!(t, http.server.request.duration { http.route: "/users/{id}", url.scheme: "https" });
    fixture_semconv::assert_event!(t, session.start { session.id: "s-1" });
}

#[test]
fn failed_assertions_list_what_was_emitted() {
    let t = semlex::testing::Telemetry::init();
    fixture_service::handle_request("/users/7", "/users/{id}", 200);
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        fixture_semconv::assert_span!(t, db.client { db.namespace: "other" });
    }))
    .expect_err("the assertion fails");
    let message = failure
        .downcast_ref::<String>()
        .expect("a formatted message");
    // Other tests share the exporters, so check the parts, not the order.
    for part in [
        "no finished `db.client` span (Client) with db.namespace = \"other\"\nfinished spans:\n",
        "  - Client `SELECT users` (Unset): db.system.name = \"postgresql\", db.namespace = \"app\"",
    ] {
        assert!(message.contains(part), "missing {part:?} in:\n{message}");
    }
}

#[test]
fn metrics_take_unregistered_attributes() {
    let t = semlex::testing::Telemetry::init();
    fixture_service::record_failed_request("/failing/{id}", Duration::from_millis(9), "timeout");
    #[allow(deprecated, clippy::disallowed_methods)] // reviewed: test
    let () = fixture_semconv::assert_metric!(t, http.server.request.duration {
        http.route: "/failing/{id}",
        unregistered("demo.error.reason"): "timeout",
    });
}

#[test]
fn signals_take_extra_registry_attributes() {
    let t = semlex::testing::Telemetry::init();
    #[allow(deprecated, clippy::disallowed_methods)] // reviewed: test
    let () = fixture_semconv::metric!(http.server.request.count: 1, {
        http.request.method: "DELETE",
        extra(http.response.status_code): 503,
    });
    fixture_semconv::assert_metric!(t, http.server.request.count {
        http.request.method: "DELETE",
        http.response.status_code: 503,
    });
}

#[test]
fn events_take_extra_template_attributes() {
    let t = semlex::testing::Telemetry::init();
    // A template key is only known at run time, so the `tracing` fallback
    // leaves it out; the event itself carries it.
    #[allow(deprecated, clippy::disallowed_methods)] // reviewed: test
    let () = fixture_semconv::event!(session.start {
        session.id: "s-extra",
        extra(http.request.header("x-trace")): vec!["on".to_string()],
    });
    fixture_semconv::assert_event!(t, session.start { session.id: "s-extra" });
}

#[test]
fn in_place_records_survive_a_dropped_span() {
    let t = semlex::testing::Telemetry::init();
    {
        let span = fixture_semconv::span!(http.server.request {
            http.request.method: "GET",
            url.scheme: "https",
        });
        let span = fixture_semconv::record!(span, url.path: "/cancelled", demo.tenant.code: "ACME");
        // Recorded as soon as known, without consuming the span.
        let record_route = || fixture_semconv::record!(&span, http.route: "/cancelled/{id}");
        record_route();
        // Dropped without `end()`, as when a future is cancelled.
    }
    fixture_semconv::assert_span!(t, http.server.request {
        url.path: "/cancelled",
        http.route: "/cancelled/{id}",
    });
}

#[test]
fn metric_literals_fit_any_int_instrument() {
    let t = semlex::testing::Telemetry::init();
    // A plain `1` on a counter (unsigned): no `1u64` needed.
    fixture_semconv::metric!(http.server.request.count: 1, { http.request.method: "PUT" });
    fixture_semconv::assert_metric!(t, http.server.request.count { http.request.method: "PUT" });
}
