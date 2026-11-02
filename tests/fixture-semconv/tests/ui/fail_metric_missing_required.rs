use fixture_semconv::metric;

fn main() {
    metric!(http.server.request.duration: 0.5, { url.scheme: "https" });
}
