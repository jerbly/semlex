// A registry attribute the metric doesn't define.
use showcase::metric;

fn main() {
    metric!(shop.cart.items_sold: 1, {
        shop.payment.method: "card",
        shop.tenant.code: "ACME",
        url.scheme: "https",
    });
}
