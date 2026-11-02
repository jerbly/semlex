//! Opt-in signals record nothing unless enabled in `semlex::Config`.

mod common;

#[test]
fn disabled_opt_in_metric_records_nothing() {
    let t = common::telemetry();
    semlex::init(t.config()).unwrap();
    fixture_service::record_cache_fill(0.9);
    fixture_service::record_request("/", std::time::Duration::from_millis(5));
    let exported = t.exported();
    assert!(common::find(&exported, "demo.cache.fill_ratio").is_none());
    assert!(
        common::find(&exported, "http.server.request.duration").is_some(),
        "recommended metrics still record"
    );
}
