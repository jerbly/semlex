use fixture_semconv::event;

fn main() {
    event!(session.start { session.id: "s-1" }, severity: loud);
}
