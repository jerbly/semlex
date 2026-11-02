//! Call-site support for signal macros: instrumentation scopes, per-call-site
//! instrument and logger caches, and metric measurement kinds.

use crate::entity::{self, Associations};
use crate::init::{self, DynLogger};
use opentelemetry::metrics::Meter;
use opentelemetry::{InstrumentationScope, KeyValue};
use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, RwLock};

/// The instrumentation scope of a call site: the calling crate's name and
/// version, under the registry's scope prefix, with its schema URL.
#[derive(Debug, Clone, Copy)]
pub struct Scope {
    /// Prefix from `registry!`'s `scope_prefix` option (e.g. `acme.`).
    pub prefix: &'static str,
    /// `CARGO_PKG_NAME` of the calling crate.
    pub name: &'static str,
    /// `CARGO_PKG_VERSION` of the calling crate.
    pub version: &'static str,
    /// The registry's schema URL.
    pub schema_url: &'static str,
}

impl Scope {
    /// The OpenTelemetry scope.
    pub fn instrumentation_scope(&self) -> InstrumentationScope {
        InstrumentationScope::builder(format!("{}{}", self.prefix, self.name))
            .with_version(self.version)
            .with_schema_url(self.schema_url)
            .build()
    }
}

/// A metric call site's instrument.
///
/// When `semlex::init` installed the meter provider, the provider is fixed,
/// so the instrument is created once. Otherwise (library mode, or `init`
/// without a meter provider) the application owns the global provider and
/// may install or replace it at any time: the instrument is cached together
/// with the provider it came from (the `Arc` itself, so its address can't be
/// reused by a later provider) and re-created when the global one changes.
/// An instrument from the no-op provider that's global at startup would
/// otherwise record nothing for good.
pub struct MetricSite<I> {
    after_init: OnceLock<I>,
    global: RwLock<Option<(GlobalMeterProvider, I)>>,
}

type GlobalMeterProvider = Arc<dyn opentelemetry::metrics::MeterProvider + Send + Sync>;

impl<I> Default for MetricSite<I> {
    fn default() -> Self {
        Self::new()
    }
}

impl<I> MetricSite<I> {
    /// An empty site (one `static` per call site).
    pub const fn new() -> Self {
        Self {
            after_init: OnceLock::new(),
            global: RwLock::new(None),
        }
    }

    /// The instrument, or `None` for a disabled opt-in metric.
    pub fn instrument(
        &self,
        name: &str,
        opt_in: bool,
        scope: &Scope,
        associations: &'static Associations,
        create: fn(&Meter) -> I,
    ) -> Option<Cow<'_, I>>
    where
        I: Clone,
    {
        if !init::signal_enabled(name, opt_in) {
            return None;
        }
        if init::owns_meter_provider() {
            return Some(Cow::Borrowed(self.after_init.get_or_init(|| {
                entity::observe(associations);
                create(&opentelemetry::global::meter_with_scope(
                    scope.instrumentation_scope(),
                ))
            })));
        }
        // The application owns the global meter provider: follow it.
        let provider = opentelemetry::global::meter_provider();
        if let Ok(cached) = self.global.read()
            && let Some((cached_provider, i)) = &*cached
            && Arc::ptr_eq(cached_provider, &provider)
        {
            return Some(Cow::Owned(i.clone()));
        }
        entity::observe(associations);
        let i = create(&provider.meter_with_scope(scope.instrumentation_scope()));
        if let Ok(mut cached) = self.global.write() {
            *cached = Some((provider, i.clone()));
        }
        Some(Cow::Owned(i))
    }
}

/// An event call site's logger, created on first use after `init`.
///
/// OpenTelemetry Rust has no global logger provider. Without one from
/// `semlex::init` (library mode, or `init` without a logger provider), the
/// event goes to its call site's `tracing` fallback instead: a `tracing`
/// event named after it, which the application's `tracing` bridge turns into
/// a log record with that `event_name` (with the bridge's scope, a severity,
/// and without array or `any` attributes).
pub struct EventSite {
    logger: OnceLock<Option<Box<dyn DynLogger>>>,
    observed: AtomicBool,
}

impl Default for EventSite {
    fn default() -> Self {
        Self::new()
    }
}

impl EventSite {
    /// An empty site (one `static` per call site).
    pub const fn new() -> Self {
        Self {
            logger: OnceLock::new(),
            observed: AtomicBool::new(false),
        }
    }

    /// Emits the event through semlex's logger, or through `fallback` (the
    /// call site's `tracing` event) when there's no logger provider. Nothing
    /// for a disabled opt-in event.
    #[allow(clippy::too_many_arguments)]
    pub fn emit(
        &self,
        event_name: &'static str,
        opt_in: bool,
        scope: &Scope,
        associations: &'static Associations,
        attributes: Vec<KeyValue>,
        fallback: fn(&[KeyValue]),
    ) {
        if !init::signal_enabled(event_name, opt_in) {
            return;
        }
        let logger = if init::is_initialized() {
            self.logger
                .get_or_init(|| {
                    entity::observe(associations);
                    init::logger(scope.instrumentation_scope())
                })
                .as_deref()
        } else {
            None
        };
        match logger {
            Some(logger) => logger.emit_event(event_name, attributes),
            None => {
                if !self.observed.swap(true, Ordering::Relaxed) {
                    entity::observe(associations);
                }
                fallback(&attributes);
            }
        }
    }
}

/// What a metric's measurements are (from the instrument and
/// `code_generation.metric_value_type`).
pub mod measure {
    /// A measurement kind and the number type its instrument records.
    pub trait Kind {
        /// The instrument's number type.
        type Number;
    }
    /// `double` metrics: `f64`, `f32`.
    pub struct F64;
    /// `double` metrics in seconds (unit `s`): `f64`, `f32`, `Duration`.
    pub struct Seconds;
    /// `int` counters and histograms: unsigned integers.
    pub struct U64;
    /// `int` up-down counters and gauges: signed integers.
    pub struct I64;
    impl Kind for F64 {
        type Number = f64;
    }
    impl Kind for Seconds {
        type Number = f64;
    }
    impl Kind for U64 {
        type Number = u64;
    }
    impl Kind for I64 {
        type Number = i64;
    }
}

/// Converts a value into a measurement of kind `K`. Concrete impls only, so
/// a wrong type reports this message.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a valid measurement here: this metric takes `{K}` measurements",
    label = "expected a `{K}` measurement",
    note = "`F64`: `f64`, `f32` | `Seconds` (unit `s`): `f64`, `f32` or `std::time::Duration` | `U64` (int counters, histograms): `u64`, `u32`, `u16`, `u8`, `usize`, or a non-negative literal | `I64` (int up-down counters, gauges): any integer up to 64 bits (`u64`/`usize` saturate at `i64::MAX`)"
)]
pub trait IntoMeasurement<K: measure::Kind> {
    /// The measurement.
    fn into_measurement(self) -> K::Number;
}

macro_rules! measurement {
    ($kind:ident: $($t:ty => |$v:ident| $e:expr),+ $(,)?) => {
        $(impl IntoMeasurement<measure::$kind> for $t {
            fn into_measurement(self) -> <measure::$kind as measure::Kind>::Number {
                let $v = self;
                $e
            }
        })+
    };
}

measurement!(F64: f64 => |v| v, f32 => |v| v.into());
measurement!(Seconds: f64 => |v| v, f32 => |v| v.into(), std::time::Duration => |v| v.as_secs_f64());
measurement!(U64: u64 => |v| v, u32 => |v| v.into(), u16 => |v| v.into(), u8 => |v| v.into(), usize => |v| v as u64);
measurement!(I64: i64 => |v| v, i32 => |v| v.into(), i16 => |v| v.into(), i8 => |v| v.into(), u32 => |v| v.into(), u16 => |v| v.into(), u8 => |v| v.into(), isize => |v| v as i64);
// Like `int` attributes: counts and sizes saturate at `i64::MAX`.
measurement!(I64: u64 => |v| i64::try_from(v).unwrap_or(i64::MAX), usize => |v| i64::try_from(v).unwrap_or(i64::MAX));

/// A non-negative integer literal measurement (`metric!(m: 1, …)`): valid for
/// any `int` instrument, and for `double` ones (as `1.0`). Without it an
/// unsuffixed `1` would be an `i32`, which counters don't take. Negative
/// literals stay `i32`, so a counter rejects them.
#[derive(Debug, Clone, Copy)]
pub struct IntLiteral(pub u64);
measurement!(U64: IntLiteral => |v| v.0);
measurement!(I64: IntLiteral => |v| i64::try_from(v.0).unwrap_or(i64::MAX));
measurement!(F64: IntLiteral => |v| v.0 as f64);
