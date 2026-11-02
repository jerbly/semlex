semlex::registry!("../../../../fixtures/resolved/resolved.yaml", {
    annotations: { demo.case: ignore, demo.slo: ignore },
    conversions: { http.route: ::std::string::String },
});

fn main() {}
