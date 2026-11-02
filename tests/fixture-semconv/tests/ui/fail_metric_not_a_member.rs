use fixture_semconv::metric;

fn main() {
    metric!(http.server.request.duration: 0.5, {
        http.request.method: "GET",
        http.route: "/",
        user.email: redacted("alice@example.com"),
    });
}
