// Approved uses: the allow goes on a `let` (rustc ignores it on a bare macro
// statement) or on an item.
use fixture_semconv::{attrs, metric, record, span};

#[allow(deprecated, clippy::disallowed_methods)] // reviewed: example
fn log_retry(n: i64) {
    fixture_semconv::warn!(http.route: "/", unregistered("demo.retry.attempt"): n, "retrying");
}

fn main() {
    #[allow(deprecated, clippy::disallowed_methods)] // reviewed: example
    let kvs = attrs![http.route: "/", unregistered("demo.retry.attempt"): 2, unregistered("demo.retry.reason"): "timeout"];
    assert_eq!(kvs.len(), 3);

    #[allow(deprecated, clippy::disallowed_methods)] // reviewed: example
    let span = span!(http.server.request {
        http.request.method: "GET",
        url.scheme: "https",
        unregistered("demo.queue.depth"): 3,
    });
    #[allow(deprecated, clippy::disallowed_methods)] // reviewed: example
    let span = record!(span, unregistered("demo.retry.attempt"): 1);
    let span = record!(span, url.path: "/", demo.tenant.code: "ACME");
    span.end();
    log_retry(1);

    #[allow(deprecated, clippy::disallowed_methods)] // reviewed: example
    let () = metric!(http.server.request.count: 1, {
        http.request.method: "GET",
        unregistered("demo.error.reason"): "timeout",
    });

    // `extra(…)`: registry attributes the signal doesn't define, typed and
    // checked as usual.
    #[allow(deprecated, clippy::disallowed_methods)] // reviewed: example
    let () = metric!(http.server.request.count: 1, {
        http.request.method: "GET",
        extra(http.response.status_code): 503,
    });
    #[allow(deprecated, clippy::disallowed_methods)] // reviewed: example
    let span = span!(http.server.request {
        http.request.method: "GET",
        url.scheme: "https",
        extra(demo.timeout): std::time::Duration::from_secs(1),
    });
    let span = record!(span, url.path: "/", demo.tenant.code: "ACME");
    span.end();
    // A template the metric doesn't define.
    #[allow(deprecated, clippy::disallowed_methods)] // reviewed: example
    let () = metric!(http.server.request.count: 1, {
        http.request.method: "GET",
        extra(http.request.header("x-retry")): vec!["1".to_string()],
    });
}
