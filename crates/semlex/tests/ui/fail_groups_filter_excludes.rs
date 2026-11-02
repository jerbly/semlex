semlex::registry!("../../../../fixtures/resolved/resolved.yaml", { annotations: { demo.case: ignore, demo.slo: ignore }, groups: ["http.*"] });

fn main() {
    let _ = spans::db::client::builder();
}
