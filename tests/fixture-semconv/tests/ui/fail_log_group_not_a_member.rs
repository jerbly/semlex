use fixture_semconv::error;

fn main() {
    error!(group exception { exception.type: "E", exception.message: "m", http.route: "/" }, "failed");
}
