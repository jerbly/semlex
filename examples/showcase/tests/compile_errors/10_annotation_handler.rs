// The shop's own annotation `shop.case: upper`, enforced on literals by the
// registry crate's handler.
use showcase::metric;

fn main() {
    metric!(shop.checkout.active: 1, { shop.tenant.code: "acme" });
}
