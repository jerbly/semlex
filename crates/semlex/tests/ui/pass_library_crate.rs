// A library that owns its registry and instruments itself:
// `visibility: crate` keeps the generated modules and macros private (and
// makes `crate::span!` work), and `allow_unstable` accepts its own
// development items without a feature. No warnings for the many generated
// items it doesn't use.
#![deny(warnings)]

semlex::registry!("../../../../fixtures/resolved/resolved.yaml", {
    annotations: { demo.case: ignore, demo.slo: ignore },
    visibility: crate,
    allow_unstable: true,
});

mod handlers {
    pub fn handle(user: Option<String>) {
        let span = crate::span!(http.server.request { http.request.method: "GET", url.scheme: "https" });
        let span = crate::record!(span, url.path: "/", demo.tenant.code: "ACME", user.id?: redacted(user));
        span.end();
        // `session.start` and `session.id` are development: no feature needed.
        crate::event!(session.start { session.id: "s-1" });
        crate::warn!(http.route: "/", "done");
    }
}

fn main() {
    handlers::handle(None);
}
