// A function returning `Ready` uses `ready()`, which names a missing
// required attribute (here `shop.order.id`) instead of a type mismatch.
use showcase::{record, span};

fn order_span() -> showcase::spans::shop::orders::insert::Ready {
    let span = span!(shop.orders.insert { db.system.name: "postgresql" }, name: "INSERT orders");
    let span = record!(span, db.namespace: "shop");
    span.ready()
}

fn main() {
    order_span().end();
}
