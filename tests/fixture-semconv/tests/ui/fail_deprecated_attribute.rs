#![deny(deprecated)]
use fixture_semconv::attrs;

fn main() {
    let _ = attrs![http.method: "GET"];
}
