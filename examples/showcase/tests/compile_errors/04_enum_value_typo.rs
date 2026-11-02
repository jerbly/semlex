// Enum literals are checked against the registry's members.
use showcase::{record, span};

fn main() {
    let span = span!(http.server.request { http.request.method: "PSOT", url.scheme: "https" });
    let span = record!(span, url.path: "/", shop.tenant.code: "ACME");
    span.end();
}
