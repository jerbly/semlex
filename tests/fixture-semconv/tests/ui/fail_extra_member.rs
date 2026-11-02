// `extra(…)` is for attributes the signal doesn't define: naming a member
// is an error.
#![allow(deprecated)]

fn main() {
    let () = fixture_semconv::metric!(http.server.request.count: 1, {
        http.request.method: "GET",
        extra(http.request.method): "GET",
    });
}
