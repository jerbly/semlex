// `http.response.status_code` is an `int` in the registry.
use showcase::{record, span};

fn main() {
    let span = span!(http.server.request { http.request.method: "GET", url.scheme: "https" });
    let span = record!(span, url.path: "/", shop.tenant.code: "ACME");
    let span = record!(span, http.response.status_code: "201");
    span.end();
}
