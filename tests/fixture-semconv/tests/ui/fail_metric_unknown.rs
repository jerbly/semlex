use fixture_semconv::metric;

fn main() {
    metric!(http.server.request.durations: 0.5);
}
