// A registry that refers to its dependency's attributes (`error.type`,
// `server.address`) without importing them: semlex takes their definitions
// from the catalog. `error.type` is an open enum with only `_OTHER`, so it
// takes plain strings, literal or not.
#![deny(warnings)]

semlex::registry!("../../../../crates/semlex-schema/tests/data/with-dependency/resolved.yaml", {
    visibility: crate,
});

#[derive(Debug)]
struct Failed;

impl std::fmt::Display for Failed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("failed")
    }
}

impl std::error::Error for Failed {}

fn main() {
    let span = crate::span!(app.job.run {});
    let span = crate::record!(span, app.job.id: "j-1");
    crate::record!(&span, error.type: "timeout", server.address: String::from("jobs.local"));
    let class = String::from("timeout");
    crate::record!(&span, error.type: class);
    crate::end!(span, error: &Failed, error.type: "failed");
}
