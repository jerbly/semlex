# semlex-schema

The serde model of Weaver's `resolved/2.0` registry format, and the
effective view of it that [semlex](https://crates.io/crates/semlex) uses.

You probably want [`semlex`](https://crates.io/crates/semlex). This crate is
for tools that read a resolved registry themselves:

```rust
let registry = semlex_schema::load("resolved.yaml")?;
for group in &registry.groups {
    println!("{} {}: {} attributes", group.kind.tag().noun(), group.id, group.attributes.len());
}
```

Make the file with `weaver registry package --v2`.

This crate is interim: semlex will switch to Weaver's own schema crate when
that is published. Custom macros should import these types through
`semlex_expand::schema`, which will keep working where the types match.

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
