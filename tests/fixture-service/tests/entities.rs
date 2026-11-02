//! The Resource is built with `entity!`, and entity associations of the
//! signals the service used are checked against it.

mod common;

use opentelemetry::KeyValue;
use opentelemetry_sdk::Resource;

#[test]
fn resource_from_entities_and_association_checks() {
    let attributes = fixture_service::resource_attributes("i-123");
    let resource = Resource::builder_empty()
        .with_attributes(attributes.clone())
        .build();
    assert_eq!(
        resource
            .get(&"service.instance.id".into())
            .map(|v| v.to_string())
            .as_deref(),
        Some("i-123")
    );
    assert_eq!(
        resource
            .get(&"host.name".into())
            .map(|v| v.to_string())
            .as_deref(),
        Some("web-1")
    );

    // Configure semlex with a Resource that lacks a host or container entity.
    let service_only: Vec<KeyValue> = attributes
        .iter()
        .filter(|kv| kv.key.as_str().starts_with("service."))
        .cloned()
        .collect();
    let t = common::telemetry();
    semlex::init(t.config().with_resource(service_only.clone())).unwrap();

    fixture_service::handle_request("/users/42", "/users/{id}", 200);
    fixture_service::record_request("/users/{id}", std::time::Duration::from_millis(3));

    // `http.server.request` needs all_of(service, one_of(host, container)).
    let violations =
        semlex::check_associations().expect_err("the span's association isn't satisfied");
    assert_eq!(violations.len(), 1, "{violations:#?}");
    assert_eq!(
        (violations[0].kind, violations[0].signal),
        ("span", "http.server.request")
    );
    assert_eq!(
        violations[0].expected,
        "all_of(service, one_of(host, container))"
    );
    assert!(violations[0].to_string().starts_with("span `http.server.request` must be associated with all_of(service, one_of(host, container))"));

    // The full Resource (service + host) satisfies every signal used.
    assert_eq!(semlex::check_associations_with(&attributes), Ok(()));
}
