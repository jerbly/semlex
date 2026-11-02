//! The log macros produce `tracing` events with registry keys as field
//! names, exported through `opentelemetry-appender-tracing`.

use opentelemetry::logs::{AnyValue, Severity};
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_sdk::logs::{InMemoryLogExporter, SdkLoggerProvider};
use tracing_subscriber::prelude::*;

fn attr(record: &opentelemetry_sdk::logs::SdkLogRecord, key: &str) -> Option<AnyValue> {
    record
        .attributes_iter()
        .find(|(k, _)| k.as_str() == key)
        .map(|(_, v)| v.clone())
}

#[test]
fn log_macros_export_registry_attributes() {
    let exporter = InMemoryLogExporter::default();
    let provider = SdkLoggerProvider::builder()
        .with_simple_exporter(exporter.clone())
        .build();
    let subscriber =
        tracing_subscriber::registry().with(OpenTelemetryTracingBridge::new(&provider));

    tracing::subscriber::with_default(subscriber, || {
        fixture_service::log_upstream_timeout("/users/{id}");
        fixture_service::log_failure(&std::io::Error::other("disk full"));
    });

    let logs = exporter.get_emitted_logs().unwrap();
    assert_eq!(logs.len(), 2);

    // Not events: no `event_name` (the bridge would otherwise use the
    // `tracing` callsite name, `event <file>:<line>`).
    for log in &logs {
        assert_eq!(
            log.record.event_name().unwrap_or_default(),
            "",
            "{:?}",
            log.record
        );
    }

    let warn = &logs[0].record;
    assert_eq!(warn.severity_number(), Some(Severity::Warn));
    assert_eq!(
        warn.body(),
        Some(&AnyValue::String("upstream timed out after 250ms".into()))
    );
    assert_eq!(
        attr(warn, "http.route"),
        Some(AnyValue::String("/users/{id}".into()))
    );
    assert_eq!(
        attr(warn, "http.response.status_code"),
        Some(AnyValue::Int(504)),
        "ints stay ints"
    );

    let error = &logs[1].record;
    assert_eq!(error.severity_number(), Some(Severity::Error));
    assert_eq!(
        attr(error, "exception.type"),
        Some(AnyValue::String("std::io::Error".into()))
    );
    assert_eq!(
        attr(error, "exception.message"),
        Some(AnyValue::String("disk full".into()))
    );
}
