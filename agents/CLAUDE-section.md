## Telemetry (semlex)

<!-- Replace `registry/`, `resolved.yaml` and `crate::` with your local
     registry, its packaged file and the crate that calls `registry!` (this
     crate, or a workspace `*-semconv` crate). -->

Telemetry is defined in this crate's local semantic-convention registry
(`registry/`, packaged into `resolved.yaml`), which pulls attributes from
upstream registries such as OpenTelemetry semconv, and recorded only through
the macros `registry!` generates (`crate::span!`, …). The compiler checks every call site against the
registry. See `docs/guide.md` in semlex for the macros and their errors.

- **Look signals up before instrumenting.** Use the `weaver` MCP server
  (`search`, `get_span`, `get_metric`, `get_event`, `get_entity`,
  `get_attribute`) or `resolved.yaml` to find the span, metric or event that
  fits, its required and sampling-relevant attributes, and enum values.
- **Use the registry's names exactly**, as dotted keys, and literal enum
  values as written on the wire (`http.request.method: "GET"`).
- **Treat compile errors from the macros as the spec.** Each names the
  attribute, the signal and the fix; apply the fix. Don't work around them
  with `otel_mut()`, `into_otel()` or raw OpenTelemetry calls.
- **Never use `unregistered(…)` or `extra(…)`.** If an attribute or signal is missing,
  stop and propose a change to the local registry (YAML, including
  annotations such as `cardinality` and `sensitivity`) for review instead:
  a `ref:` to an upstream attribute if one fits, otherwise a local
  definition. After a registry change, re-package (`weaver registry package`).
- **Spans:** put sampling-relevant attributes in `span!`; set the remaining
  required ones before `end()` with `let span = record!(span, …)` (rebind);
  record everything else in place, as soon as it's known, with
  `record!(&span, …)`; optional values with `key?: option`. Pass
  `span.child_context()` to callees, and `impl semlex::StartedSpan` to
  helpers.
- **Required but unavailable:** never invent a value for a required
  attribute; skip the signal on that path or propose a registry change.
- **PII:** attributes marked `sensitivity: pii` take `redacted(v)` or
  `hashed(v)`; never log the raw value elsewhere.
- **Logs:** call the log macros by path (`crate::warn!`), never
  glob-imported next to `tracing`'s.
- **Initialization:** in an application, `semlex::init(…)` runs once at
  startup, before any instrumented code, with the logger provider (for
  events and exceptions); install the `tracing` bridge for the log macros. A
  **library** never calls it: its spans and metrics use the application's
  global providers, and its events and exceptions need the application's
  logger provider passed to `init` or a `tracing` bridge. Say so in the
  library's README. See the guide's "Where telemetry goes". In tests,
  `semlex::testing::Telemetry::init()` does it.
- **Every instrumented behavior gets a telemetry test:** use
  `semlex::testing::Telemetry::init()` and `crate::assert_span!` /
  `assert_metric!` / `assert_event!` to prove the signal is emitted with the
  attributes the change is about. Compile checks prove instrumentation is
  well-formed; these prove it exists.
