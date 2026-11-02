use fixture_semconv::attrs;

fn main() {
    let n: i64 = 42;
    let _ = attrs![http.route: n];
}
