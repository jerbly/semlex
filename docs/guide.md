# semlex guide

How to use semlex: setting up a local registry, the macros, testing, and what each
compile error means.

- [Cheat sheet](#cheat-sheet)
- [Setup](#setup)
- [Where telemetry goes](#where-telemetry-goes): logger provider, `tracing` bridge, exceptions, and what application and library authors should do
- [Attribute values](#attribute-values)
- [`attrs!`](#attrs)
- [`metric!`](#metric)
- [`event!`](#event)
- [`span!` and `record!`](#span-and-record)
- [`entity!` and the Resource](#entity-and-the-resource)
- [Logs](#logs)
- [`unregistered(…)` and `extra(…)`](#unregistered-and-extra)
- [Testing](#testing)
- [Compile errors](#compile-errors)

## Cheat sheet

Every call-site form on one page; the sections below explain each.

```rust
use crate::{attrs, end, entity, event, metric, record, span};   // or your `*-semconv` crate
use semlex::{Context, KeyValue};

// Spans: sampling-relevant attributes at start; options after the braces.
let span = span!(http.server.request { http.request.method: "GET", url.scheme: "https" });
let span = span!(db.client { db.system.name: "postgresql" }, parent: &cx, name: "SELECT users");

// Recording: rebind for a required attribute not yet set; in place for the rest.
let span = record!(span, url.path: path);
record!(&span, http.route: "/users/{id}");
record!(&span, http.response.status_code?: maybe_status);   // only if Some
record!(current, app.cart.size: n);                          // the current span

// Children, helpers and the current span.
fetch(&span.child_context());
span.in_scope(|| work());                                    // span is current inside
fn helper(span: &impl semlex::StartedSpan) { /* child_context, in_scope, set_status, … */ }
fn setup(cx: &Context) -> spans::http::server::request::Ready { /* … */ span.ready() }

// Ending.
span.end();                                                  // needs every required attribute
end!(span, error: &e, error.type: "timeout");                // error.type, exception, status
end!(span, result: &result, error.type: |e| e.class());      // end() on Ok

// Metrics and events.
metric!(http.server.request.duration: elapsed, { http.request.method: "GET" });
metric!(app.jobs.done: 1, { app.queue: &queue });
event!(session.start { session.id: &id });

// Attributes without a signal; entities for the Resource.
let kvs: [KeyValue; 2] = attrs![http.request.method: "GET", http.route: route];
let host = entity!(host { host.name: hostname });

// Logs: call by path (the names clash with `tracing`'s).
crate::warn!(http.route: route, "upstream timed out");

// Escape hatches, approved in the macro.
metric!(app.jobs.done: 1, { extra(error.type, reviewed: "until the metric has it"): "timeout" });
record!(&span, unregistered("app.experiment", reviewed: "experiment flag"): true);
```

| Value | Form |
|---|---|
| a string | `"lit"`, `&str`, `&String`, `String`, … |
| an enum | `"GET"` (checked), `HttpRequestMethod::Get`, `&method`, `custom("PURGE")` |
| PII (`sensitivity: pii`) | `redacted(v)` or `hashed(v)` |
| an `Option` | `key?: opt` (wrappers apply inside: `key?: redacted(opt)`) |
| a template attribute | `http.request.header("content-type"): vec![…]` |

## Setup

semlex runs over a **local registry**: one per service or library, defining
the signals that code emits. It pulls what it needs from upstream registries
(OpenTelemetry semconv, an organization's shared conventions) as Weaver
dependencies. The resolved file holds only what the local registry uses, so
it stays small however large the upstreams are. There's no single big
registry for everything.

### 1. The local registry

```yaml
# registry/manifest.yaml
file_format: definition_manifest/2.0
schema_url: https://example.com/schemas/checkout/1.0.0
stability: development
dependencies:
  - name: otel
    schema_url: https://opentelemetry.io/schemas/1.44.0
    registry_path: https://github.com/open-telemetry/semantic-conventions.git@v1.44.0[model]
```

```yaml
# registry/signals.yaml
file_format: definition/2
attributes:
  - key: checkout.cart.size
    type: int
    stability: development
    brief: Items in the cart.
spans:
  - type: checkout.request
    kind: server
    stability: development
    brief: A checkout request.
    name:
      templates: ['{http.request.method}']
    attributes:
      - ref: http.request.method      # from the upstream
        requirement_level: required
      - ref: checkout.cart.size       # local
```

A signal can `ref:` an upstream attribute without importing it; semlex takes
its definition from the resolved file's attribute catalog. An upstream can be
a git checkout, a published archive or a local path (`registry_path`), and a
local registry can have several, e.g. OpenTelemetry semconv and an
organization's shared registry.

### 2. Package it

```sh
weaver registry package --v2 -r registry -o . \
    --resolved-registry-uri https://example.com/schemas/checkout/1.0.0/resolved.yaml
```

This writes `resolved.yaml` (Weaver's `resolved/2.0`) and a publication
`manifest.yaml`. Commit both, and in CI re-package and fail on a diff
(`git diff --exit-code`): `registry!` compiles against whatever the file
holds, so a registry edit without re-packaging leaves the code checked
against a stale model, silently.

**Taking upstream changes:** bump the dependency (the tag in
`registry_path`, and its `schema_url`), re-package, and review the diff of
`resolved.yaml`. Changes that affect the code are compile errors or warnings
where the code is: a renamed attribute becomes a deprecation warning at each
use, and a removed one an error.

### 3. Expand it

`registry!` goes at the root of the crate that uses the registry:

```rust
// src/lib.rs (or main.rs), before the `mod` declarations
semlex::registry!("resolved.yaml", {
    scope_prefix: "acme.",   // instrumentation scope names: `acme.<crate>`
    visibility: crate,       // macros and modules stay private to this crate
    allow_unstable: true,    // its own development signals, without a feature
    annotations: { acme.case: crate::annotations::case, acme.owner: ignore },
});
```

```toml
[dependencies]
semlex = "…"

[dev-dependencies]
semlex = { version = "…", features = ["testing"] }
```

Call the macros as `crate::span!`, or import them: `use crate::{record,
span};`. The crate's own names win over the macros' short names: with `use
tracing::warn;` at the crate root, `warn!` is `tracing`'s, and semlex's is
`crate::__semlex_macros::warn!`.

**Build cost:** `registry!` re-expands whenever its crate is rebuilt, so
every edit pays for it. A library with 74 attributes: about 0.5 s
and 173 MB per incremental `cargo check`, against 0.23 s and 135 MB without
semlex. Add this to the workspace's root `Cargo.toml`; it roughly halves the
time `registry!` takes:

```toml
[profile.dev.build-override]
opt-level = 3
```

#### Option: a workspace `*-semconv` crate

When several crates in one workspace share a local registry, or the registry
is large enough that re-expanding it on every edit is slow, put `registry!`
in a small crate of its own, which then re-expands only when the registry
changes.

```toml
# checkout-semconv/Cargo.toml
[dependencies]
semlex = "…"

[features]
# Allows non-stable registry items at call sites.
semlex-unstable = []
```

```rust
// checkout-semconv/src/lib.rs
semlex::registry!("resolved.yaml", { scope_prefix: "acme." });
```

The crates that use it depend on it (and on `semlex`, for `init` and
testing), and import the macros by name: `use checkout_semconv::{record,
span};`. With the default `visibility` (`public`), the semconv crate itself
must use bare names (`span!`) after the invocation: rustc rejects
`crate::span!` for exported macros that a macro defined in the same crate
(rust-lang/rust#52234).

#### `registry!` options

| Option | Meaning |
|---|---|
| `stability` | `all` (default), `stable_only`, or a minimum level. Non-stable items need the `semlex-unstable` feature of the crate that calls `registry!`, unless `allow_unstable` is set. Cargo unifies features, so one crate enabling it enables it for the whole build. |
| `allow_unstable` | `true`: non-stable items are usable without the feature. For a crate that uses its own registry, where its own development signals are the point. |
| `visibility` | `public` (default): the generated modules and macros are exported, for a workspace `*-semconv` crate. `crate`: visible in this crate only (`pub(crate)`), for a crate that uses its own registry, so they stay out of its public API. |
| `scope_prefix` | Prefix for call sites' instrumentation scope names (the calling crate's name follows). |
| `annotations` | Every annotation namespace not built in must be declared: a `#[semlex::handler]` module or `ignore`. See [annotations.md](annotations.md). |
| `conversions` | External types an enum attribute accepts (`From<T>` and `&T`), e.g. `{ http.request.method: ::http::Method }`. |
| `groups` | Only emit these signals and attribute groups, e.g. `["http.*"]`. |

Instrumented code also uses `semlex`'s re-exports of `Context` and
`KeyValue`, so it doesn't need an `opentelemetry` dependency for them:
`use semlex::{Context, KeyValue};`. The examples below import the macros by
name (`use …::{attrs, entity, event, metric, record, span};`). The log macros
are the exception: call them by path (see [Logs](#logs)).

### 4. Initialize at startup

```rust
semlex::init(semlex::Config::new()
    .with_meter_provider(meter_provider)
    .with_tracer_provider(tracer_provider)
    .with_logger_provider(logger_provider)       // for `event!` and exceptions
    .with_resource(resource_attributes.clone())  // entity association checks
    .enable("acme.cache.fill_ratio"))?;          // opt-in signals, by name
```

`init` succeeds once per process. Call it in the application, never in a
library. A provider left out of `init` works as in library mode: spans or
metrics follow the global provider (below), and events and exceptions go out
as `tracing` events.

### Libraries

A library instrumented with semlex doesn't call `init`. Its spans and metrics
go through OpenTelemetry's global providers, whichever ones the application
has installed when each signal is recorded, whether it installs them itself
(`opentelemetry::global::set_tracer_provider`, `set_meter_provider`) or
through `semlex::init`. Signals recorded before the application installs
providers go to the no-op defaults and are lost, but call sites don't get
stuck there.

Events and exceptions are different, because OpenTelemetry Rust has no global
logger provider. How they reach the application, and what to tell your users,
is in [Where telemetry goes](#where-telemetry-goes).

A library usually has its own local registry and expands it with
`visibility: crate` ([Setup](#3-expand-it)), so its instrumentation stays
out of its public API.

**Testing a library:** a library's own test binary is an application, so
`semlex::testing::Telemetry::init()` (which calls `semlex::init`) is right
there.

## Where telemetry goes

semlex records through the OpenTelemetry API. Where each record goes depends
on what the *application* sets up, because OpenTelemetry Rust has global
tracer and meter providers but **no global logger provider**.

| Record | Goes through |
|---|---|
| Spans (`span!`) | the global tracer provider |
| Metrics (`metric!`) | the global meter provider |
| Events (`event!`) and exceptions (`end!`, `end_with_error`, `record_error`) | the logger provider given to `semlex::init`; without one, a `tracing` event for the application's `tracing` bridge |
| Log macros (`info!`, `warn!` …) | `tracing`, so the application's `tracing` bridge |

The global providers are installed by `semlex::init(…with_tracer_provider(…))`
or by the application itself (`opentelemetry::global::set_tracer_provider`).
The `tracing` bridge is `opentelemetry-appender-tracing`'s
`OpenTelemetryTracingBridge`: a `tracing` layer that turns `tracing` events
into OpenTelemetry log records.

### What the application's setup means for events and exceptions

| Application sets up | `event!` and exceptions | Log macros |
|---|---|---|
| `semlex::init` with a logger provider (and the bridge) | **exact records**, through semlex's logger | through the bridge |
| the bridge only | through the bridge, with the differences below | through the bridge |
| neither | **lost**, silently | not exported (other `tracing` layers still see them) |

With a logger provider, semlex never sends events or exceptions through
`tracing` as well, so nothing is recorded twice.

A record that goes through the bridge has the same `event_name`, attributes
and trace context as one sent through semlex's logger. These differ:

| | Through semlex's logger | Through the `tracing` bridge |
|---|---|---|
| Instrumentation scope | `<scope_prefix><crate>`, with its version and the registry's schema URL | the bridge's own scope: no crate, version or schema URL |
| Severity | `event!`'s `severity:`, or none; `ERROR` for exceptions | `event!`'s `severity:`, or `INFO`; `ERROR` for exceptions |
| `target` | not set | the emitting module (`semlex::log` for exceptions) |
| Array and `any` attributes | recorded | left out: `tracing` can't carry them |
| Template and `unregistered(…)` attributes | recorded | left out of `event!`: their keys or types aren't fixed at compile time |
| Delivery | always | only through the application's `tracing` filters; other layers (e.g. a console formatter) see them too |

Live check matches both by `event_name` and attributes. Only the logger's
records tell a backend which library and semantic-conventions version they
came from.

### Exceptions

Span events are deprecated in OpenTelemetry, so semlex records exceptions as
the conventions' log-based `exception` event, in the span's trace context:

| Field | Value |
|---|---|
| `event_name` | `exception` |
| severity | `ERROR` |
| `exception.message` | the error's `Display` |
| `exception.type` | the error's Rust type name (from `end!`) |
| `exception.stacktrace` | the error's source chain, when it has one: `"{error}\n\nCaused by:\n    0: {source}…"` |

The span itself gets status `Error` with the error's message as the
description, from `end!`, `end_with_error` and `record_error` alike. `end!`
also records `error.type` on the span. So even when the `exception` record is
lost (neither a logger provider nor a bridge), the span still says that the
operation failed and how. Error messages reach both the record and the span
status, so keep secrets and personal data out of `Display` for errors that
are recorded.

### For application authors

Do this once, at startup, in the binary. It applies whether or not your own
code uses semlex: a dependency instrumented with semlex needs it too.

1. **Install your providers globally**, through `semlex::init` or with
   `opentelemetry::global::set_*`.
2. **Give semlex your logger provider**, so events and exceptions are exact
   records. If you don't otherwise use semlex, this is one line, and spans
   and metrics keep using your global providers:

   ```rust
   semlex::init(semlex::Config::new().with_logger_provider(logger_provider.clone()))?;
   ```
3. **Install the `tracing` bridge**, for the log macros (and anything else that
   logs through `tracing`). Filter out OpenTelemetry's own diagnostics, which
   would otherwise feed back into the bridge:

   ```rust
   use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
   use tracing_subscriber::{filter::filter_fn, prelude::*};

   let bridge = OpenTelemetryTracingBridge::new(&logger_provider)
       .with_filter(filter_fn(|meta| !meta.target().starts_with("opentelemetry")));
   tracing_subscriber::registry().with(bridge).init();
   ```
4. **Let the records through your filters**: `INFO` for library-mode events,
   `ERROR` for exceptions.
5. **Shut the providers down** before exiting, so batched records are
   exported.

`examples/showcase/src/main.rs` does all of this with stdout exporters.

### For library authors

1. **Never call `semlex::init`** or install global providers: that's the
   application's job (see [Libraries](#libraries)).
2. **Record errors with `end!`, `end_with_error` or `record_error`**, and
   events with `event!`. Don't use span events: `add_event` is deprecated.
3. **Tell your users what to set up.** Something like this in your README:

   > This crate records OpenTelemetry spans, metrics and events with
   > [semlex](https://github.com/jerbly/semlex). Spans and metrics use your
   > global tracer and meter providers. For exact event and exception records,
   > pass your logger provider to semlex at startup:
   > `semlex::init(semlex::Config::new().with_logger_provider(p))`.
   > Otherwise they reach OpenTelemetry only through a `tracing` bridge
   > (`opentelemetry-appender-tracing`).
4. **Keep event attributes `tracing`-friendly where you can.** Array, `any`,
   template and `unregistered(…)` attributes are left out when an application
   relies on the bridge.
5. **Test with `semlex::testing::Telemetry::init()`.** It installs a logger
   provider, so tests see exact records. To test the bridge path too, install
   the bridge without calling `init`, in its own test binary
   (`tests/fixture-service/tests/library_mode_logs.rs`).

## Attribute values

Keys are the registry's dotted names. Values are checked against the
attribute's type:

| Attribute type | Accepts |
|---|---|
| `string` | `&str`, `&String`, `String`, `Cow<'static, str>`, `Arc<str>` (borrowed strings are copied; string literals aren't) |
| `int` | any integer type up to 64 bits; `u64` and `usize` saturate at `i64::MAX` (OpenTelemetry ints are `i64`) |
| `double` | `f64`, `f32` |
| `boolean` | `bool` |
| arrays | `Vec<…>` of the above |
| enum | the generated enum (`enums::HttpRequestMethod::Get`, or `&` one to reuse it), a **literal** (`"GET"`, checked at compile time), or `custom(value)` for values outside the registry |
| `sensitivity: pii` | only `redacted(value)` or `hashed(value)`; `hashed` needs `semlex`'s `pii` feature (on by default; turn it off to drop `hmac` and `sha2`) |
| `code_generation.rust.type: T` | a `T` (e.g. `Duration`, `IpAddr`) |
| `template[T]` | `key.prefix("suffix"): value`, e.g. `http.request.header("content-type"): vec![…]` |

A runtime `&str` isn't accepted for an enum (it couldn't be checked): pass the
enum, a literal, or `custom(…)`. The exception is an enum whose only member is
`_OTHER` (like OpenTelemetry's `error.type`): there's nothing to check against,
so it takes plain strings.

**Number literals** are `i64` or `f64`, the types OpenTelemetry stores,
whatever their suffix: `3_000_000_000` and `1usize` both work for an `int`
attribute, and `5` for a string attribute gets the usual "is not a valid value
here" message (saying `i64`). Literals also get compile-time checks where
there's something to check: enum members (with "did you mean") and
annotation handlers. An attribute with a handler takes a literal of its own
kind only (a string for strings, a number for ints).

Registry annotation handlers (e.g. a `case: upper` rule) check **literals** at
compile time and may **transform** every value at run time: a runtime value
is normalized, not rejected. [annotations.md](annotations.md) has the hooks.

`cardinality: unbounded` restricts only metrics; unbounded attributes are
fine on spans, events and logs.

## `attrs!`

```rust
let kvs: [KeyValue; 2] = attrs![http.request.method: "GET", http.route: route];
```

Registry-wide checks only (no signal), for hand-written OpenTelemetry calls.

## `metric!`

```rust
metric!(http.server.request.duration: elapsed, {
    http.request.method: "GET",
    http.route: route,
});
```

- Required attributes must be given. Only attributes the registry defines on
  this metric are allowed, as on every signal; anything else needs a registry
  change, or an approved `extra(…)` or `unregistered(…)`
  ([below](#unregistered-and-extra)).
- The measurement type follows the instrument and
  `code_generation.metric_value_type`: `double` metrics take `f64`/`f32`
  (and `Duration` for unit `s`); `int` counters and histograms take unsigned
  integers (`usize` included); `int` up-down counters and gauges take any
  integer up to 64 bits (`u64`/`usize` saturate at `i64::MAX`). A
  non-negative literal (`1`) fits any `int` instrument, and `double` ones.
- `cardinality: unbounded` attributes are compile errors, including through
  `extra(…)`. `unregistered(…)` is allowed; approve it only for values with
  bounded cardinality.

## `event!`

```rust
event!(session.start { session.id: id });
event!(session.start { session.id: id }, severity: warn);
```

A log record with `event_name`, in the current trace context. `severity:`
sets the record's severity: `trace`, `debug`, `info`, `warn`, `error` or
`fatal`. Without it the record has none. It goes through
the logger provider given to `init`, or through the application's `tracing`
bridge when there isn't one: see [Where telemetry goes](#where-telemetry-goes).
Event refinements can't be emitted yet (Weaver doesn't record their base
event).

## `span!` and `record!`

```rust
let span = span!(http.server.request {
    http.request.method: "GET",          // sampling-relevant: required here
    url.scheme: "https",
});
let span = record!(span, url.path: path);   // required: consumes and returns
record!(&span, http.route: route);           // not required: in place
load_user(&span.child_context());
record!(&span, http.response.status_code?: status);   // an Option: only if Some
span.end();                                // every required attribute must be set
```

**Two forms of `record!`.** A span's type tracks which required attributes
are set, so recording one that isn't set yet consumes the span and returns it
in its new state: `let span = record!(span, …)`. Every other attribute
(not required, or required and already set) can be recorded in place with
`record!(&span, …)`, which keeps the membership check. Use it in closures,
and to record values as soon as they're known: an in-place value is on the
span at once, so it survives a dropped span (e.g. a cancelled future). Using
the in-place form for a required attribute that isn't set yet is a compile
error that says to rebind.

**Optional values.** `key?: option` in `span!`, `record!`, `metric!`,
`event!` and `entity!` records the attribute only if the value is `Some`.
Wrappers apply to the value inside: `user.id?: redacted(maybe_id)`. An option
can't set a required attribute that isn't set yet (a compile error); give it
a value. Not in `attrs!`, `record!(current, …)` or the log macros: unwrap
first.

**Helpers.** A span's type spells out which required attributes are set.
Each span type has a **`Ready`** alias for its started state with every
required attribute set (`spans::http::server::request::Ready`). A function can
return one to share a span's setup, or take `&Ready` to record on it with the
full membership check; callers `end()` it as usual:

```rust
fn get_object_span(cx: &Context, bucket: &str) -> spans::aws::s3::get_object::Ready {
    let span = span!(aws.s3.get_object { … }, parent: cx);
    let span = record!(span, aws.s3.bucket: bucket);
    span.ready()   // names any required attribute still missing
}
```

Return through `ready()`: it has `end()`'s checks, so a missing attribute is
reported by name. Returning the span directly would only report a type
mismatch between two `Builder<Set, Unset, …>` types.

A helper that records on a span it's lent takes `&Ready` and uses the same
spelling as the caller. `record!(&span, …)` finds the span through any number
of references:

```rust
fn record_object_facts(span: &spans::aws::s3::get_object::Ready, etag: &str, size: u64) {
    record!(&span, aws.s3.etag: etag, file.size: size);
}
```

A helper for any span (any type, any state) takes `impl semlex::StartedSpan`:
`child_context`, `in_scope`, `set_status`, `record_error` and
`end_with_error`. It can't end the span successfully or record checked
attributes on it.

**The current span.** `span.in_scope(|| …)` runs a closure with the span as
the current span, so code that only sees the ambient context records on it
(`record!(current, …)`) and starts its children. It returns the closure's
value. In async code, attach the context to the future instead, since a
guard held across `.await` leaks to other tasks:
`fut.with_context(span.child_context())` (`opentelemetry::trace::FutureExt`).

**Ending with an error, following the conventions.** `end!` records
`error.type` on the span (membership-checked: the span must define it), emits
the `exception` event, sets the status to `Error`, then calls the checked
`end()`, so error paths need the required attributes too:

```rust
end!(span, error: &e, error.type: e.class());          // on an error
end!(span, result: &result, error.type: |e| e.class()); // end() on Ok, as above on Err
```

`error:` takes a reference to the error, or to a handle that derefs to
`dyn Error` (`&Box<dyn Error + Send + Sync>`, `&anyhow::Error`). The
`exception.type` attribute is the error's Rust type name, so it's left out
for a handle, whose concrete type isn't known. `result:` takes the result by
value or by reference.

The class is the low-cardinality `error.type` value. `span.end_with_error(&e)`
is the unchecked alternative: it doesn't check the required attributes or set
`error.type`. `span.record_error(&e)` records an error without ending the
span.

All three set the same status: `Error`, with the error's message (its
`Display`) as the description. The conventions say the description SHOULD be
the exception message, and advise against repeating `error.type` there.
Keep sensitive details out of error messages that reach telemetry.
`record_error` marks the span as failed while it stays open; if the
operation then recovers (a retry succeeds), set `Status::Ok`, which
overrides `Error`.

**Exceptions are log-based events** (span events are deprecated in
OpenTelemetry): all three emit the `exception` record in the span's trace
context. What it contains and where it goes:
[Exceptions](#exceptions).

- **Sampling-relevant** required attributes must be in `span!` (samplers see
  only those); recording one later is a compile error.
- **Required** attributes must be set before `end()`; each missing one is its
  own compile error. `end_with_error(&e)` doesn't check them.
- **Names** come from the registry's templates, re-evaluated at `end()` when a
  templated attribute arrives later. A span whose name the registry gives in
  prose needs `name: "…"` in `span!`.
- **Options** after the attributes: `parent: &cx`, `links: …`,
  `start_time: t`, `name: …`. The kind comes from the registry.
- **Error paths:** set what's known (e.g. the status code, if a response is
  sent), then `end!(span, error: &e, error.type: class)` (above).
  `end_with_error(&e)` skips the required-attribute check, for spans that
  can't have them.
- **Passing spans around:** keep the span in the function that started it and
  pass `span.child_context()` to callees, or `impl StartedSpan` to helpers.
- Also: `in_scope(f)`, `set_status`, `record_error`, `end_at(t)`,
  `record!(current, key: value)` on the current span (registry-wide checks),
  `otel_mut()` (borrow the raw span) and `into_otel()` (leave semlex).
  `add_event` (a span event) is deprecated: emit a log-based event with
  `event!` instead.

## `entity!` and the Resource

```rust
let service = entity!(service {
    service.name: "router",
    service.namespace: "acme",
    service.instance.id: id,
});
let host = entity!(host { host.name: hostname });
let attributes: Vec<KeyValue> = service.into_iter().chain(host).collect();
let resource = Resource::builder().with_attributes(attributes.clone()).build();
```

Identity attributes are required. Pass the same attributes to
`Config::with_resource`; each signal type's entity associations are then
checked when it's first used (debug builds warn), and
`semlex::check_associations()` checks everything used so far.

## Logs

```rust
crate::warn!(http.route: route, http.response.status_code: 504, "upstream timed out");
crate::error!(group exception { exception.type: "io", exception.message: msg }, "failed");
```

`tracing` events with registry keys as field names. `group X { … }` checks a
public attribute group's membership and requirements. Arrays, `any` and
template attributes can't be logged through `tracing`; use `event!`.

Log records have no `event_name`: they aren't registry events (`event!` emits
those). The macros pass `tracing` an empty name, because
`opentelemetry-appender-tracing` otherwise copies the callsite name
(`event <file>:<line>`) into `event_name`.

**Call these by path** (`crate::warn!`, or `checkout_semconv::warn!` from a
workspace crate) rather than importing them: `warn!`, `error!` and the rest
share names with `tracing`'s macros, so importing both (or glob-importing
both crates) is ambiguous.

## `unregistered(…)` and `extra(…)`

Two deliberately ugly escape hatches for attributes a signal's registry
definition doesn't allow. Prefer changing the registry.

| | For | Checks |
|---|---|---|
| `unregistered("retry.attempt"): n` | a key that isn't in the registry | none: untyped |
| `extra(error.type): "timeout"` | a registry attribute this signal doesn't define | the attribute's type, enum values, PII and annotation handlers, stability; on metrics, `cardinality: unbounded` |

Both warn at the key in every build, and clippy rejects them in CI unless
approved. Approve a use in the macro, with the reason:

```rust
let span = record!(span, unregistered("retry.attempt", reviewed: "until the registry has it"): n);

metric!(http.server.request.duration: elapsed, {
    http.request.method: "GET",
    http.route: route,
    extra(error.type, reviewed: "timeouts by class, until the metric has it"): "timeout",
});
```

The reason must be a non-empty string literal. An approved use doesn't warn
or call the clippy marker. The older form still works:
`#[allow(deprecated, clippy::disallowed_methods)]` on a `let` or item (rustc
ignores attributes on a bare macro statement). Both work on spans, metrics and events, and are errors on
entities. `unregistered(…)` is an error for keys that are in the registry;
`extra(…)` is an error for an attribute the signal defines (write `key: …`;
`record!` reports it when the crate is built rather than by `cargo check`),
and where any registry attribute is already allowed (`attrs!`,
`record!(current, …)`, log macros). On metrics, approve only
values with bounded cardinality (an error reason, not an id).
`scripts/escape-report.sh` lists every use.

## Testing

Enable `semlex`'s `testing` feature in dev-dependencies, then assert that the
expected signals were emitted:

```rust
use crate::{assert_event, assert_metric, assert_span};

#[test]
fn request_is_instrumented() {
    let t = semlex::testing::Telemetry::init();   // once per process; calls semlex::init
    handle_request("/users/42");
    assert_span!(t, http.server.request { http.route: "/users/{id}" });
    assert_metric!(t, http.server.request.duration { http.request.method: "GET" });
    assert_event!(t, session.start { session.id: "s-1" });
}
```

With `visibility: crate`, the assertion macros are private to the crate like
the others, so telemetry tests go in `#[cfg(test)]` modules; integration
tests in `tests/` can use them from a workspace `*-semconv` crate.

Assertions check that a matching signal exists, and tests in one binary share
the exporters, so a signal from another test can satisfy an assertion. Use
values unique to each test (ids, paths) in the expected attributes. A failure
lists what was emitted, with span status.

`Telemetry::init` calls `semlex::init`, which succeeds once per process: in a
test binary, use it instead of your own `semlex::init` call, not alongside it.

### When a required attribute isn't available

Don't invent a value. If a signal requires an attribute a code path can't
know (a metric requiring `http.route` on a request no route matched), either
skip that signal on that path, or propose a registry change (make it
conditionally required) for review. Note the choice in a comment.

## Compile errors

| Error | Fix |
|---|---|
| ``cannot find value `rout` in module `__semlex_attr::http` `` (with a suggestion) | Misspelled key; use the suggested name, or look the attribute up in the registry. |
| `` `http_route` is not an attribute of metric `…` `` | The signal doesn't define it (types are keys with `.` as `_`). Use one it defines, or add it to the signal in the registry. |
| `` missing required attribute `x` on span `…` `` | Set `x` where the note says (`span!`, `record!` before `end()`, the `metric!`/`event!` attributes, `entity!`). |
| `` missing sampling-relevant attribute `x` on span `…` `` / `` `x` is sampling-relevant … can't be recorded after start `` | Move `x` into `span!`. |
| `` `i64` is not a valid value here: this attribute takes a `StringValue` `` | Wrong value type; see [Attribute values](#attribute-values). |
| `` this attribute is required and not set yet: `key?: option` and `record!(&span, …)` can't set it `` | Give it a value: `key: value`, and on a started span `let span = record!(span, key: value)`. |
| `` unknown value for `http.request.method`; did you mean "GET"? `` | Enum literal typo; use the suggestion, or `custom("…")` for a value outside the registry. |
| `` attribute `x` is development; enable the `semlex-unstable` feature … `` | Non-stable item: enable the feature on the crate that calls `registry!` (or set `allow_unstable: true` there), or use a stable attribute. |
| `` value must be upper case (annotation `acme.case: upper`) `` | An annotation handler rejected the literal; the message says why. |
| `` `String` is not a valid value here: this attribute takes a `Pii<StringValue>` `` | PII attribute: wrap with `redacted(…)` or `hashed(…)`. |
| `` `url.path` is unbounded … can't be an attribute of metric `…` `` | Use a bounded attribute on metrics. |
| `` span `db.client` has no name template: pass `name: …` to `span!` `` | Add `name: "…"`. |
| `` use of deprecated constant …: renamed to `x` `` | The attribute was renamed in the registry; use `x`. |
| `` `extra(…)` is for attributes this signal doesn't define, and it defines this one `` | Drop the `extra(…)`: write `key: value`. |
| `` use of deprecated function …: `k` is a registry attribute this signal doesn't define `` | An `extra(…)` use; add `k` to the signal in the registry, or get the use approved. |
| `` use of deprecated function …: `k` is not in the semantic-convention registry `` | An `unregistered(…)` use; add `k` to the registry, or get the use approved. |
