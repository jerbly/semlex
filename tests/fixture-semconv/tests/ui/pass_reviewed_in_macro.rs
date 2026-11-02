// `reviewed: "why"` inside `extra(…)` / `unregistered(…)` approves the use:
// no warning, no clippy marker, and no `#[allow]` on a `let`.
#![deny(warnings)]

use fixture_semconv::{attrs, metric, record, span};

fn main() {
    metric!(http.server.request.count: 1, {
        http.request.method: "GET",
        extra(http.response.status_code, reviewed: "per-status counts until the metric has it"): 503,
    });
    let span = span!(http.server.request {
        http.request.method: "GET",
        url.scheme: "https",
        unregistered("demo.experiment", reviewed: "experiment flag"): true,
    });
    record!(&span, extra(demo.timeout, reviewed: "timeout on the request span"): std::time::Duration::from_secs(1));
    let span = record!(span, url.path: "/", demo.tenant.code: "ACME");
    span.end();
    let _ = attrs![unregistered("demo.batch", reviewed: "batch id for the importer"): "b-1"];
}
