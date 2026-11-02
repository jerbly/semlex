use fixture_semconv::{record, span};

fn main() {
    let span = span!(http.server.request { http.request.method: "GET", url.scheme: "https" });
    let _span = record!(span, demo.timeout: std::time::Duration::from_secs(1));
}
