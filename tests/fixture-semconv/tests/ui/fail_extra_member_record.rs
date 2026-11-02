// `record!` knows the signal only by the span's type, so naming a member in
// `extra(…)` is reported when the crate is built, at the call site.
#![allow(deprecated)]

fn main() {
    let span = fixture_semconv::span!(http.server.request { http.request.method: "GET", url.scheme: "https" });
    fixture_semconv::record!(&span, extra(http.route): "/");
}
