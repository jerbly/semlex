// `url.scheme` is sampling-relevant: samplers only see attributes given at
// start, so it must be in `span!`.
use showcase::{record, span};

fn main() {
    let span = span!(http.server.request { http.request.method: "GET" });
    let span = record!(span, url.scheme: "https", url.path: "/", shop.tenant.code: "ACME");
    span.end();
}
