//! Runtime support for the log macros (`info!`, `warn!`, …): registry-checked
//! fields on `tracing` events.
//!
//! `tracing`'s `Value` is sealed, so each attribute kind maps to a type that
//! already implements it. Arrays and `any` have no mapping: `tracing` can't
//! carry them, so they're a compile error pointing to `event!`.

use crate::{Attribute, IntoValue, kind};
use opentelemetry::Value;
use tracing::field::DisplayValue;

/// How an attribute kind is recorded as a `tracing` field.
#[diagnostic::on_unimplemented(
    message = "`{Self}` attributes can't be logged through `tracing`",
    label = "arrays and `any` values have no `tracing` field type",
    note = "emit an event with `event!` (the OpenTelemetry Logs API) to record this attribute"
)]
pub trait LogKind {
    /// The `tracing` field type.
    type Field: tracing::Value;
    /// Converts the (already checked and transformed) value.
    fn field(value: Value) -> Self::Field;
}

fn text(value: Value) -> DisplayValue<String> {
    tracing::field::display(value.as_str().into_owned())
}

impl LogKind for kind::StringValue {
    type Field = DisplayValue<String>;
    fn field(value: Value) -> Self::Field {
        text(value)
    }
}

impl LogKind for kind::IntValue {
    type Field = i64;
    fn field(value: Value) -> i64 {
        match value {
            Value::I64(i) => i,
            other => other.as_str().parse().unwrap_or_default(),
        }
    }
}

impl LogKind for kind::DoubleValue {
    type Field = f64;
    fn field(value: Value) -> f64 {
        match value {
            Value::F64(f) => f,
            other => other.as_str().parse().unwrap_or_default(),
        }
    }
}

impl LogKind for kind::BoolValue {
    type Field = bool;
    fn field(value: Value) -> bool {
        matches!(value, Value::Bool(true))
    }
}

/// PII values are already `[REDACTED]` or a hash: text.
impl<K> LogKind for kind::Pii<K> {
    type Field = DisplayValue<String>;
    fn field(value: Value) -> Self::Field {
        text(value)
    }
}

/// `code_generation.rust.type`: recorded as the underlying kind.
impl<K: LogKind, T> LogKind for kind::Typed<K, T> {
    type Field = K::Field;
    fn field(value: Value) -> Self::Field {
        K::field(value)
    }
}

/// Enum attributes: generated enums implement this through their wire type.
#[doc(hidden)]
pub fn text_field(value: Value) -> DisplayValue<String> {
    text(value)
}

/// One log field: the value checked, converted and transformed like any
/// attribute value, then mapped to its `tracing` type.
pub fn log_field<A, V>(attribute: A, value: V) -> <A::Kind as LogKind>::Field
where
    A: Attribute,
    A::Kind: LogKind,
    V: IntoValue<A::Kind>,
{
    <A::Kind as LogKind>::field(attribute.apply(value.into_value()))
}

/// An attribute's key, as a constant `tracing` field name.
pub const fn key_of<A: Attribute>(_attribute: &A) -> &'static str {
    A::KEY
}

/// The conventions' log-based `exception` event, in the current context (the
/// span's, attached by the caller), with severity `ERROR`.
///
/// Through the logger provider given to `semlex::init` when there is one.
/// Otherwise (library mode) as a `tracing` event named `exception`, which the
/// application's `tracing` bridge turns into the same log record (the bridge
/// uses the `tracing` name as `event_name`).
pub(crate) fn exception(
    scope: &crate::signal::Scope,
    ty: Option<&'static str>,
    message: String,
    stacktrace: Option<String>,
) {
    if let Some(logger) = crate::init::logger(scope.instrumentation_scope()) {
        let mut attributes = vec![opentelemetry::KeyValue::new("exception.message", message)];
        if let Some(ty) = ty {
            attributes.push(opentelemetry::KeyValue::new("exception.type", ty));
        }
        if let Some(st) = stacktrace {
            attributes.push(opentelemetry::KeyValue::new("exception.stacktrace", st));
        }
        logger.emit_record(
            "exception",
            Some((opentelemetry::logs::Severity::Error, "ERROR")),
            attributes,
        );
        return;
    }
    tracing::event!(
        name: "exception",
        tracing::Level::ERROR,
        "exception.message" = %message,
        "exception.type" = ty,
        "exception.stacktrace" = stacktrace.as_deref(),
    );
}

/// How an attribute kind is carried by `event!`'s `tracing` fallback (library
/// mode). Every kind the log macros can log, as they log it; arrays and `any`
/// can't be `tracing` fields, so they're left out of the fallback record.
pub trait EventKind {
    /// The `tracing` field type.
    type Field: tracing::Value;
    /// The field, or `None` to leave it out.
    fn field(value: Value) -> Option<Self::Field>;
}

impl<K: LogKind> EventKind for K {
    type Field = K::Field;
    fn field(value: Value) -> Option<Self::Field> {
        Some(K::field(value))
    }
}

macro_rules! not_a_field {
    ($($k:ident),*) => {$(
        impl EventKind for kind::$k {
            type Field = DisplayValue<String>;
            fn field(_: Value) -> Option<Self::Field> {
                None
            }
        }
    )*};
}
not_a_field!(
    StringArrayValue,
    IntArrayValue,
    DoubleArrayValue,
    BoolArrayValue,
    AnyValue
);

/// One field of `event!`'s `tracing` fallback: `attribute`'s value among the
/// event's (already checked and transformed) attributes, if it was set.
pub fn event_field<A>(
    _attribute: &A,
    attributes: &[opentelemetry::KeyValue],
) -> Option<<A::Kind as EventKind>::Field>
where
    A: Attribute,
    A::Kind: EventKind,
{
    attributes
        .iter()
        .find(|kv| kv.key.as_str() == A::KEY)
        .and_then(|kv| <A::Kind as EventKind>::field(kv.value.clone()))
}
