use fixture_semconv::attrs;

fn main() {
    let _ = attrs![http.request.header: vec!["text/plain"]];
}
