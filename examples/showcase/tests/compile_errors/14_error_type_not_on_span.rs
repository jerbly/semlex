// `end!` records `error.type` on the span, so the span must define it
// (`db.client` doesn't): the same membership check as any attribute.
use showcase::{end, record, span};

fn main() {
    let e = std::io::Error::other("connection reset");
    let span = span!(db.client { db.system.name: "postgresql" }, name: "SELECT orders");
    let span = record!(span, db.namespace: "shop");
    end!(span, error: &e, error.type: "io");
}
