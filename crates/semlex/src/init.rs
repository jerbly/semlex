//! `semlex::init`: providers, the initialized flag and opt-in signals.
//!
//! Call sites create their instruments, tracers and loggers lazily. After
//! [`init`] they cache them for good. Without `init` (library mode: a library
//! never calls it, and an application may install OpenTelemetry's global
//! providers itself), spans and metrics follow the global providers as they
//! are when each signal is recorded, so nothing stays stuck on the no-op
//! provider that's global at startup. Events need `init`, because
//! OpenTelemetry Rust has no global logger provider.

use opentelemetry::InstrumentationScope;
use opentelemetry::KeyValue;
use opentelemetry::logs::{AnyValue, LogRecord, Logger, LoggerProvider};
use opentelemetry::metrics::MeterProvider;
use std::collections::BTreeSet;
use std::fmt;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

static INITIALIZED: AtomicBool = AtomicBool::new(false);
static STATE: OnceLock<State> = OnceLock::new();

struct State {
    logger_provider: Option<Box<dyn DynLoggerProvider>>,
    /// Whether `init` installed the global meter / tracer provider. Only
    /// then are those providers fixed, so call sites may cache for good.
    installed_meter: bool,
    installed_tracer: bool,
    enabled: BTreeSet<String>,
    resource: Option<Vec<KeyValue>>,
}

/// What [`init`] installs.
#[derive(Default)]
#[must_use = "pass the config to `semlex::init`"]
pub struct Config {
    meter_provider: Option<Box<dyn FnOnce() + Send>>,
    tracer_provider: Option<Box<dyn FnOnce() + Send>>,
    logger_provider: Option<Box<dyn DynLoggerProvider>>,
    enabled: BTreeSet<String>,
    resource: Option<Vec<KeyValue>>,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("meter_provider", &self.meter_provider.is_some())
            .field("tracer_provider", &self.tracer_provider.is_some())
            .field("logger_provider", &self.logger_provider.is_some())
            .field("enabled", &self.enabled)
            .finish()
    }
}

impl Config {
    /// An empty config: no providers, no opt-in signals enabled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Installs `provider` as the global meter provider at [`init`].
    pub fn with_meter_provider<P>(mut self, provider: P) -> Self
    where
        P: MeterProvider + Send + Sync + 'static,
    {
        self.meter_provider = Some(Box::new(move || {
            opentelemetry::global::set_meter_provider(provider)
        }));
        self
    }

    /// Installs `provider` as the global tracer provider at [`init`].
    pub fn with_tracer_provider<P, T, S>(mut self, provider: P) -> Self
    where
        P: opentelemetry::trace::TracerProvider<Tracer = T> + Send + Sync + 'static,
        T: opentelemetry::trace::Tracer<Span = S> + Send + Sync + 'static,
        S: opentelemetry::trace::Span + Send + Sync + 'static,
    {
        self.tracer_provider = Some(Box::new(move || {
            opentelemetry::global::set_tracer_provider(provider);
        }));
        self
    }

    /// Uses `provider` for events (`event!`). The OpenTelemetry Rust API has
    /// no global logger provider, so semlex holds this one.
    pub fn with_logger_provider<P>(mut self, provider: P) -> Self
    where
        P: LoggerProvider + Send + Sync + 'static,
        P::Logger: Send + Sync + 'static,
    {
        self.logger_provider = Some(Box::new(provider));
        self
    }

    /// The Resource's attributes (the same ones given to the SDK), so each
    /// signal type's entity associations are checked when it's first used
    /// (debug builds warn) and by [`crate::check_associations`].
    pub fn with_resource(mut self, attributes: impl IntoIterator<Item = KeyValue>) -> Self {
        self.resource = Some(attributes.into_iter().collect());
        self
    }

    /// Enables an opt-in signal (`requirement_level: opt_in`) by name, e.g.
    /// `demo.cache.fill_ratio`. Opt-in signals record nothing unless enabled.
    pub fn enable(mut self, signal: impl Into<String>) -> Self {
        self.enabled.insert(signal.into());
        self
    }
}

/// [`init`] was called twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AlreadyInitialized;

impl fmt::Display for AlreadyInitialized {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "semlex::init was already called; providers can only be installed once per process",
        )
    }
}

impl std::error::Error for AlreadyInitialized {}

/// Installs the providers and configures semlex. Call it once, in the
/// application (never in a library), at startup. It's optional for spans and
/// metrics, which otherwise use whatever global providers the application
/// installed, but needed for events (the logger provider), opt-in signals and
/// entity association checks.
///
/// A second call returns [`AlreadyInitialized`] and changes nothing: call
/// sites that already created their instruments would keep the first
/// providers anyway.
pub fn init(config: Config) -> Result<(), AlreadyInitialized> {
    let Config {
        meter_provider,
        tracer_provider,
        logger_provider,
        enabled,
        resource,
    } = config;
    STATE
        .set(State {
            logger_provider,
            installed_meter: meter_provider.is_some(),
            installed_tracer: tracer_provider.is_some(),
            enabled,
            resource,
        })
        .map_err(|_| AlreadyInitialized)?;
    for install in [meter_provider, tracer_provider].into_iter().flatten() {
        install();
    }
    INITIALIZED.store(true, Ordering::Release);
    Ok(())
}

/// Whether [`init`] has run.
pub fn is_initialized() -> bool {
    INITIALIZED.load(Ordering::Acquire)
}

/// Whether `init` installed the global meter provider (so it's fixed).
pub(crate) fn owns_meter_provider() -> bool {
    is_initialized() && STATE.get().is_some_and(|s| s.installed_meter)
}

/// Whether `init` installed the global tracer provider (so it's fixed).
pub(crate) fn owns_tracer_provider() -> bool {
    is_initialized() && STATE.get().is_some_and(|s| s.installed_tracer)
}

/// Whether a signal should record: always for recommended signals; for
/// opt-in signals, only when enabled in the [`Config`].
pub(crate) fn signal_enabled(name: &str, opt_in: bool) -> bool {
    !opt_in || STATE.get().is_some_and(|s| s.enabled.contains(name))
}

pub(crate) fn resource() -> Option<&'static [KeyValue]> {
    STATE.get()?.resource.as_deref()
}

pub(crate) fn logger(scope: InstrumentationScope) -> Option<Box<dyn DynLogger>> {
    STATE
        .get()?
        .logger_provider
        .as_ref()
        .map(|p| p.logger(scope))
}

/// A type-erased logger that emits semlex events.
pub(crate) trait DynLogger: Send + Sync {
    fn emit_event(&self, event_name: &'static str, attributes: Vec<KeyValue>) {
        self.emit_record(event_name, None, attributes);
    }
    /// An event log record, with a severity if given (e.g. `exception`).
    fn emit_record(
        &self,
        event_name: &'static str,
        severity: Option<(opentelemetry::logs::Severity, &'static str)>,
        attributes: Vec<KeyValue>,
    );
}

impl<L> DynLogger for L
where
    L: Logger + Send + Sync,
{
    fn emit_record(
        &self,
        event_name: &'static str,
        severity: Option<(opentelemetry::logs::Severity, &'static str)>,
        attributes: Vec<KeyValue>,
    ) {
        let mut record = self.create_log_record();
        record.set_event_name(event_name);
        if let Some((number, text)) = severity {
            record.set_severity_number(number);
            record.set_severity_text(text);
        }
        record.add_attributes(
            attributes
                .into_iter()
                .map(|kv| (kv.key, any_value(kv.value))),
        );
        // The SDK attaches the current trace context when the record has none.
        self.emit(record);
    }
}

trait DynLoggerProvider: Send + Sync {
    fn logger(&self, scope: InstrumentationScope) -> Box<dyn DynLogger>;
}

impl<P> DynLoggerProvider for P
where
    P: LoggerProvider + Send + Sync,
    P::Logger: Send + Sync + 'static,
{
    fn logger(&self, scope: InstrumentationScope) -> Box<dyn DynLogger> {
        Box::new(self.logger_with_scope(scope))
    }
}

fn any_value(v: opentelemetry::Value) -> AnyValue {
    use opentelemetry::{Array, Value};
    let list = |items: Vec<AnyValue>| AnyValue::ListAny(Box::new(items));
    match v {
        Value::Bool(b) => AnyValue::Boolean(b),
        Value::I64(i) => AnyValue::Int(i),
        Value::F64(f) => AnyValue::Double(f),
        Value::String(s) => AnyValue::String(s),
        Value::Array(Array::Bool(v)) => list(v.into_iter().map(AnyValue::Boolean).collect()),
        Value::Array(Array::I64(v)) => list(v.into_iter().map(AnyValue::Int).collect()),
        Value::Array(Array::F64(v)) => list(v.into_iter().map(AnyValue::Double).collect()),
        Value::Array(Array::String(v)) => list(v.into_iter().map(AnyValue::String).collect()),
        other => AnyValue::String(other.as_str().into_owned().into()),
    }
}
