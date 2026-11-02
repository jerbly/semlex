//! The committed fixtures conform to Weaver's published JSON schema for
//! `resolved/2.0` (vendored at the pinned Weaver version).

use serde_json::Value;
use std::path::PathBuf;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn schema() -> Value {
    let text =
        std::fs::read_to_string(fixtures().join("schemas/semconv.resolved.v2.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn validate(rel: &str) {
    let text = std::fs::read_to_string(fixtures().join(rel)).unwrap();
    let instance: Value = serde_saphyr::from_str_with_options(
        &text,
        serde_saphyr::options! { strict_booleans: true },
    )
    .unwrap();
    let validator = jsonschema::validator_for(&schema()).expect("schema compiles");
    let errors: Vec<String> = validator
        .iter_errors(&instance)
        .map(|e| format!("{} at {}", e, e.instance_path()))
        .take(20)
        .collect();
    assert!(
        errors.is_empty(),
        "{rel} does not match Weaver's schema:\n{}",
        errors.join("\n")
    );
}

#[test]
fn fixture_conforms() {
    validate("resolved/resolved.yaml");
}

#[test]
fn otel_semconv_conforms() {
    validate("otel-semconv/resolved.yaml");
}
