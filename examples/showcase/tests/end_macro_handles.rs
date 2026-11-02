//! `end!` with a boxed error, an owned result and a borrowed temporary (its
//! own process: the test telemetry is shared, and span names would collide
//! with `end_macro`).

use opentelemetry::trace::Status;
use showcase::enums::PaymentMethod;
use showcase::{end, record, span};

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

#[test]
fn boxed_errors_and_owned_results() {
    let t = semlex::testing::Telemetry::init();
    // `error:` takes any handle that derefs to `dyn Error`; its concrete
    // type isn't known, so there's no `exception.type`.
    let span = span!(shop.payment.authorize { shop.payment.method: PaymentMethod::Card });
    let span = record!(span, shop.payment.timeout: std::time::Duration::from_secs(1));
    let boxed: Box<dyn std::error::Error + Send + Sync> = Box::new(Timeout);
    end!(span, error: &boxed, error.type: "timeout");
    // `result:` takes the result by value too.
    let span = span!(shop.payment.authorize { shop.payment.method: PaymentMethod::Wallet });
    let span = record!(span, shop.payment.timeout: std::time::Duration::from_secs(1));
    let result: Result<(), Timeout> = Err(Timeout);
    end!(span, result: result, error.type: |e| e.class());

    let spans = t.spans();
    let card = spans
        .iter()
        .find(|s| s.name == "authorize card")
        .expect("boxed");
    assert_eq!(card.status, Status::error("provider timed out"));
    let (exception, _) = exception_event(t, card);
    assert_eq!(exception["exception.message"], "provider timed out");
    assert!(!exception.contains_key("exception.type"));
    let wallet = spans
        .iter()
        .find(|s| s.name == "authorize wallet")
        .expect("owned");
    assert_eq!(wallet.status, Status::error("provider timed out"));
    let (exception, _) = exception_event(t, wallet);
    assert_eq!(exception["exception.type"], "end_macro_handles::Timeout");
}

/// Wraps an error, built in place at the call site.
#[derive(Debug)]
struct Wrapped(Timeout);

impl std::fmt::Display for Wrapped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "wrapped: {}", self.0)
    }
}

impl std::error::Error for Wrapped {}

#[test]
fn error_can_borrow_a_temporary() {
    let t = semlex::testing::Telemetry::init();
    let span = span!(shop.payment.authorize { shop.payment.method: PaymentMethod::Invoice });
    let span = record!(span, shop.payment.timeout: std::time::Duration::from_secs(1));
    // `&Wrapped(..)` is a temporary (a call, so not promoted to a constant):
    // it must live through the whole `end!`.
    let timeout = || Timeout;
    end!(span, error: &Wrapped(timeout()), error.type: "timeout");

    let spans = t.spans();
    let invoice = spans
        .iter()
        .find(|s| s.name == "authorize invoice")
        .expect("temporary");
    assert_eq!(invoice.status, Status::error("wrapped: provider timed out"));
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
