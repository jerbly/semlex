//! `end!(span, result: &r, error.type: |e| class)`: `end()` on `Ok`; on `Err`,
//! `error.type`, the `exception` event and status `Error` with the error's
//! message, then `end()`. `record_error` sets the same status and event.

use opentelemetry::trace::Status;
use showcase::enums::PaymentMethod;
use showcase::{assert_span, end, record, span};

#[derive(Debug)]
struct Timeout;

impl std::fmt::Display for Timeout {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("provider timed out")
    }
}

impl std::error::Error for Timeout {}

impl Timeout {
    fn class(&self) -> &'static str {
        "timeout"
    }
}

fn authorize(method: PaymentMethod, result: Result<(), Timeout>) {
    let span = span!(shop.payment.authorize { shop.payment.method: method });
    let span = record!(span, shop.payment.timeout: std::time::Duration::from_secs(1));
    end!(span, result: &result, error.type: |e| e.class());
}

#[test]
fn ok_ends_and_err_follows_the_conventions() {
    let t = semlex::testing::Telemetry::init();
    authorize(PaymentMethod::Wallet, Ok(()));
    authorize(PaymentMethod::Card, Err(Timeout));

    assert_span!(t, shop.payment.authorize { shop.payment.method: "card", error.type: "timeout" });
    let spans = t.spans();
    let wallet = spans
        .iter()
        .find(|s| s.name == "authorize wallet")
        .expect("ok span");
    assert_eq!(wallet.status, Status::Unset);
    assert!(wallet.events.is_empty());
    let card = spans
        .iter()
        .find(|s| s.name == "authorize card")
        .expect("error span");
    assert_eq!(card.status, Status::error("provider timed out"));
    // Log-based, not a span event.
    assert!(card.events.is_empty());
    let (exception, severity) = exception_event(t, card);
    assert_eq!(severity, Some(opentelemetry::logs::Severity::Error));
    assert_eq!(exception["exception.message"], "provider timed out");
    assert_eq!(exception["exception.type"], "end_macro::Timeout");
    // No sources, so no chain.
    assert!(!exception.contains_key("exception.stacktrace"));
}

#[test]
fn record_error_sets_the_same_status_and_event() {
    let t = semlex::testing::Telemetry::init();
    let span = span!(shop.payment.authorize { shop.payment.method: PaymentMethod::Invoice });
    let span = record!(span, shop.payment.timeout: std::time::Duration::from_secs(1));
    span.record_error(&Timeout);
    span.end();

    let spans = t.spans();
    let invoice = spans
        .iter()
        .find(|s| s.name == "authorize invoice")
        .expect("span");
    assert_eq!(invoice.status, Status::error("provider timed out"));
    assert!(invoice.events.is_empty());
    let (exception, _) = exception_event(t, invoice);
    assert_eq!(exception["exception.message"], "provider timed out");
}

/// The log-based `exception` event emitted in `span`'s context: its
/// attributes as text, and its severity.
fn exception_event(
    t: &semlex::testing::Telemetry,
    span: &opentelemetry_sdk::trace::SpanData,
) -> (
    std::collections::BTreeMap<String, String>,
    Option<opentelemetry::logs::Severity>,
) {
    let logs = t.logs();
    let record = logs
        .iter()
        .map(|l| &l.record)
        .find(|r| {
            r.event_name() == Some("exception")
                && r.trace_context().is_some_and(|c| {
                    c.trace_id == span.span_context.trace_id()
                        && c.span_id == span.span_context.span_id()
                })
        })
        .expect("an `exception` log record in the span's context");
    let attributes = record
        .attributes_iter()
        .map(|(k, v)| {
            let text = match v {
                opentelemetry::logs::AnyValue::String(s) => s.to_string(),
                other => format!("{other:?}"),
            };
            (k.to_string(), text)
        })
        .collect();
    (attributes, record.severity_number())
}
