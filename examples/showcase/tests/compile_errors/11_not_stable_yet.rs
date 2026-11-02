// `crypto` is a development member of `shop.payment.method`: it needs the registry
// crate's `semlex-unstable` feature.
use showcase::metric;

fn main() {
    metric!(shop.cart.items_sold: 1, {
        shop.payment.method: "crypto",
        shop.tenant.code: "ACME",
    });
}
