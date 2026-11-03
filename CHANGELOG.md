# Changelog

## Unreleased

First public version. Not yet published to crates.io.

### Registry input
- `registry!("resolved.yaml", { … })` expands a Weaver V2 resolved registry (`weaver registry package --v2`, `resolved/2.0`) into Rust at compile time, with no generated files. The crate rebuilds when the file changes.
- A local registry can `ref:` attributes from upstream registries (e.g. OpenTelemetry semconv) through Weaver dependencies; their definitions come from the resolved attribute catalog.
- Options: `stability`, `allow_unstable`, `visibility` (`public` or `crate`), `scope_prefix`, `annotations`, `conversions` and `groups`.
- Built against Weaver 0.28 and OpenTelemetry Rust 0.33. MSRV 1.89.

### Call-site macros
- `span!` and `record!` with typestate spans: sampling-relevant attributes at start, required attributes before `end()`, names from the registry's templates. `record!(&span, …)` records in place; `key?: option` records only if `Some`.
- `end!(span, error: …)` and `end!(span, result: …)` follow the error conventions: `error.type`, a log-based `exception` event and status `Error`.
- `metric!`, `event!`, `entity!`, `attrs!` and the log macros (`info!`, `warn!` …).
- Every attribute is checked against the registry: membership, value types, enum values (with "did you mean"), requirement levels, stability, deprecations, `cardinality: unbounded` on metrics and `sensitivity: pii` (`redacted(…)` / `hashed(…)`).
- Escape hatches `unregistered("key")` and `extra(key)` warn in every build and fail clippy unless approved (`reviewed: "why"` in the macro); `scripts/escape-report.sh` lists them.

### Annotations and extension
- `#[semlex::handler]` modules for your own annotations: compile-time literal checks and run-time transforms.
- `semlex-expand`'s `AnnotationHook` API for custom `registry!` macros (`examples/advanced`).

### Runtime
- `semlex::init(Config)` with tracer, meter and logger providers, opt-in signals and Resource entities. Libraries don't call `init`: spans and metrics follow OpenTelemetry's global providers, and events and exceptions fall back to `tracing` events.
- Event and `exception` log records carry the event's `Timestamp`, not only the SDK's `ObservedTimestamp`.
- `semlex::testing` (feature `testing`): in-memory telemetry and `assert_span!` / `assert_metric!` / `assert_event!`.

### Docs and examples
- The guide, annotations and custom-macro docs, agent instructions and skills (`agents/`), and three examples: `simple`, `showcase` and `advanced`.
