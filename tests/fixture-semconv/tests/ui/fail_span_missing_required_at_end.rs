use fixture_semconv::span;

fn main() {
    let span = span!(http.server.request { http.request.method: "GET", url.scheme: "https" });
    span.end();
}
