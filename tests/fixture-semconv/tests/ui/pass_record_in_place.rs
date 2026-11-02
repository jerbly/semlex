// `record!(&span, …)` records without consuming the span: in closures, as
// values become known, and for required attributes once they're set.
use fixture_semconv::{record, span};

fn main() {
    let span = span!(http.server.request { http.request.method: "GET", url.scheme: "https" });
    // Required attributes change the span's type: consume and rebind.
    let span = record!(span, url.path: "/users/1", demo.tenant.code: "ACME");

    // Non-required attributes, through a shared reference.
    record!(&span, http.route: "/users/{id}");
    let status: Option<i64> = Some(200);
    let respond = || record!(&span, http.response.status_code?: status);
    respond();
    // A required attribute that's already set can be recorded again.
    record!(&span, url.path: "/users/1/");
    #[allow(deprecated, clippy::disallowed_methods)] // reviewed: example
    let () = record!(&span, unregistered("demo.retry.attempt"): 1);
    span.end();
}
