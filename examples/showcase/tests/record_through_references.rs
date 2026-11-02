//! `record!(&span, …)` finds the span through any number of references, so a
//! helper taking `&Ready` records with the same spelling as the caller.

use showcase::spans::shop::payment::authorize::Ready;
use showcase::{assert_span, end, record, span};

/// A helper that records on a span it was lent.
fn note_provider_timeout(span: &Ready) {
    record!(&span, error.type: "provider_timeout");
}

#[test]
fn helpers_record_through_a_reference() {
    let t = semlex::testing::Telemetry::init();
    let span = span!(shop.payment.authorize { shop.payment.method: "wallet" });
    let span = record!(span, shop.payment.timeout: std::time::Duration::from_secs(2)).ready();
    note_provider_timeout(&span);
    let lent = &span;
    let email: Option<String> = None;
    record!(&lent, shop.customer.email?: hashed(email));
    end!(span);
    assert_span!(t, shop.payment.authorize { shop.payment.method: "wallet", error.type: "provider_timeout" });
}
