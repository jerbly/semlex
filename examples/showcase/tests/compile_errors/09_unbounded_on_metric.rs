// `url.path` is `cardinality: unbounded`: never a metric attribute, not even
// through `extra(…)`.
#![allow(deprecated)]
use showcase::metric;

fn main() {
    metric!(http.server.request.duration: std::time::Duration::from_millis(5), {
        http.request.method: "GET",
        http.route: "/cart/{id}",
        extra(url.path): "/cart/42",
    });
}
