# semlex-expand

The expansion engine for [semlex](https://crates.io/crates/semlex): what
`registry!()` and the call-site macros expand to. It also has the API for
custom `registry!` macros.

You probably want [`semlex`](https://crates.io/crates/semlex). Use this crate
directly only to write your own `registry!` proc macro with annotation hooks,
for annotations that semlex's handler modules can't express. See
[Custom macros](https://github.com/jerbly/semlex/blob/main/docs/custom-macros.md) and the
[advanced example](https://github.com/jerbly/semlex/tree/main/examples/advanced).
Use the same version as your `semlex`.

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
