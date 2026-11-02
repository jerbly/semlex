//! A metric recorded before any provider is installed goes to the no-op
//! global and is lost, without poisoning the call site: the next one, after
//! `semlex::init`, is exported.

mod common;

use std::time::Duration;

#[test]
fn call_site_recovers_after_init() {
    // Before init: only the no-op global provider exists.
    fixture_service::record_request("/before", Duration::from_secs(1));

    let t = common::telemetry();
    semlex::init(t.config()).unwrap();
    assert_eq!(
        semlex::init(semlex::Config::new()),
        Err(semlex::AlreadyInitialized)
    );

    fixture_service::record_request("/after", Duration::from_secs(2));

    let exported = t.exported();
    let (m, ..) =
        common::find(&exported, "http.server.request.duration").expect("exported after init");
    let (count, sum, attrs) = common::f64_histogram(m);
    assert_eq!((count, sum), (1, 2.0), "only the measurement after init");
    assert!(attrs.contains(&("http.route".to_string(), "/after".to_string())));
}
