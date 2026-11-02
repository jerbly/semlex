use fixture_semconv::attrs;

fn main() {
    #[allow(deprecated)]
    let _ = attrs![unregistered("http.route"): "/"];
}
