//! A custom `registry!` macro with an annotation hook (docs/custom-macros.md).
//!
//! `slo-macros` is `semlex::registry!` plus a hook for the structured
//! `acme.slo: { objective, latency }` annotation in this crate's local
//! registry (`registry/`, packaged with `just example-registries`). It
//! expands everything `semlex::registry!` does, plus `slo::…` items per
//! annotated metric. The call-site macros work as usual.

slo_macros::registry!("resolved.yaml", { scope_prefix: "acme.", visibility: crate });

use std::time::Duration;

/// Records a request's duration and returns whether it met the SLO.
pub fn record_request(route: &str, elapsed: Duration) -> bool {
    crate::metric!(http.server.request.duration: elapsed, {
        http.request.method: "GET",
        http.route: route,
    });
    slo::http::server::request::duration::within(elapsed)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    #[test]
    fn slo_items_come_from_the_annotation() {
        use crate::slo::http::server::request::duration as slo;
        assert_eq!(slo::METRIC, "http.server.request.duration");
        assert_eq!(slo::OBJECTIVE, 0.99);
        assert_eq!(slo::LATENCY, Duration::from_millis(500));
        assert!(slo::within(Duration::from_millis(120)));
        assert!(!slo::within(Duration::from_secs(1)));
        // The refinement inherits the annotation.
        assert_eq!(slo::api::METRIC, "http.server.request.duration");
    }

    #[test]
    fn the_registry_macros_still_work() {
        let t = semlex::testing::Telemetry::init();
        assert!(super::record_request(
            "/slo/{id}",
            Duration::from_millis(80)
        ));
        crate::assert_metric!(t, http.server.request.duration { http.route: "/slo/{id}" });
    }
}
