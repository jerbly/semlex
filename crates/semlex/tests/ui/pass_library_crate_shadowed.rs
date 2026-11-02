// With `visibility: crate`, the crate's own names win over the macros' short
// names instead of clashing with them: here `warn` is `tracing::warn`, and
// semlex's is still reachable through `__semlex_macros`.
#![deny(warnings)]

use tracing::warn;

semlex::registry!("../../../../fixtures/resolved/resolved.yaml", {
    annotations: { demo.case: ignore, demo.slo: ignore },
    visibility: crate,
    allow_unstable: true,
});

fn main() {
    warn!(answer = 42, "plain tracing");
    crate::__semlex_macros::warn!(http.route: "/", "semlex");
    let span = crate::span!(http.server.request { http.request.method: "GET", url.scheme: "https" });
    let span = crate::record!(span, url.path: "/", demo.tenant.code: "ACME");
    span.end();
}
