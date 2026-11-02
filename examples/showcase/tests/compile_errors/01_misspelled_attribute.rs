// A misspelled key: rustc suggests the registry's spelling.
use showcase::metric;

fn main() {
    metric!(shop.cart.items_sold: 1, {
        shop.payment.method: "card",
        shop.tenant.cod: "ACME",
    });
}
