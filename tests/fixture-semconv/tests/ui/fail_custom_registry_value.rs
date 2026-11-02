use fixture_semconv::attrs;

fn main() {
    let _ = attrs![http.request.method: custom("GET")];
}
