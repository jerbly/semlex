# Annotations

Registry annotations customize what `registry!()` emits; they replace the
customization Jinja templates used to provide.

## The declaration rule

Every annotation in the registry must be one of:

- **built in** to semlex: `code_generation`, `cardinality`, `sensitivity`;
- **upstream** (recognized and ignored, so registries importing OpenTelemetry
  semconv need no declarations): `dependency_resolution`,
  `naming_conventions`, `stability`;
- **declared** in `registry!`'s `annotations` option, as a handler module or
  `ignore`:

```rust
semlex::registry!("resolved.yaml", {
    annotations: {
        acme.case: crate::annotations::case,   // a #[semlex::handler] module
        acme.owner: ignore,                    // used by other tools only
    },
});
```

A declaration covers that path and everything below it. An undeclared
annotation is a `registry!()` error naming the key and the item it's on, so a
YAML typo fails the build instead of doing nothing.

Annotations on a signal's reference to an attribute override the definition's
per key (recursively into maps). Weaver (0.27, 0.28) drops the definition's other
keys in that case; semlex merges them back.

## Built-in annotations

| Annotation | Effect |
|---|---|
| `code_generation.exclude: true` | The attribute, enum member or signal isn't emitted. |
| `code_generation.metric_value_type: int \| double` | The metric's measurement type (default `double`). |
| `code_generation.rust.ident: name` | Replaces the attribute's last path segment (keywords, collisions), or an enum member's variant name. |
| `code_generation.rust.enum_name: Name` | The generated enum's type name. |
| `code_generation.rust.type: path::Type` | The attribute takes this Rust type instead, converted with `semlex::ToValue` (built in: `Duration` as seconds for `double`, IP and socket addresses for `string`; implement it for your own types). |
| `cardinality: bounded \| unbounded` | `unbounded` attributes are a compile error on metrics. |
| `sensitivity: pii` | Values must be `redacted(…)` (records `[REDACTED]`) or `hashed(…)` (HMAC-SHA256, keyed per process unless `semlex::pii::set_hash_key` is called). |

`policies/registry/annotations.rego` validates these shapes, and bans
unbounded attributes on metrics, in `weaver registry check --v2 -p
policies/registry`, so mistakes surface in the registry PR.

## Handlers

A handler is a module marked `#[semlex::handler]`, in the registry crate. It
implements any of three hooks; the attribute fills in no-op defaults for the
rest.

```rust
pub mod annotations {
    #[semlex::handler]
    pub mod case {
        use semlex::Value;

        /// Compile-time check of literal values (a `const fn`).
        pub const fn check_literal(value: &str, arg: &str) {
            if semlex::const_fns::str_eq(arg, "upper") {
                let b = value.as_bytes();
                let mut i = 0;
                while i < b.len() {
                    if b[i].is_ascii_lowercase() {
                        panic!("value must be upper case (annotation `acme.case: upper`)");
                    }
                    i += 1;
                }
            }
        }

        /// Runtime transform of every value, after conversion.
        pub fn apply(value: Value, arg: &str) -> Value {
            match value {
                Value::String(s) if arg == "upper" => Value::from(s.as_str().to_uppercase()),
                other => other,
            }
        }

        // Optional: change which values the attribute accepts.
        // pub type Kind<K> = …;
    }
}
```

| Hook | Signature | When |
|---|---|---|
| `check_literal` | `pub const fn check_literal(value: T, arg: A)` | Literal values, at compile time (`cargo check` reports it) |
| `Kind` | `pub type Kind<K> = …;` | Wraps the attribute's value kind (how `sensitivity: pii` works) |
| `apply` | `pub fn apply(value: Value, arg: A) -> Value` | Every value, at run time |

- `T` is the attribute's literal type (`&str`, `i64`, `f64`, `bool`); `A` is
  the annotation's value as a Rust literal. Maps and lists aren't allowed as
  handler arguments.
- `check_literal` messages must be static strings (`const fn` panics can't
  format): name the annotation; the error location shows the key and value.
- `semlex::const_fns` has const string equality and an edit-distance lookup.
- Handlers apply to attribute definitions; built-ins run first, then declared
  handlers in declaration order.
- A module without `#[semlex::handler]` fails with
  "cannot find value `__MARK_THIS_MODULE_WITH_SEMLEX_HANDLER`".

For behavior handlers can't express (emitting extra items per signal, reading
structured annotation values), wrap `semlex-expand` in your own proc-macro
crate with an annotation hook: see [custom-macros.md](custom-macros.md).
