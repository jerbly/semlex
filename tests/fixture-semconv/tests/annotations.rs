//! Annotation effects at runtime: a user handler's transform, PII wrappers,
//! `conversions` and `code_generation.rust.type`.

use fixture_semconv::__semlex::__private::opentelemetry::{KeyValue, Value};
use fixture_semconv::attrs;
use std::time::Duration;

fn text(kv: &KeyValue) -> String {
    kv.value.as_str().into_owned()
}

#[test]
fn user_handler_transforms_runtime_values() {
    let tenant = String::from("acme-eu");
    let kvs = attrs![demo.tenant.code: "ACME-EU", demo.tenant.code: tenant];
    assert_eq!(text(&kvs[0]), "ACME-EU");
    assert_eq!(
        text(&kvs[1]),
        "ACME-EU",
        "`demo.case: upper` upper-cases runtime values"
    );
}

#[test]
fn pii_values_are_redacted_or_hashed() {
    let email = String::from("alice@example.com");
    let kvs = attrs![user.email: redacted(email.clone()), user.email: hashed(email)];
    assert_eq!(text(&kvs[0]), semlex_pii_redacted());
    let hash = text(&kvs[1]);
    assert_eq!(hash.len(), 64);
    assert!(!hash.contains("alice"));
}

fn semlex_pii_redacted() -> &'static str {
    fixture_semconv::__semlex::pii::REDACTED
}

#[test]
fn conversions_from_http_method() {
    let method = http::Method::POST;
    let kvs = attrs![http.request.method: &method, http.request.method: http::Method::from_bytes(b"BREW").unwrap()];
    assert_eq!(text(&kvs[0]), "POST");
    assert_eq!(text(&kvs[1]), "BREW", "unknown methods become `Custom`");
}

#[test]
fn rust_type_override_takes_a_duration() {
    let kvs = attrs![demo.timeout: Duration::from_millis(1500)];
    assert_eq!(kvs[0].value, Value::F64(1.5));
}
