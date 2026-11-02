use fixture_semconv::entity;

fn main() {
    let _ = entity!(service { service.name: "fixture-service" });
}
