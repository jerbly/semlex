#![allow(deprecated)]
use fixture_semconv::metric;

fn main() {
    metric!(http.server.request.count: 1, {
        http.request.method: "GET",
        extra(http.response.status_code): "503",
    });
    metric!(http.server.request.count: 1, {
        http.request.method: "GET",
        extra(http.rout): "/",
    });
}
