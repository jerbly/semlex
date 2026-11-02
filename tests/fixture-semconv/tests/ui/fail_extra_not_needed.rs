use fixture_semconv::{attrs, entity};

fn main() {
    let _ = attrs![extra(http.route): "/"];
    let _ = entity!(host { host.name: "h", extra(http.route): "/" });
    fixture_semconv::warn!(extra(http.route): "/", "x");
}
