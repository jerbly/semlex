# semlex

Compile-time OpenTelemetry semantic conventions for Rust: `registry!()` expands
a Weaver V2 resolved registry into Rust, and call-site macros (`span!`,
`metric!`, …) are checked against it by `rustc`.

**Model:** each service or library has a *local* registry that depends on
upstream registries (OpenTelemetry semconv, an organization's shared one)
through Weaver dependencies, and expands it with `registry!` in that crate,
or optionally in a workspace `*-semconv` crate shared by several crates.
There's no organization-wide registry or crate. Keep examples neutral
(`acme.*`, `shop.*`); no company names.

## Where things are

- **Usage:** `docs/guide.md`, `docs/annotations.md`, `docs/custom-macros.md`,
  and `examples/` (`simple`, `showcase` with every feature, `advanced`).
- **Agent instructions for users:** `agents/`.
- **Weaver behaviour semlex handles, and open upstream issues:**
  `fixtures/README.md`.
- **Crates:**
  - `semlex-schema`: serde model and effective view of `resolved/2.0`;
  - `semlex-expand`: the expansion engine, with `emit.rs` (what
    `registry!()` emits) and `callsite.rs` (call-site macros);
  - `semlex-macros`: proc-macro shims;
  - `semlex`: the runtime and facade.
- **semlex's own test crates:** `tests/fixture-semconv` (UI tests for every
  diagnostic), `tests/fixture-service` (runtime tests) and
  `tests/otel-stress` (all of OTel semconv, for `just measure`).

## Working here

- `just ci` runs everything CI does. `just test` runs `fixture-semconv`
  separately, because Cargo unifies its `semlex-unstable` feature.
- Pinned: Rust 1.99 with `rust-src` (`rust-toolchain.toml`; trybuild output
  depends on it), MSRV 1.89 (`just msrv`), Weaver 0.28.0 (`justfile`),
  OpenTelemetry Rust 0.33.
- Diagnostics are tested with trybuild (`tests/ui`, and the showcase's
  `tests/compile_errors`), and expansions with insta snapshots. When output
  changes:
  1. check that the change is the intended one (diff the headlines);
  2. re-record with `TRYBUILD=overwrite` or `INSTA_UPDATE=always`.
- After editing a registry, re-package it: `just registry` (fixtures) or
  `just example-registries`. `just fixtures-check` fails on drift.
- `just measure` runs the full-OTel stress test: a regression marker for
  what `registry!()` emits (≤ 2 s, ≤ 700 MB). Re-measure after changes to it.
- Keep the docs in step with any change: the guide, the CHANGELOG and the
  examples.
- Commit only when asked.
