// `key?: option`: the attribute is set only if the value is `Some`.
use fixture_semconv::{metric, record, span};

fn main() {
    let route: Option<&'static str> = Some("/users/{id}");
    let status: Option<i64> = None;
    let span = span!(http.server.request {
        http.request.method: "GET",
        url.scheme: "https",
        http.route?: route,
    });
    let span = record!(span,
        url.path: "/users/1",
        demo.tenant.code: "ACME",
        http.response.status_code?: status,
    );
    span.end();

    let method: Option<&'static str> = None;
    metric!(http.server.request.duration: 0.5, {
        http.route: "/",
        http.request.method: "GET",
        url.scheme?: method,
    });
}
