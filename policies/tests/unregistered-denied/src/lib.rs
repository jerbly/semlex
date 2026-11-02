//! Unapproved escape hatches.

/// Records a retry count that isn't in the registry, without approval.
pub fn unapproved() -> usize {
    fixture_semconv::attrs![unregistered("demo.retry.attempt"): 1].len()
}

/// Records a status code on a metric that doesn't define it, without approval.
pub fn unapproved_extra() {
    fixture_semconv::metric!(http.server.request.count: 1, {
        http.request.method: "GET",
        extra(http.response.status_code): 503,
    });
}
