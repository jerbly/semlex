use fixture_semconv::{record, span};

#[derive(Debug)]
struct Failed;
impl std::fmt::Display for Failed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("failed")
    }
}
impl std::error::Error for Failed {}

fn main() {
    // Required attributes may be given in `span!` or recorded later.
    let span = span!(http.server.request {
        http.request.method: "GET",
        url.scheme: "https",
        url.path: "/",
    });
    let child_cx = span.child_context();
    let _guard = child_cx.attach();
    record!(current, http.route: "/");
    let span = record!(span, demo.tenant.code: "ACME", http.response.status_code: 200);
    let _ = span.otel_mut().is_recording();
    span.end();

    // Error paths don't need the remaining required attributes.
    let span = span!(http.server.request { http.request.method: "GET", url.scheme: "https" });
    span.end_with_error(&Failed);

    // Leaving semlex.
    let span = span!(http.server.request { http.request.method: "GET", url.scheme: "https" });
    let _cx = span.into_otel();
}
