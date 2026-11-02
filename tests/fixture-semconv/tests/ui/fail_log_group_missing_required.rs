use fixture_semconv::error;

fn main() {
    error!(group exception { exception.type: "std::io::Error" }, "failed");
}
