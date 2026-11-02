//! `span.in_scope(f)`: the span is current inside `f`, so
//! `record!(current, …)` records on it and spans started there are its
//! children.

use semlex::StartedSpan;
use showcase::enums::PaymentMethod;
use showcase::{record, span};

#[test]
fn the_span_is_current_inside() {
    let t = semlex::testing::Telemetry::init();
    let span = span!(shop.payment.authorize { shop.payment.method: PaymentMethod::Card });
    let span = record!(span, shop.payment.timeout: std::time::Duration::from_secs(1));
    let items = span.in_scope(|| {
        record!(current, shop.cart.item_count: 3);
        let child = span!(shop.payment.authorize { shop.payment.method: PaymentMethod::Wallet });
        let child = record!(child, shop.payment.timeout: std::time::Duration::from_secs(1));
        child.end();
        3
    });
    assert_eq!(items, 3, "returns the closure's value");
    // Through the trait too, for helpers taking `impl StartedSpan`.
    StartedSpan::in_scope(&span, || record!(current, shop.order.id: "ord-7"));
    span.end();

    let spans = t.spans();
    let parent = spans
        .iter()
        .find(|s| s.name == "authorize card")
        .expect("parent");
    let child = spans
        .iter()
        .find(|s| s.name == "authorize wallet")
        .expect("child");
    assert_eq!(child.parent_span_id, parent.span_context.span_id());
    let keys: Vec<&str> = parent.attributes.iter().map(|kv| kv.key.as_str()).collect();
    assert!(keys.contains(&"shop.cart.item_count"), "{keys:?}");
    assert!(keys.contains(&"shop.order.id"), "{keys:?}");
}
