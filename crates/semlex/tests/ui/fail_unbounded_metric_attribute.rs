semlex::registry!("../../../../crates/semlex/tests/ui/data/unbounded-metric.yaml");

fn main() {
    let _ = metrics::http::server::duration::builder().with(attr::url::path, "/users/42");
}
