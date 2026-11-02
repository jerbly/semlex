//! **Telemetry tests**: `semlex::testing` installs in-memory exporters, and
//! `assert_span!` / `assert_metric!` / `assert_event!` check, with
//! registry-checked attributes, that the checkout emits what the registry
//! says it should.

use showcase::enums::PaymentMethod;
use showcase::{CheckoutRequest, Customer};
use showcase::{assert_event, assert_metric, assert_span};

fn request(cart: &str, payment: PaymentMethod) -> CheckoutRequest {
    CheckoutRequest {
        method: http::Method::POST,
        path: format!("/cart/{cart}/checkout"),
        tenant: "acme".into(),
        customer: Some(Customer {
            id: "cust-7".into(),
            email: "ada@example.com".into(),
        }),
        skus: vec!["SKU-1".into(), "SKU-2".into()],
        payment,
    }
}

#[test]
fn a_checkout_is_traced_measured_and_recorded() {
    let t = semlex::testing::Telemetry::init();
    assert_eq!(
        showcase::checkout(&request("100", PaymentMethod::Card)),
        Ok("ord-1001".into())
    );

    assert_span!(t, http.server.request {
        http.request.method: "POST",
        url.path: "/cart/100/checkout",
        http.route: "/cart/{id}/checkout",
        http.response.status_code: 201,
        // Normalized by the `shop.case` handler.
        shop.tenant.code: "ACME",
        // PII takes its wrappers here too: compares with what was recorded.
        shop.customer.id: redacted("cust-7"),
    });
    assert_span!(t, shop.payment.authorize { shop.payment.method: "card" });
    assert_span!(t, shop.orders.insert { db.operation.name: "INSERT", shop.order.id: "ord-1001" });
    assert_metric!(t, http.server.request.duration { http.route: "/cart/{id}/checkout", http.response.status_code: 201 });
    assert_metric!(t, shop.cart.items_sold { shop.payment.method: "card", shop.tenant.code: "ACME" });
    assert_event!(t, shop.order.placed { shop.order.id: "ord-1001", shop.cart.item_count: 2 });
}

#[test]
fn a_declined_payment_is_an_error() {
    let t = semlex::testing::Telemetry::init();
    assert_eq!(
        showcase::checkout(&request("200", PaymentMethod::Invoice)),
        Err(402)
    );

    assert_span!(t, http.server.request {
        url.path: "/cart/200/checkout",
        http.response.status_code: 402,
        error.type: "payment_declined",
    });
    assert_metric!(t, http.server.request.duration { http.response.status_code: 402, error.type: "payment_declined" });

    // `end!`: `error.type`, status `Error` with the message, and the `exception` event
    // with the source chain.
    assert_span!(t, shop.payment.authorize { shop.payment.method: "invoice", error.type: "declined" });
    let spans = t.spans();
    let authorize = spans
        .iter()
        .find(|s| s.name == "authorize invoice")
        .expect("the payment span");
    assert_eq!(
        authorize.status,
        opentelemetry::trace::Status::error("payment declined")
    );
    // Log-based, not a span event, in the span's context.
    assert!(authorize.events.is_empty());
    let (exception, _) = exception_event(t, authorize);
    assert_eq!(exception["exception.message"], "payment declined");
    assert_eq!(exception["exception.type"], "showcase::PaymentDeclined");
    assert_eq!(
        exception["exception.stacktrace"],
        "payment declined\n\nCaused by:\n    0: invoices need a credit check"
    );
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
