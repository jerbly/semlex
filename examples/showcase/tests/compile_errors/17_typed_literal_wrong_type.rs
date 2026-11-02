// Number literals are `i64` or `f64`, suffixed or not: `1usize` works for an
// `int` attribute, and `5i64` or `5` for a string gets the usual message.
use showcase::event;

fn main() {
    event!(shop.order.placed { shop.order.id: "ord-1", shop.cart.item_count: 1usize });
    event!(shop.order.placed { shop.order.id: 5i64, shop.cart.item_count: 1 });
    event!(shop.order.placed { shop.order.id: 5, shop.cart.item_count: 1 });
}
