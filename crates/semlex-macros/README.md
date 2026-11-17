# semlex-macros

The proc-macro shim for [semlex](https://crates.io/crates/semlex):
`registry!()`, `#[handler]` and the call-site macros.

Don't depend on this crate directly. Depend on
[`semlex`](https://crates.io/crates/semlex), which re-exports these macros
and has the runtime that the generated code uses.

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
