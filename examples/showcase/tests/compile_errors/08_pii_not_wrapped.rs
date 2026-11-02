// `shop.customer.email` is `sensitivity: pii`: wrap it in `redacted(…)` or
// `hashed(…)`.
use showcase::{enums::PaymentMethod, record, span};

fn main() {
    let email = String::from("ada@example.com");
    let span = span!(shop.payment.authorize { shop.payment.method: PaymentMethod::Card });
    let span = record!(span, shop.payment.timeout: std::time::Duration::from_secs(5));
    let span = record!(span, shop.customer.email: email);
    span.end();
}
