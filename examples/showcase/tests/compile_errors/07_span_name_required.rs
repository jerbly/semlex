// `db.client` names its spans in prose, so the call site must give a name.
use showcase::{record, span};

fn main() {
    let span = span!(db.client { db.system.name: "postgresql" });
    let span = record!(span, db.namespace: "shop");
    span.end();
}
