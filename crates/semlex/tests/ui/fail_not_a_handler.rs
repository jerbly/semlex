semlex::registry!("../../../../fixtures/resolved/resolved.yaml", { annotations: { demo.case: crate::plain, demo.slo: ignore } });

/// Not marked `#[semlex::handler]`.
mod plain {}

fn main() {}
