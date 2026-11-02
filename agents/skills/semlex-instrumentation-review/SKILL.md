---
name: semlex-instrumentation-review
description: Review a change in a semlex-instrumented Rust codebase for missing or weakened telemetry - new request handlers, message consumers, outgoing calls or jobs without their registry signals, missing telemetry tests, and escape hatches (unregistered, otel_mut, into_otel, semlex-unstable). Use when reviewing a PR or diff that touches service code.
---

# Review for missing telemetry

The compiler proves instrumentation is well-formed and live check proves what
reaches the wire, but neither notices telemetry that was never written. This
review looks for absence.

## 1. Collect the change

Get the diff (e.g. `git diff <base>...HEAD`). List what it adds or changes:

- **entry points:** HTTP/gRPC handlers and routes, message consumers, scheduled jobs;
- **outgoing calls:** HTTP/gRPC clients, database queries, cache and queue operations;
- **error paths** in those;
- **instrumentation:** the registry crate's `span!`, `record!`, `metric!`, `event!`, `entity!` and log macros (imported by name or called by path);
- **tests:** `assert_span!`, `assert_metric!`, `assert_event!`.

## 2. Check each entry point and outgoing call

For each, find the signal the registry defines for it (the `weaver` MCP
server's `search`, or `resolved.yaml`): e.g. a new route →
`http.server.request` span and `http.server.request.duration` metric; a new
query → `db.client` span.

Flag:

- **No instrumentation** where the registry defines a signal.
- **Missing conditionally required attributes** whose condition holds here
  (e.g. `http.route` once the route is matched); the compiler can't check these.
- **Error paths** that `end()` normally instead of `end_with_error(&e)`.
- **Context not propagated**: a callee starts a span without
  `parent: &span.child_context()` (or an attached context), breaking the trace.
- **Changed behavior without a test**: new or changed instrumentation with no
  `assert_span!` / `assert_metric!` / `assert_event!` covering it.

## 3. Escape hatches

Run `scripts/escape-report.sh` (or grep the diff) and flag every new:

- `unregistered(…)`: should be a registry change; an approval
  (`reviewed: "why"` in the macro, or `#[allow(deprecated,
  clippy::disallowed_methods)]`) needs a real reason; on a
  metric, also check its values are bounded (an error reason, not an id);
- `extra(…)`: a registry attribute on a signal that doesn't define it;
  should be a change to the signal in the registry; approvals as above;
- `otel_mut()` / `into_otel()`: raw OpenTelemetry access, bypassing checks;
- `features = ["semlex-unstable"]`: non-stable registry items enabled;
- raw `opentelemetry` / `tracing` calls recording telemetry outside the macros.

## 4. Report

For each finding: the location, what's missing, the registry signal or
attribute that should be there, and the fix (code, test, or registry
change). Say explicitly when you found nothing in a category.
