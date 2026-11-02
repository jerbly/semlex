---
name: semlex-instrument
description: Add or change OpenTelemetry instrumentation in a Rust codebase that uses semlex (registry-checked span!, record!, metric!, event!, entity! and log macros from an org semconv crate). Use when instrumenting a handler, consumer, client call or job, when a semlex macro fails to compile, or when asked to add telemetry tests.
---

# Instrument with semlex

semlex checks every call site against the semantic-convention registry at
compile time. Work from the registry, let the compiler tell you what's
missing, and prove the result with a telemetry test.

## 1. Find the signal

For the code you're instrumenting, decide what happens there: an incoming
request (server span + duration metric), an outgoing call (client span), a
message consumed or produced, a discrete event, a log.

Look it up with the `weaver` MCP server (`search` with a few words, then
`get_span` / `get_metric` / `get_event` / `get_attribute`), or in the registry
crate's `resolved.yaml`. Note:

- the signal id (`http.server.request`) and kind;
- **required** attributes, and which are **sampling-relevant** (spans);
- conditionally required attributes and their conditions (follow them);
- enum attributes' values;
- annotations: `sensitivity: pii` (wrap values), `cardinality: unbounded`
  (never on metrics).

If nothing fits, or an attribute you need isn't defined: **stop and propose a
registry change** (the YAML to add, with annotations). Never use
`unregistered(…)` or `extra(…)`.

## 2. Write the call sites

Import the registry crate's macros by name (not `tracing`'s log macros
alongside them): `crate::` when this crate calls `registry!`, or the
workspace `*-semconv` crate:

```rust
use crate::{metric, record, span};

let span = span!(http.server.request {
    http.request.method: method,          // sampling-relevant: at start
    url.scheme: "https",
});
// Required attributes not set yet: consume and rebind.
let span = record!(span, url.path: path);
// Everything else, in place, as soon as it's known (also in closures):
record!(&span, http.route: route);
call_dependency(&span.child_context());
record!(&span, http.response.status_code?: status);   // an Option: only if Some
span.end();

metric!(http.server.request.duration: elapsed, {
    http.request.method: method,
    http.route: route,
});
```

- Use the registry's dotted keys and wire values exactly.
- Enum values: a literal (`"GET"`), the generated enum, a converted type
  (if the registry crate declares `conversions`), or `custom(value)` for values
  outside the registry. A runtime `&str` isn't accepted.
- `record!(span, …)` (rebind) for a required attribute that isn't set yet;
  `record!(&span, …)` for the rest. Optional values: `key?: option`.
- Helpers take `impl semlex::StartedSpan` (the span's type can't be named);
  they can't end it successfully or record checked attributes on it.
- Error paths: record what's known (e.g. the status code), then
  `end!(span, error: &e, error.type: class)` (or `result: &r, error.type: |e| class`),
  which sets `error.type`, the log-based `exception` event and status, and checks the
  required attributes. A function that shares span setup returns
  `spans::…::Ready`.
- A required attribute a path can't know: don't invent a value; skip the
  signal there (say so in a comment) or propose a registry change.
- Logs: `crate::warn!(key: value, …, "message")`, called by path.
- Errors and events are log records: they need the application's logger
  provider in `semlex::init`, or its `tracing` bridge. In a library, don't
  call `init`; make sure the README tells users what to set up (guide,
  "Where telemetry goes").

## 3. Compile and fix

Run `cargo check`. Each semlex error names the attribute, the signal and the
fix; apply it. Common ones:

| Error says | Do |
|---|---|
| missing required attribute `x` … | set `x` where the note says |
| missing sampling-relevant attribute / can't be recorded after start | move it into `span!` |
| `x` is not an attribute of … | use an attribute the signal defines (look it up) |
| not a valid value here: this attribute takes a `K` | convert the value (the note lists accepted types) |
| unknown value … did you mean "…"? | fix the enum literal |
| is development; enable the `semlex-unstable` feature | ask before enabling non-stable items |
| takes a `Pii<…>` | wrap with `redacted(…)` or `hashed(…)` |

Don't silence errors with `otel_mut()`, `into_otel()`, raw OpenTelemetry
calls or `#[allow(…)]`.

## 4. Prove it with a test

```rust
use crate::{assert_metric, assert_span};   // in a #[cfg(test)] module

#[test]
fn request_is_instrumented() {
    let t = semlex::testing::Telemetry::init();
    my_service::handle_request(/* … */);
    assert_span!(t, http.server.request { http.route: "/users/{id}" });
    assert_metric!(t, http.server.request.duration { http.route: "/users/{id}" });
}
```

`semlex` needs its `testing` feature in dev-dependencies. Assert the
attributes your change is about, with values unique to the test (tests share
the exporters, so another test's signal could match). A failure lists what was emitted; fix the
code, not the assertion, unless the assertion was wrong.

## 5. Check

- `cargo test` passes; `cargo clippy -- -D warnings` passes.
- No `unregistered(`, `extra(`, `otel_mut(`, `into_otel(` added (`scripts/escape-report.sh` lists them).
- Any registry change you need is written up for review, not worked around.
