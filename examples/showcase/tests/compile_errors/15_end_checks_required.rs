// `end!` ends with the checked `end()`: error paths need the required
// attributes too (`end_with_error` is the unchecked alternative).
use showcase::{end, span};

fn main() {
    let e = std::io::Error::other("connection reset");
    let span = span!(http.server.request { http.request.method: "GET", url.scheme: "https" });
    end!(span, error: &e, error.type: "io");
}
