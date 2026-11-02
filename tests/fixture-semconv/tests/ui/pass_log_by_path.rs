use tracing::*;

fn main() {
    // Call semlex's log macros by path next to tracing's.
    fixture_semconv::warn!(http.route: "/", "registry-checked");
    warn!(answer = 42, "plain tracing");
}
