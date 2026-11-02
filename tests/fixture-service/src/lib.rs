//! Example service: the demo registry's signals, recorded through the
//! `metric!` and `event!` macros.

use fixture_semconv::{entity, event, metric, record, span};
use semlex::{Context, KeyValue};
use std::time::Duration;

/// The Resource's attributes: this service instance on its host.
pub fn resource_attributes(instance_id: &str) -> Vec<KeyValue> {
    let service = entity!(service {
        service.name: "fixture-service",
        service.namespace: "demo",
        service.instance.id: instance_id.to_string(),
        service.version: env!("CARGO_PKG_VERSION"),
    });
    let host = entity!(host { host.name: "web-1" });
    service.into_iter().chain(host).collect()
}

/// Handles an HTTP request: a server span whose name gains the route when
/// it's matched, and a child database span.
pub fn handle_request(path: &str, route: &str, status: u16) {
    // Sampling-relevant attributes go in `span!`, so samplers see them.
    let span = span!(http.server.request {
        http.request.method: "GET",
        url.scheme: "https",
        url.path: path.to_string(),
    });
    // Routing happens after the span starts; the name is re-evaluated at
    // `end()` from `{http.request.method}` to `{http.request.method} {http.route}`.
    let span = record!(span, http.route: route.to_string(), demo.tenant.code: "ACME");
    // An approved escape hatch: the attribute isn't in the registry yet.
    #[allow(deprecated, clippy::disallowed_methods)]
    // reviewed: retry counts are being added to the registry
    let span = record!(span, unregistered("demo.retry.attempt"): 0);
    load_user(&span.child_context());
    let span = record!(span, http.response.status_code: status);
    span.end();
}

/// A database call, as a child of `parent`.
fn load_user(parent: &Context) {
    // `db.client` names spans in prose, so the call site gives the name.
    let span =
        span!(db.client { db.system.name: "postgresql" }, parent: parent, name: "SELECT users");
    let span = record!(span, db.namespace: "app", db.operation.name: "SELECT");
    span.end();
}

/// Records one HTTP request's duration.
pub fn record_request(route: &str, elapsed: Duration) {
    metric!(http.server.request.duration: elapsed, {
        http.request.method: "GET",
        http.route: route.to_string(),
        url.scheme: "https",
    });
}

/// Records a failed request's duration, with why it failed. The reason isn't
/// in the registry yet; approved as an `unregistered(…)` attribute.
pub fn record_failed_request(route: &str, elapsed: Duration, reason: &'static str) {
    #[allow(deprecated, clippy::disallowed_methods)]
    // reviewed: until `demo.error.reason` is registered
    let () = metric!(http.server.request.duration: elapsed, {
        http.request.method: "GET",
        http.route: route.to_string(),
        unregistered("demo.error.reason"): reason,
    });
}

/// Emits a session start event.
pub fn start_session(id: String) {
    event!(session.start { session.id: id });
}

/// Records the cache fill ratio (an opt-in metric).
pub fn record_cache_fill(ratio: f64) {
    metric!(demo.cache.fill_ratio: ratio, { demo.tier: "pro" });
}

/// Logs an upstream timeout: registry-checked fields on a `tracing` event.
/// The log macros are called by path, so they don't clash with `tracing`'s.
pub fn log_upstream_timeout(route: &str) {
    fixture_semconv::warn!(
        http.route: route.to_string(),
        http.response.status_code: 504,
        "upstream timed out after {}ms", 250
    );
}

/// Logs a failure with the `exception` attribute group.
pub fn log_failure(error: &std::io::Error) {
    fixture_semconv::error!(
        group exception {
            exception.type: "std::io::Error",
            exception.message: error.to_string(),
        },
        "request failed"
    );
}

/// Deletes a user: `DELETE /users/{id}` for a tenant.
///
/// Deliberately not instrumented: a target for trying the agent instructions
/// in `agents/`.
pub fn delete_user(path: &str, tenant: &str, user_id: &str) -> Result<u16, std::io::Error> {
    if !path.starts_with("/users/") {
        return Ok(404);
    }
    delete_user_row(tenant, user_id)?;
    Ok(204)
}

/// The database call behind `delete_user`.
fn delete_user_row(tenant: &str, user_id: &str) -> Result<(), std::io::Error> {
    if tenant.is_empty() || user_id.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no such user",
        ));
    }
    Ok(())
}
