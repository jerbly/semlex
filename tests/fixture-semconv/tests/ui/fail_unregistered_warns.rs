#![deny(deprecated)]
use fixture_semconv::attrs;

fn main() {
    let _ = attrs![unregistered("demo.retry.attempt"): 2];
}
