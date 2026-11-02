//! `attrs!` produces the expected `KeyValue`s (stable attributes; runs in the
//! default build, without `semlex-unstable`).

use fixture_semconv::__semlex::__private::opentelemetry::{Key, KeyValue, StringValue, Value};
use fixture_semconv::{attrs, enums::HttpRequestMethod};

#[test]
fn literals_expressions_enums_and_templates() {
    let route = String::from("/users/{id}");
    let status: u16 = 404;
    let kvs = attrs![
        http.request.method: "GET",
        http.route: route,
        http.response.status_code: status,
        url.scheme: "https",
        http.request.header("content-type"): vec!["text/plain", "charset=utf-8"],
    ];
    assert_eq!(
        kvs,
        [
            KeyValue::new("http.request.method", "GET"),
            KeyValue::new("http.route", "/users/{id}"),
            KeyValue::new("http.response.status_code", 404_i64),
            KeyValue::new("url.scheme", "https"),
            KeyValue::new(
                "http.request.header.content-type",
                Value::Array(
                    vec![
                        StringValue::from("text/plain"),
                        StringValue::from("charset=utf-8")
                    ]
                    .into()
                )
            ),
        ]
    );
    // Keys are static: no allocation for registry keys.
    assert!(matches!(kvs[0].key, ref k if *k == Key::from_static_str("http.request.method")));
}

#[test]
fn enum_values_and_custom() {
    let method = HttpRequestMethod::Post;
    let kvs = attrs![
        http.request.method: method,
        http.request.method: custom("BREW"),
        http.request.method: custom(String::from("PROPFIND")),
        http.request.method: "_OTHER",
    ];
    let values: Vec<String> = kvs
        .iter()
        .map(|kv| kv.value.as_str().into_owned())
        .collect();
    assert_eq!(values, ["POST", "BREW", "PROPFIND", "_OTHER"]);
    assert_eq!(
        HttpRequestMethod::from_static("PUT"),
        HttpRequestMethod::Put
    );
    assert_eq!(HttpRequestMethod::from_static("BREW").as_str(), "BREW");
}

#[test]
fn keyword_segments_are_raw_identifiers() {
    // `exception.type`: `type` is a Rust keyword; the macro path uses `r#type`.
    let kvs = attrs![exception.type: "std::io::Error"];
    assert_eq!(kvs[0], KeyValue::new("exception.type", "std::io::Error"));
}

#[test]
fn integer_literals_are_i64() {
    // Above `i32::MAX`, and suffixed: both are recorded as `i64`.
    let [big, suffixed] = attrs![
        http.response.status_code: 3_000_000_000,
        http.response.status_code: 200usize,
    ];
    assert_eq!(big.value, Value::I64(3_000_000_000));
    assert_eq!(suffixed.value, Value::I64(200));
}

#[test]
fn borrowed_strings() {
    let route = String::from("/users/{id}");
    let path: &str = "/users/7";
    let [a, b, c] = attrs![http.route: &route, url.path: path, url.path: "/static"];
    assert_eq!(a.value, Value::String(StringValue::from("/users/{id}")));
    assert_eq!(b.value, Value::String(StringValue::from("/users/7")));
    assert_eq!(c.value, Value::String(StringValue::from("/static")));
}
