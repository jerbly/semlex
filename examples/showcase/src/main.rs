//! Runs a few checkouts and prints the telemetry: `cargo run -p showcase`.

use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider};
use opentelemetry_sdk::trace::SdkTracerProvider;
use showcase::enums::PaymentMethod;
use showcase::{CheckoutRequest, Customer};
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // The Resource, from registry entities.
    let attributes = showcase::resource_attributes("checkout-1");
    let resource = Resource::builder()
        .with_attributes(attributes.clone())
        .build();

    // Ordinary OpenTelemetry SDK setup; any exporters work.
    let tracers = SdkTracerProvider::builder()
        .with_resource(resource.clone())
        .with_simple_exporter(opentelemetry_stdout::SpanExporter::default())
        .build();
    let meters = SdkMeterProvider::builder()
        .with_resource(resource.clone())
        .with_reader(
            PeriodicReader::builder(opentelemetry_stdout::MetricExporter::default()).build(),
        )
        .build();
    let loggers = SdkLoggerProvider::builder()
        .with_resource(resource)
        .with_simple_exporter(opentelemetry_stdout::LogExporter::default())
        .build();
    // The log macros are `tracing` events: bridge them to OpenTelemetry,
    // leaving out the SDK's own diagnostics.
    let bridge = OpenTelemetryTracingBridge::new(&loggers).with_filter(filter_fn(|meta| {
        !meta.target().starts_with("opentelemetry")
    }));
    tracing_subscriber::registry().with(bridge).init();

    // semlex: once, at startup.
    semlex::init(
        semlex::Config::new()
            .with_tracer_provider(tracers.clone())
            .with_meter_provider(meters.clone())
            .with_logger_provider(loggers.clone())
            // Entity associations are checked against this (debug builds warn).
            .with_resource(attributes)
            // Opt-in signals are off unless enabled by name.
            .enable("shop.inventory.fill_ratio"),
    )?;

    let ok = showcase::checkout(&CheckoutRequest {
        method: http::Method::POST,
        path: "/cart/42/checkout".into(),
        tenant: "acme".into(),
        customer: Some(Customer {
            id: "cust-7".into(),
            email: "ada@example.com".into(),
        }),
        skus: vec!["SKU-1".into(), "SKU-2".into()],
        payment: PaymentMethod::Card,
    });
    println!("checkout with a card: {ok:?}");

    let declined = showcase::checkout(&CheckoutRequest {
        method: http::Method::POST,
        path: "/cart/43/checkout".into(),
        tenant: "acme".into(),
        customer: None,
        skus: vec!["SKU-3".into()],
        payment: PaymentMethod::Invoice,
    });
    println!("checkout by invoice: {declined:?}");

    showcase::record_unusual_method("/cart/44");
    showcase::report_inventory(0.72, 0.31);

    tracers.shutdown()?;
    meters.shutdown()?;
    loggers.shutdown()?;
    Ok(())
}
