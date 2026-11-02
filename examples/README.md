# Examples

Each example is one crate with its own local registry (`registry/`), which
takes attributes from OpenTelemetry semconv as its upstream, packaged into
`resolved.yaml` and expanded by `registry!` in the crate. Re-package after
editing a registry: `just example-registries`.

| Example | What it shows |
|---|---|
| [`simple`](simple) | **Start here.** The quick start as a runnable crate: one span, one metric, a local attribute and an upstream one, a telemetry test. `cargo run -p simple`. |
| [`showcase`](showcase) | A shop's checkout using every semlex feature. Runnable (`cargo run -p showcase`), with telemetry tests and [a tour of compile errors](showcase/tests/compile_errors). |
| [`advanced`](advanced) | A custom `registry!` macro ([`advanced/slo-macros`](advanced/slo-macros)) with an annotation hook: per-metric SLOs from a structured annotation. See [docs/custom-macros.md](../docs/custom-macros.md). |

semlex's own test crates are in [`tests/`](../tests): `fixture-semconv` and
`fixture-service` (the fixture registry, every diagnostic and runtime
behaviour) and `otel-stress` (all of OpenTelemetry semconv in one crate, a
stress test for `just measure`).
