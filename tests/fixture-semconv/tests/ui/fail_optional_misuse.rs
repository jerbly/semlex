use fixture_semconv::{attrs, record, span};

fn main() {
    let route: Option<&'static str> = None;
    let _ = attrs![http.route?: route];
    let span = span!(http.server.request { http.request.method: "GET", url.scheme: "https" });
    let span = record!(span, url.path?: "/");
    let span = record!(span, url.path: "/", demo.tenant.code: "ACME");
    span.end();
}
