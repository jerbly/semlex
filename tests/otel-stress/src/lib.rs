//! Example registry crate over all of OpenTelemetry semantic conventions 1.44:
//! a stress test for `registry!()`. Real crates expand a local
//! registry, which holds only what it uses.

semlex::registry!("../../fixtures/otel-semconv/resolved.yaml");
