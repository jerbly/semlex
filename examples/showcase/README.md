# semlex showcase

A small online shop's checkout, instrumented with semlex. It uses every
feature and is runnable.

| Part | What it is |
|---|---|
| [`registry/`](registry) | The shop's local registry: its own `shop.*` attributes and its signals, with the `http.*`, `url.*`, `db.*`, `error.type`, `exception.*` and `service.*` attributes taken from OpenTelemetry semconv (the upstream in [`registry/manifest.yaml`](registry/manifest.yaml)). Packaged into `resolved.yaml`. |
| [`src/lib.rs`](src/lib.rs) | `semlex::registry!` with one annotation handler, and the checkout service instrumented with its macros |
| [`src/main.rs`](src/main.rs) | A runnable demo printing the telemetry |
| [`tests/`](tests) | Telemetry tests and [a tour of compile errors](tests/compile_errors) |

```sh
cargo run -p showcase                              # print the telemetry to stdout
cargo test -p showcase                             # telemetry tests and the compile-error tour
just example-registries                            # re-package after editing registry/
```

## Where each feature is

| Feature | Registry | Code |
|---|---|---|
| Spans, with sampling-relevant attributes at start | `http.server.request` | `checkout`: `span!` |
| Required attributes before `end()` | `url.path`, `shop.tenant.code` | `checkout`: `record!` |
| Span names from templates, re-evaluated at `end()` | `name.templates` | `POST` → `POST /cart/{id}/checkout` |
| Span names in prose (`name:` required) | `db.client` | `insert_order` |
| Span refinements | `span.shop.orders.insert` | `insert_order` |
| Child spans | | `child_context()` → `authorize_payment`, `insert_order` |
| Recording without consuming the span | | `record!(&span, http.route: …)` in `checkout` |
| Optional values | | `shop.customer.id?: redacted(customer_id)`, `error.type?: error` |
| Sharing a span's setup | | `order_span(…) -> spans::shop::orders::insert::Ready` |
| Helpers that take any started span; `in_scope` | | `note_cart(span: &impl StartedSpan, …)` |
| Error paths, following the conventions | `error.type` | `end!(span, error: &e, error.type: …)` in `checkout`, `authorize_payment`; `result:` form in `tests/end_macro.rs` |
| `record!(current, …)` | | `note_cart_size` |
| Histogram with a `Duration` | `http.server.request.duration` (unit `s`) | `record_duration` |
| Counter (unsigned), up-down counter (signed) | `shop.cart.items_sold`, `shop.checkout.active` | `checkout` |
| Opt-in signals | `shop.inventory.fill_ratio` (`requirement_level: opt_in`) | `report_inventory`; enabled in `main.rs` |
| Events, with arrays | `shop.order.placed`, `shop.cart.skus` (`string[]`) | `checkout`: `event!` |
| Logs, with an attribute group | `exception` (public group) | `checkout`: `warn!(… group exception { … })` |
| Entities and the Resource | `service`, `entity_associations` | `resource_attributes`; `main.rs` |
| Enums: generated type, literal, `custom(…)` | `http.request.method`, `shop.payment.method` | `PaymentMethod::Card`, `"card"`, `custom("PURGE")` |
| Enum type name | `code_generation.rust.enum_name: PaymentMethod` | `enums::PaymentMethod` |
| External type conversions | `conversions` in the registry crate | `http::Method` in `checkout` |
| Rust type override | `shop.payment.timeout` (`code_generation.rust.type`) | `Duration::from_secs(5)` |
| Template attributes | `http.request.header` | `http.request.header("content-type")` |
| PII | `sensitivity: pii` | `redacted(…)`, `hashed(…)` |
| Cardinality | `url.path` (`cardinality: unbounded`) | compile error 09 |
| Your own annotations | `shop.case: upper` | handler in the registry crate; normalizes the tenant |
| Stability | `crypto` member is `development` | compile error 11 |
| Deprecation | `shop.cart.size` renamed | compile error 12 |
| `extra(…)` (approved) | tenant on the duration metric | `record_duration` |
| `unregistered(…)` (approved) | not in the registry | `authorize_payment` |
| `attrs!` | | `link_attributes` |
| Telemetry tests | | [`tests/telemetry.rs`](tests/telemetry.rs) |

## The compile-error tour

[`tests/compile_errors/`](tests/compile_errors) holds one mistake per file.
The `.stderr` beside each one is exactly what `rustc` reports:

| File | Mistake |
|---|---|
| `01_misspelled_attribute` | `shop.tenant.cod` (suggests `code`) |
| `02_not_on_this_signal` | `url.scheme` on a metric that doesn't define it |
| `03_wrong_value_type` | a string for an `int` |
| `04_enum_value_typo` | `"PSOT"` (suggests `"POST"`) |
| `05_missing_required_attribute` | ending a span without `url.path` and `shop.tenant.code` |
| `06_sampling_relevant_too_late` | `url.scheme` recorded after start |
| `07_span_name_required` | `db.client` without `name:` |
| `08_pii_not_wrapped` | a raw email |
| `09_unbounded_on_metric` | `url.path` on a metric |
| `10_annotation_handler` | `"acme"` where `shop.case: upper` applies |
| `11_not_stable_yet` | a `development` enum value without `semlex-unstable` |
| `12_renamed_attribute` | `shop.cart.size` (renamed to `shop.cart.item_count`) |
| `13_unregistered_needs_review` | an unapproved `unregistered(…)` |
| `14_error_type_not_on_span` | `end!` with `error.type` on a span that doesn't define it |
| `15_end_checks_required` | `end!` on a span missing required attributes |
| `16_ready_names_missing` | returning `Ready` with a required attribute missing |
| `17_typed_literal_wrong_type` | `5i64` and `5` for a string (and `1usize` for an int, accepted) |
