use fixture_semconv::{record, span};

fn main() {
    let span = span!(http.server.request { http.request.method: "GET", url.scheme: "https" });
    // Required and not set yet: needs `let span = record!(span, …)`.
    record!(&span, url.path: "/users/1");
    // Sampling-relevant: only at start, in place or not.
    record!(&span, http.request.method: "POST");
}
