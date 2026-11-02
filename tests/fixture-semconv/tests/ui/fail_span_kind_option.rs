use fixture_semconv::span;

fn main() {
    let _ = span!(http.server.request { http.request.method: "GET", url.scheme: "https" }, kind: Server);
}
