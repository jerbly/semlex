// `unregistered(…)` always warns (and fails CI through clippy) until it's
// approved: `unregistered("key", reviewed: "why")`.
#![deny(warnings)]
use showcase::{record, span};

fn main() {
    let span = span!(http.server.request { http.request.method: "GET", url.scheme: "https" });
    let span = record!(span, url.path: "/", shop.tenant.code: "ACME");
    let span = record!(span, unregistered("shop.coupon.code"): "SPRING");
    span.end();
}
