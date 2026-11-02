use fixture_semconv::warn;

fn main() {
    warn!(http.request.header("accept"): vec!["*/*"], "odd header");
}
