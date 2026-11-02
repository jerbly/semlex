//! Non-stable attributes and members, with the `semlex-unstable` feature.

use fixture_semconv::__semlex::__private::opentelemetry::{KeyValue, Value};
use fixture_semconv::{attrs, enums::RpcGrpcStatusCode};

#[test]
fn unstable_items_with_opt_in() {
    let kvs = attrs![
        http.request.method: "QUERY",          // development member of a stable enum
        rpc.grpc.status_code: 2,                 // int enum
        demo.cache.hit: true,                    // boolean enum
        demo.weights: vec![0.25, 0.75],          // double[]
        demo.label("team"): "payments",          // template[string]
        demo.tier: "pro",                        // enum with a renamed type (ServiceTier)
    ];
    assert_eq!(
        kvs,
        [
            KeyValue::new("http.request.method", "QUERY"),
            KeyValue::new("rpc.grpc.status_code", 2_i64),
            KeyValue::new("demo.cache.hit", true),
            KeyValue::new("demo.weights", Value::Array(vec![0.25, 0.75].into())),
            KeyValue::new("demo.label.team", "payments"),
            KeyValue::new("demo.tier", "pro"),
        ]
    );
    assert_eq!(RpcGrpcStatusCode::from_i64(1), RpcGrpcStatusCode::Cancelled);
}
