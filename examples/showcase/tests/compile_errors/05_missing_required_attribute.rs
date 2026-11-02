// `url.path` and `shop.tenant.code` are required on `http.server.request`:
// the span can't end without them.
use showcase::span;

fn main() {
    let span = span!(http.server.request { http.request.method: "GET", url.scheme: "https" });
    span.end();
}
