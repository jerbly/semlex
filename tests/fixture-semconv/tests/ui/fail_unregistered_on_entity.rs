use fixture_semconv::entity;

fn main() {
    let _ = entity!(host { host.name: "web-1", unregistered("host.rack"): "r1" });
}
