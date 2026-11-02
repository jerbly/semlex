use fixture_semconv::*;
use tracing::*;

fn main() {
    warn!(http.route: "/", "glob imports of both make `warn!` ambiguous");
}
