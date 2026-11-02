// `shop.cart.size` was renamed in the registry; with warnings denied, the
// deprecation is an error that names the replacement.
#![deny(warnings)]
use showcase::event;

fn main() {
    event!(shop.order.placed { shop.order.id: "ord-1", shop.cart.size: 2 });
}
