//! The smallest semlex service: `cargo run -p simple`.
//!
//! Its local registry (`registry/`) defines one span and one metric, and
//! takes `error.type` from OpenTelemetry semconv, its upstream. It's packaged
//! into `resolved.yaml` (`just example-registries`) and expanded here.

use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::trace::SdkTracerProvider;

// The macros and modules stay private to this crate.
semlex::registry!("resolved.yaml", { visibility: crate });

use crate::{metric, record, span};

/// Greets `name`, with a span and a count.
fn greet(name: &str) -> Result<String, &'static str> {
    let span = span!(greeter.greet { greeter.name: name });
    if name.is_empty() {
        record!(&span, error.type: "empty_name");
        metric!(greeter.greetings: 1, { error.type: "empty_name" });
        span.end();
        return Err("nobody to greet");
    }
    metric!(greeter.greetings: 1);
    span.end();
    Ok(format!("Hello, {name}!"))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Ordinary OpenTelemetry SDK setup; any exporters work.
    let tracers = SdkTracerProvider::builder()
        .with_simple_exporter(opentelemetry_stdout::SpanExporter::default())
        .build();
    let meters = SdkMeterProvider::builder()
        .with_reader(
            PeriodicReader::builder(opentelemetry_stdout::MetricExporter::default()).build(),
        )
        .build();
    semlex::init(
        semlex::Config::new()
            .with_tracer_provider(tracers.clone())
            .with_meter_provider(meters.clone()),
    )?;

    println!("{}", greet("world")?);
    let _ = greet("");

    tracers.shutdown()?;
    meters.shutdown()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    // With `visibility: crate`, telemetry tests live in the crate.
    use crate::{assert_metric, assert_span};

    #[test]
    fn greetings_are_traced_and_counted() {
        let t = semlex::testing::Telemetry::init();
        super::greet("test").unwrap();
        assert!(super::greet("").is_err());
        assert_span!(t, greeter.greet { greeter.name: "test" });
        assert_span!(t, greeter.greet { greeter.name: "", error.type: "empty_name" });
        assert_metric!(t, greeter.greetings { error.type: "empty_name" });
    }
}
