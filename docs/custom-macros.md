# Custom `registry!` macros

Most registries need only `semlex::registry!` and
[handler modules](annotations.md#handlers). Handlers see one attribute's
literal values and take a scalar annotation argument. When you need more,
such as items emitted per signal or structured annotation values, write a
small proc-macro crate around `semlex-expand` with an **annotation hook**.
That macro then replaces `semlex::registry!` in your registry crate (the crate that expands your local registry).

The worked example is [`examples/advanced`](../examples/advanced): a service
whose local registry annotates a metric, and the macro crate
[`examples/advanced/slo-macros`](../examples/advanced/slo-macros) it uses
instead of `semlex::registry!`. It reads per-metric SLOs:

```yaml
metrics:
  - name: http.server.request.duration
    instrument: histogram
    unit: s
    annotations:
      acme:
        slo: { objective: 0.99, latency: 0.5 }
```

From that it emits `crate::slo::http::server::request::duration::{OBJECTIVE, LATENCY, within}`.

## The macro crate

```toml
[lib]
proc-macro = true

[dependencies]
semlex-expand = "…"   # the same version as your `semlex`
quote = "1.0"
```

```rust
use semlex_expand::schema::Group;
use semlex_expand::{AnnotationHook, Emit, Value};

#[proc_macro]
pub fn registry(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    semlex_expand::registry_from_env(input.into(), &[&Slo]).into()
}

struct Slo;

impl AnnotationHook for Slo {
    fn key(&self) -> &str { "acme.slo" }

    fn on_metric(&self, metric: &Group, value: &Value, out: &mut Emit<'_>) {
        let Some(objective) = value.get("objective").and_then(Value::as_f64) else {
            return out.error("expected `objective: <0..1>`");
        };
        let mut module = vec!["slo"];
        module.extend(metric.id.split('.'));
        out.items(&module, quote::quote! { pub const OBJECTIVE: f64 = #objective; });
    }
}
```

## The registry crate

The registry crate invokes your macro, with the same input as `semlex::registry!`,
and still depends on `semlex` for the runtime and call-site macros:

```toml
[dependencies]
semlex = "…"
acme-macros = { path = "../acme-macros" }
```

```rust
acme_macros::registry!("resolved.yaml", { scope_prefix: "acme." });
```

Call sites are unchanged: `span!`, `metric!` and the rest work as usual.

## Hooks

| Method | Called for |
|---|---|
| `key()` | The dotted annotation path the hook owns, e.g. `acme.slo`. It and everything below it count as declared; don't also list it in `annotations`. |
| `on_attribute(&AttributeDef, &Value, &mut Emit)` | Attribute definitions carrying the annotation |
| `on_span`, `on_metric`, `on_event`, `on_entity` (`&Group, &Value, &mut Emit`) | Signals (and refinements) carrying the annotation. Refinements inherit their base's annotations, so expect both. |

- `value` is the annotation's value in any YAML shape (`serde_json::Value`,
  re-exported as `semlex_expand::Value`). Validate it, and reject unknown
  fields.
- Only emitted items are passed, after `stability`, `groups` and
  `code_generation.exclude` are applied.
- Methods you don't implement are errors when called. An annotation on an
  item kind your hook doesn't handle (and any hook annotation on an attribute
  group) fails the build.
- Signal-level overrides of attribute annotations aren't dispatched to
  `on_attribute`. Read them from `Group::attributes` in the signal hooks.

`Emit`:

| Method | |
|---|---|
| `items(&["slo", "http"], tokens)` | Emits items in `crate::slo::http`. Modules are shared across calls and hooks; segments are mangled like registry names. The root modules `registry!()` emits (`attr`, `metrics`, …) are reserved. |
| `error(msg)` | A compile error at the `registry!` path: "annotation `acme.slo` on metric `x`: msg". |
| `signal_path(&group)` | `crate::metrics::…` for the signal's generated module. |
| `attribute_path(key)` | `crate::attr::…` for the attribute's constant. |

## Testing

Call `registry_with_hooks(input, dir, crate_name, &[&YourHook])` from a unit
test, and snapshot the output with `prettyplease` and `insta` (see
`examples/advanced/slo-macros/src/lib.rs`). Test the items themselves in the
registry crate (`examples/advanced/src/lib.rs`).

## Stability

The supported API is `registry`, `registry_with_hooks`, `registry_from_env`,
`AnnotationHook`, `Emit`, and the re-exported `schema` (`semlex-schema`) and
`Value` types. Other public items are semlex's own plumbing; they're hidden
from the docs and may change at any time.

Before 1.0, a breaking change to the supported API bumps the minor version
and is listed in the CHANGELOG. `semlex-expand` and `semlex` are released
together; use the same version of both. The `schema` types follow Weaver's
resolved format, so a Weaver bump can change them.
