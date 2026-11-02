use fixture_semconv::attrs;

fn main() {
    let m: &'static str = "GET";
    let _ = attrs![http.request.method: m];
}
