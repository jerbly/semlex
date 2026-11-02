// `reviewed:` needs a non-empty reason, as a string literal.
fn main() {
    let _ = fixture_semconv::attrs![unregistered("demo.batch", reviewed: ""): "b-1"];
    let _ = fixture_semconv::attrs![unregistered("demo.batch", approved: "yes"): "b-1"];
}
