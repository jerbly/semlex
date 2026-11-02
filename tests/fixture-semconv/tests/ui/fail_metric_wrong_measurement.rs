use fixture_semconv::metric;

fn main() {
    metric!(http.server.request.count: 1.5, { http.request.method: "GET" });
}
