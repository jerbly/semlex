// An optional value never satisfies a required attribute.
use fixture_semconv::{record, span};

fn main() {
    let path: Option<&'static str> = Some("/");
    let span = span!(http.server.request { http.request.method: "GET", url.scheme: "https" });
    let span = record!(span, demo.tenant.code: "ACME", url.path?: path);
    span.end();
}
