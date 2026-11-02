//! The README's "with semlex" example, compiled and run so it can't drift.

use showcase::{metric, record, span};

fn checkout(path: String, tenant: String, n: u64) {
    let span = span!(http.server.request {
        http.request.method: "POST",
        url.scheme: "https",
    });
    let span = record!(span,
        url.path: path,
        shop.tenant.code: tenant.clone(),
        http.route: "/cart/{id}/checkout",
        http.response.status_code: 201,
    );
    span.end(); // named "POST /cart/{id}/checkout" by the registry's template

    metric!(shop.cart.items_sold: n, {
        shop.payment.method: "card",
        shop.tenant.code: tenant,
    });
}

#[test]
fn readme_example() {
    let t = semlex::testing::Telemetry::init();
    checkout("/cart/9/checkout".into(), "acme".into(), 3);
    showcase::assert_span!(t, http.server.request { url.path: "/cart/9/checkout", shop.tenant.code: "ACME" });
    showcase::assert_metric!(t, shop.cart.items_sold { shop.payment.method: "card" });
}
