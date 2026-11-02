//! Compile-time OpenTelemetry semantic conventions for Rust, straight from
//! your Weaver registry.
//!
//! This is the facade crate: it re-exports the macros, and holds the runtime
//! traits and types that generated code and call sites use. The guide
//! (`docs/guide.md`) covers setup and every macro.
//!
//! # `registry!`
//!
//! Invoke it once, at the root of the crate that uses a local registry (a
//! service or library, or a workspace `*-semconv` crate that several crates
//! share):
//!
//! ```ignore
//! semlex::registry!("resolved.yaml");
//! semlex::registry!("resolved.yaml", { stability: stable_only });
//! ```
//!
//! It reads a registry produced by `weaver registry package --v2` (path
//! relative to the crate's `Cargo.toml` directory) and emits:
//!
//! - `attr::…`: one `const` per attribute, e.g. `attr::http::request::method`;
//! - `enums::…`: one enum per enum attribute, e.g. `enums::HttpRequestMethod`;
//! - `spans::…`, `metrics::…`, `events::…`, `entities::…`, `groups::…`: the
//!   typestate builders the signal macros use;
//! - the call-site macros, e.g. `attrs!`.
//!
//! Options:
//!
//! - `stability`: which items are emitted: `all` (default), `stable_only`,
//!   or a minimum level (`development`, `alpha`, `beta`, `release_candidate`).
//!   Using an emitted non-stable item requires the registry crate's
//!   `semlex-unstable` feature (declare it in its `Cargo.toml`), unless
//!   `allow_unstable: true` is set.
//! - `visibility: crate`: the generated modules and macros are `pub(crate)`,
//!   for a crate that uses its own registry.
//! - `annotations: { ns.key: crate::path::to::handler, other: ignore }`:
//!   every annotation in the registry must be built in (`code_generation`,
//!   `cardinality`, `sensitivity`; Weaver's and OTel's own are recognized) or
//!   declared here, as a [`handler`] module or `ignore`.
//! - `conversions: { http.request.method: ::http::Method }`: an enum
//!   attribute also accepts this type (and `&` it).
//! - `groups: ["http.*"]`: emit only these signals and attribute groups.
//! - `scope_prefix: "acme."`: call sites' instrumentation scope is the
//!   prefix plus the calling crate's name.
//!
//! # Signals
//!
//! ```ignore
//! // At startup, once:
//! semlex::init(semlex::Config::new()
//!     .with_meter_provider(meter_provider)
//!     .with_logger_provider(logger_provider)
//!     .with_tracer_provider(tracer_provider)
//!     .enable("demo.cache.fill_ratio"))?;     // opt-in signals, by name
//!
//! crate::metric!(http.server.request.duration: elapsed, {
//!     http.request.method: "GET",
//!     http.route: route,
//! });
//! crate::event!(session.start { session.id: id });
//!
//! // Spans: sampling-relevant attributes at start, the rest before `end()`.
//! let span = crate::span!(http.server.request {
//!     http.request.method: "GET",
//!     url.scheme: "https",
//! });
//! let span = crate::record!(span, http.route: route);
//! call_db(&span.child_context());
//! span.end();
//! ```
//!
//! Logs, as `tracing` events with registry-checked fields (call them by
//! path: `tracing`'s macros have the same names):
//!
//! ```ignore
//! crate::warn!(http.route: route, http.response.status_code: 504, "upstream timed out");
//! crate::error!(group exception { exception.type: "io", exception.message: msg }, "failed");
//! ```
//!
//! Attributes outside the registry, deliberately ugly and policed in CI
//! (see `semlex::__private::unregistered_attribute`):
//!
//! ```ignore
//! #[allow(deprecated, clippy::disallowed_methods)] // reviewed: <why>
//! let span = crate::record!(span, unregistered("retry.attempt"): n);
//! ```
//!
//! Entities and the Resource:
//!
//! ```ignore
//! let service = crate::entity!(service { service.name: "router", service.instance.id: id });
//! let attributes: Vec<KeyValue> = service.into_iter().collect();
//! let resource = Resource::builder().with_attributes(attributes.clone()).build();
//! semlex::init(semlex::Config::new().with_resource(attributes) /* … */)?;
//! semlex::check_associations()?;   // signals used so far vs. the Resource
//! ```
//!
//! **Libraries** never call `init`. Their spans and metrics go through the
//! OpenTelemetry global providers the application installs (with or without
//! `semlex::init`). Events need the application to call `init` with a logger
//! provider, because OpenTelemetry Rust has no global one.
//!
//! # Annotation handlers
//!
//! ```ignore
//! #[semlex::handler]
//! pub mod case {
//!     pub const fn check_literal(value: &str, arg: &str) { /* panic!("…") to reject */ }
//!     pub fn apply(value: semlex::Value, arg: &str) -> semlex::Value { value }
//!     // pub type Kind<K> = …;   // change which values the attribute accepts
//! }
//! ```
//!
//! Hooks a module leaves out default to no-ops. See `docs/annotations.md`.
//!
//! PII attributes (`sensitivity: pii`) accept only `redacted(v)` or
//! `hashed(v)` values; see [`pii`].
//!
//! # `attrs!`
//!
//! ```ignore
//! let kvs: [KeyValue; 3] = crate::attrs![
//!     http.request.method: "GET",                    // literal: checked at compile time
//!     http.route: route,                             // expression: type-checked
//!     http.request.header("content-type"): vec!["text/plain"], // template attribute
//! ];
//! ```

pub use semlex_macros::{handler, registry};

/// The OpenTelemetry attribute value, for annotation handlers' `apply`.
pub use opentelemetry::Value;
/// The OpenTelemetry types call sites use most, so they don't need an
/// `opentelemetry` dependency for them: `Context` for `span!(…, parent: &cx)`,
/// and `KeyValue` from `attrs!` and `entity!`. (Not `Status`: its `Unset`
/// variant would make rustc spell out semlex's `Unset` in typestate errors.)
pub use opentelemetry::{Context, KeyValue};

// Lets this crate use its own `#[handler]` attribute, whose output names `::semlex`.
extern crate self as semlex;

pub mod entity;
pub mod handlers;
mod init;
pub mod log;
pub mod pii;
pub mod signal;
pub mod span;
#[cfg(feature = "testing")]
pub mod testing;
mod unregistered;

pub use entity::{AssociationViolation, Entity, check_associations, check_associations_with};
pub use init::{AlreadyInitialized, Config, init, is_initialized};
pub use span::StartedSpan;

/// Const helpers for annotation handlers' `check_literal`: string equality
/// and the edit-distance lookup semlex uses for enum suggestions.
pub use __private::rt as const_fns;

use opentelemetry::StringValue;
use std::borrow::Cow;
use std::sync::Arc;

/// A registry attribute: implemented by the marker type of each non-template
/// attribute that `registry!()` emits.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a plain attribute",
    label = "template attributes need a key suffix",
    note = "a template attribute takes a key suffix, e.g. `http.request.header(\"content-type\"): …`"
)]
pub trait Attribute: Copy {
    /// The dotted key, e.g. `http.request.method`.
    const KEY: &'static str;
    /// What values it takes: one of the [`kind`] markers, or a generated enum.
    type Kind;
    /// Runtime transform from annotation handlers (identity without handlers).
    fn apply(&self, value: Value) -> Value {
        value
    }
}

/// A template attribute (`template[T]`): a family of keys `<prefix>.<suffix>`.
pub trait TemplateAttribute: Copy {
    /// The key prefix, e.g. `http.request.header`.
    const PREFIX: &'static str;
    /// What values it takes.
    type Kind;
    /// Runtime transform from annotation handlers (identity without handlers).
    fn apply(&self, value: Value) -> Value {
        value
    }
}

/// A template attribute with its suffix, from `attr::….key("suffix")`.
#[derive(Debug, Clone)]
pub struct Templated<A> {
    /// The template attribute.
    pub attribute: A,
    /// The key suffix.
    pub suffix: Cow<'static, str>,
}

impl<A: TemplateAttribute> Templated<A> {
    /// The full key, `<prefix>.<suffix>`.
    pub fn key(&self) -> String {
        format!("{}.{}", A::PREFIX, self.suffix)
    }
}

/// Value kinds: what an attribute accepts.
pub mod kind {
    /// `string`: `&'static str`, `String`, `Cow<'static, str>`, `Arc<str>`.
    pub struct StringValue;
    /// `int`: Rust integer types up to 64-bit signed.
    pub struct IntValue;
    /// `double`: `f64`, `f32`.
    pub struct DoubleValue;
    /// `boolean`: `bool`.
    pub struct BoolValue;
    /// `string[]`: `Vec<String>`, `Vec<&'static str>`.
    pub struct StringArrayValue;
    /// `int[]`: `Vec<i64>`.
    pub struct IntArrayValue;
    /// `double[]`: `Vec<f64>`.
    pub struct DoubleArrayValue;
    /// `boolean[]`: `Vec<bool>`.
    pub struct BoolArrayValue;
    /// `any`: only valid on log records; no attribute value converts to it.
    pub struct AnyValue;
    /// `sensitivity: pii` on an attribute of kind `K`: the value must be
    /// wrapped with `redacted(…)` or `hashed(…)`.
    pub struct Pii<K>(core::marker::PhantomData<K>);
    /// `code_generation.rust.type: T` on an attribute of kind `K`: takes a
    /// `T`, converted with [`ToValue<K>`](crate::ToValue).
    pub struct Typed<K, T>(core::marker::PhantomData<(K, T)>);
}

/// Converts a Rust type into an attribute value of kind `K`, for attributes
/// annotated `code_generation.rust.type`. Implement it for your own types.
pub trait ToValue<K> {
    /// The OpenTelemetry value.
    fn to_value(self) -> Value;
}

impl<K, T: ToValue<K>> IntoValue<kind::Typed<K, T>> for T {
    fn into_value(self) -> Value {
        self.to_value()
    }
}

/// Seconds, as OpenTelemetry durations are.
impl ToValue<kind::DoubleValue> for std::time::Duration {
    fn to_value(self) -> Value {
        Value::F64(self.as_secs_f64())
    }
}

macro_rules! display_to_string_value {
    ($($t:ty),+) => {$(
        impl ToValue<kind::StringValue> for $t {
            fn to_value(self) -> Value {
                Value::from(self.to_string())
            }
        }
    )+};
}

display_to_string_value!(
    std::net::IpAddr,
    std::net::Ipv4Addr,
    std::net::Ipv6Addr,
    std::net::SocketAddr
);

/// Converts a Rust value into an attribute value of kind `K`.
///
/// Implemented for concrete types only (no blanket impl), so a wrong value
/// type reports this trait's message rather than a std conversion error.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a valid value here: this attribute takes a `{K}`",
    label = "expected a `{K}`",
    note = "`StringValue`: `&str`, `&String`, `String`, `Cow<'static, str>`, `Arc<str>` | `IntValue`: any integer up to 64 bits (`u64`/`usize` saturate at `i64::MAX`) | `DoubleValue`: `f64`, `f32` | `BoolValue`: `bool` | arrays: `Vec<…>` of those | enums: the enum, a string literal, or `custom(…)` | `Pii<…>` (`sensitivity: pii`): wrap the value in `redacted(…)` or `hashed(…)` | `Typed<K, T>` (`code_generation.rust.type`): pass a `T`"
)]
pub trait IntoValue<K> {
    /// The OpenTelemetry value.
    fn into_value(self) -> Value;
}

macro_rules! into_value {
    ($kind:ident: $($t:ty => |$v:ident| $e:expr),+ $(,)?) => {
        $(impl IntoValue<kind::$kind> for $t {
            fn into_value(self) -> Value {
                let $v = self;
                $e
            }
        })+
    };
}

// A borrowed string is copied; string literals don't come through here but
// through `__private::StaticStr`, which keeps them uncopied.
impl IntoValue<kind::StringValue> for &str {
    fn into_value(self) -> Value {
        Value::from(self.to_owned())
    }
}
impl IntoValue<kind::StringValue> for &String {
    fn into_value(self) -> Value {
        Value::from(self.clone())
    }
}
into_value!(StringValue:
    String => |v| Value::from(v),
    Cow<'static, str> => |v| Value::from(v),
    Arc<str> => |v| Value::from(v),
);
into_value!(IntValue:
    i64 => |v| Value::I64(v),
    i32 => |v| Value::I64(v.into()),
    i16 => |v| Value::I64(v.into()),
    i8 => |v| Value::I64(v.into()),
    u32 => |v| Value::I64(v.into()),
    u16 => |v| Value::I64(v.into()),
    u8 => |v| Value::I64(v.into()),
    // OpenTelemetry ints are `i64`: counts and sizes saturate at `i64::MAX`.
    u64 => |v| Value::I64(i64::try_from(v).unwrap_or(i64::MAX)),
    usize => |v| Value::I64(i64::try_from(v).unwrap_or(i64::MAX)),
    isize => |v| Value::I64(v as i64),
);
into_value!(DoubleValue:
    f64 => |v| Value::F64(v),
    f32 => |v| Value::F64(v.into()),
);
into_value!(BoolValue: bool => |v| Value::Bool(v));
into_value!(StringArrayValue:
    Vec<String> => |v| Value::Array(v.into_iter().map(StringValue::from).collect::<Vec<_>>().into()),
);
impl IntoValue<kind::StringArrayValue> for Vec<&str> {
    fn into_value(self) -> Value {
        Value::Array(
            self.into_iter()
                .map(|s| StringValue::from(s.to_owned()))
                .collect::<Vec<_>>()
                .into(),
        )
    }
}
into_value!(IntArrayValue: Vec<i64> => |v| Value::Array(v.into()));
into_value!(DoubleArrayValue: Vec<f64> => |v| Value::Array(v.into()));
into_value!(BoolArrayValue: Vec<bool> => |v| Value::Array(v.into()));

/// A value outside an enum's registry members, from `custom("…")`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Custom<T>(pub T);

/// Typestate marker: a required attribute has been set.
pub struct Set;
/// Typestate marker: a required attribute hasn't been set yet.
pub struct Unset;

/// Support for generated code and call-site expansions. Not public API.
#[doc(hidden)]
pub mod __private {
    pub use opentelemetry;
    pub use semlex_macros::{assert_signal, attrs, end, entity, event, log, metric, record, span};

    use super::{Attribute, IntoValue, TemplateAttribute, Templated};
    use opentelemetry::{Key, KeyValue};

    /// `custom(…)` at a call site.
    #[inline]
    pub const fn custom<T>(value: T) -> super::Custom<T> {
        super::Custom(value)
    }

    /// Recording that leaves the builder's state unchanged: the state after
    /// `with` (`Self`) is the state before (`T`). `key?: option` and
    /// `record!(&span, …)` need it: neither can mark an attribute as set.
    #[diagnostic::on_unimplemented(
        message = "this attribute is required and not set yet: `key?: option` and `record!(&span, …)` can't set it",
        label = "required, and not set yet",
        note = "give it a value (`key: value`); on a started span, `let span = record!(span, key: value)` returns the span with the attribute marked as set. `key?:` and `record!(&span, …)` work for other attributes, and for required ones once they're set"
    )]
    pub trait InPlace<T> {
        /// `self`, which is a `T`.
        fn into_same(self) -> T;
    }
    impl<T> InPlace<T> for T {
        fn into_same(self) -> T {
            self
        }
    }

    /// `key?: option`: records the attribute with the builder's own `with`
    /// if the value is `Some`, leaving the builder's state unchanged.
    pub fn with_opt<B, V, O: InPlace<B>>(
        builder: B,
        value: Option<V>,
        with: impl FnOnce(B, V) -> O,
    ) -> B {
        match value {
            Some(v) => with(builder, v).into_same(),
            None => builder,
        }
    }

    /// A string literal, recorded without copying it.
    #[derive(Debug, Clone, Copy)]
    pub struct StaticStr(pub &'static str);
    impl IntoValue<crate::kind::StringValue> for StaticStr {
        fn into_value(self) -> opentelemetry::Value {
            opentelemetry::Value::from(self.0)
        }
    }

    /// A literal for a string attribute: a string literal becomes a
    /// [`StaticStr`]; other literals pass through, so the value check
    /// reports a wrong kind (`5`) with its usual message.
    #[diagnostic::on_unimplemented(
        message = "`{Self}` is not a valid value here: this attribute takes a `StringValue`",
        label = "expected a `StringValue`"
    )]
    pub trait StringLiteral {
        /// The value passed on.
        type Out;
        /// Converts the literal.
        fn literal(self) -> Self::Out;
    }
    impl StringLiteral for &'static str {
        type Out = StaticStr;
        fn literal(self) -> StaticStr {
            StaticStr(self)
        }
    }
    macro_rules! passes_through {
        ($($t:ty),*) => {$(
            impl StringLiteral for $t {
                type Out = $t;
                fn literal(self) -> $t {
                    self
                }
            }
        )*};
    }
    passes_through!(i64, f64, bool);

    /// A signal builder's member keys, for `extra(…)`'s check.
    pub trait Members {
        /// The keys (template prefixes for template attributes).
        const MEMBERS: &'static [&'static str];
    }
    impl<T: Members> Members for &T {
        const MEMBERS: &'static [&'static str] = T::MEMBERS;
    }

    /// `record!(span, extra(key): …)`, where the signal is only known by the
    /// builder's type: [`check_extra`] when the call site is built.
    pub fn extra_on<B: Members, A: Attribute>(_: &B, _: &A) {
        const { check_extra(B::MEMBERS, A::KEY) }
    }

    /// [`extra_on`] for a template attribute.
    pub fn extra_template_on<B: Members, A: crate::TemplateAttribute>(_: &B, _: &A) {
        const { check_extra(B::MEMBERS, A::PREFIX) }
    }

    /// `extra(key)` on a signal that defines `key` (checked at compile time).
    pub const fn check_extra(members: &[&str], key: &str) {
        let mut i = 0;
        while i < members.len() {
            if rt::str_eq(members[i], key) {
                panic!(
                    "`extra(…)` is for attributes this signal doesn't define, and it defines this one: drop the `extra(…)`"
                );
            }
            i += 1;
        }
    }

    /// `end!`, after recording `error.type` and before the checked `end()`:
    /// the log-based `exception` record (with `exception.type` when the
    /// error's concrete type is known) and status `Error` with its message.
    pub fn fail<B>(span: &B, error: &dyn std::error::Error, error_type_name: Option<&'static str>)
    where
        B: std::ops::Deref<Target = crate::span::SpanCore>,
    {
        span.record_error_typed(error, error_type_name);
    }

    /// `end!`'s `error:` value, resolved by autoref specialization at the
    /// call site: `&E` for a concrete `E: Error` (with its type name), or a
    /// reference to a handle that derefs to `dyn Error` (`Box<dyn Error>`,
    /// `Arc<dyn Error + Send + Sync>`, `anyhow::Error` …), whose concrete
    /// type isn't known.
    pub struct ErrProbe<'a, T: ?Sized>(pub &'a T);

    /// `&E` with `E: Error`.
    pub trait ErrConcrete<'a> {
        /// The error and its type name.
        fn __semlex_error(self) -> (&'a dyn std::error::Error, Option<&'static str>);
    }
    impl<'a, T: std::error::Error + 'a> ErrConcrete<'a> for &&&ErrProbe<'a, T> {
        fn __semlex_error(self) -> (&'a dyn std::error::Error, Option<&'static str>) {
            (self.0, Some(std::any::type_name::<T>()))
        }
    }

    /// A handle to `dyn Error + Send + Sync`.
    pub trait ErrSendSync<'a> {
        /// The error, type unknown.
        fn __semlex_error(self) -> (&'a dyn std::error::Error, Option<&'static str>);
    }
    impl<'a, T> ErrSendSync<'a> for &&ErrProbe<'a, T>
    where
        T: ?Sized + std::ops::Deref<Target = dyn std::error::Error + Send + Sync + 'a>,
    {
        fn __semlex_error(self) -> (&'a dyn std::error::Error, Option<&'static str>) {
            (&**self.0, None)
        }
    }

    /// A handle to `dyn Error`.
    pub trait ErrDyn<'a> {
        /// The error, type unknown.
        fn __semlex_error(self) -> (&'a dyn std::error::Error, Option<&'static str>);
    }
    impl<'a, T> ErrDyn<'a> for &ErrProbe<'a, T>
    where
        T: ?Sized + std::ops::Deref<Target = dyn std::error::Error + 'a>,
    {
        fn __semlex_error(self) -> (&'a dyn std::error::Error, Option<&'static str>) {
            (&**self.0, None)
        }
    }

    /// One field of `event!`'s `tracing` fallback, resolved by autoref
    /// specialization at the call site: the attribute's value when its kind
    /// can be a `tracing` field ([`EventKind`]), otherwise left out. So an
    /// attribute the fallback can't carry never stops `event!` compiling.
    pub struct FieldProbe<'a, A>(pub &'a A, pub &'a [KeyValue]);

    /// A kind `tracing` can carry.
    pub trait FieldKnown {
        /// The field value.
        type Out: tracing::Value;
        /// The value, if the attribute was set.
        fn __semlex_field(self) -> Self::Out;
    }
    impl<A: Attribute> FieldKnown for &&FieldProbe<'_, A>
    where
        A::Kind: crate::log::EventKind,
    {
        type Out = Option<<A::Kind as crate::log::EventKind>::Field>;
        fn __semlex_field(self) -> Self::Out {
            crate::log::event_field(self.0, self.1)
        }
    }

    /// Any other kind: left out.
    pub trait FieldNone {
        /// Always `None`.
        type Out: tracing::Value;
        /// `None`.
        fn __semlex_field(self) -> Self::Out;
    }
    impl<A> FieldNone for &FieldProbe<'_, A> {
        type Out = Option<&'static str>;
        fn __semlex_field(self) -> Self::Out {
            None
        }
    }

    /// `end!(span, result: …, error.type: |e| class)`: calls the classifier
    /// with the error, so its parameter type is inferred.
    pub fn classify<E, C>(error: E, classifier: impl FnOnce(E) -> C) -> C {
        classifier(error)
    }

    /// `record!(&span, key: value)`: records through a shared reference.
    ///
    /// Called with method syntax, so auto-deref finds the span through any
    /// number of references: `&span` where `span` is the span, or a `&Ready`
    /// parameter. `check` is never called: it type-checks the span's own
    /// `with` (membership) and, through `InPlace`, that the state doesn't
    /// change.
    pub trait SpanInPlace {
        /// The span.
        fn __semlex_core(&self) -> &crate::span::SpanCore;

        /// Records `kv(value)` on the span; see the trait docs.
        fn __semlex_set_in_place<V, O>(
            &self,
            value: V,
            kv: impl FnOnce(V) -> KeyValue,
            check: fn(Self, V) -> O,
        ) where
            Self: Sized,
            O: InPlace<Self>,
        {
            let _ = check;
            self.__semlex_core().set(kv(value), crate::span::INTERNAL);
        }
    }

    impl<T: std::ops::Deref<Target = crate::span::SpanCore>> SpanInPlace for T {
        fn __semlex_core(&self) -> &crate::span::SpanCore {
            self
        }
    }

    pub use crate::entity::{Association, Associations};
    pub use crate::log::{EventKind, LogKind, event_field, key_of, log_field, text_field};
    pub use crate::pii::{hashed, redacted};
    pub use crate::signal::{EventSite, IntLiteral, IntoMeasurement, MetricSite, Scope, measure};
    pub use crate::span::{
        INTERNAL, Internal, NamePart, NameSpec, Named, NoName, SpanCore, SpanName, SpanSite,
        SpanType, StartOptions, named, record_current,
    };
    pub use crate::unregistered::{
        UnregisteredValue, check_unregistered, extra_attribute, unregistered_attribute,
        unregistered_field, unregistered_kv,
    };
    pub use tracing;

    /// One `attrs!` entry.
    #[inline]
    pub fn kv<A: Attribute, V: IntoValue<A::Kind>>(attribute: A, value: V) -> KeyValue {
        KeyValue::new(
            Key::from_static_str(A::KEY),
            attribute.apply(value.into_value()),
        )
    }

    /// One `attrs!` entry for a template attribute.
    #[inline]
    pub fn kv_template<A: TemplateAttribute, V: IntoValue<A::Kind>>(
        t: Templated<A>,
        value: V,
    ) -> KeyValue {
        KeyValue::new(Key::from(t.key()), t.attribute.apply(value.into_value()))
    }

    /// Const helpers for literal checks.
    pub mod rt {
        /// Byte-wise string equality in const contexts.
        pub const fn str_eq(a: &str, b: &str) -> bool {
            let (a, b) = (a.as_bytes(), b.as_bytes());
            if a.len() != b.len() {
                return false;
            }
            let mut i = 0;
            while i < a.len() {
                if a[i] != b[i] {
                    return false;
                }
                i += 1;
            }
            true
        }

        /// Longest string the edit-distance search considers.
        pub const MAX_LEN: usize = 64;

        /// Optimal string alignment distance (Levenshtein plus adjacent
        /// transpositions), ASCII case-insensitive. `usize::MAX` for inputs
        /// longer than [`MAX_LEN`].
        pub const fn distance(a: &str, b: &str) -> usize {
            let (a, b) = (a.as_bytes(), b.as_bytes());
            if a.len() > MAX_LEN || b.len() > MAX_LEN {
                return usize::MAX;
            }
            let mut pp = [0usize; MAX_LEN + 1];
            let mut prev = [0usize; MAX_LEN + 1];
            let mut cur = [0usize; MAX_LEN + 1];
            let mut j = 0;
            while j <= b.len() {
                prev[j] = j;
                j += 1;
            }
            let mut i = 1;
            while i <= a.len() {
                cur[0] = i;
                let mut j = 1;
                while j <= b.len() {
                    let cost = if a[i - 1].eq_ignore_ascii_case(&b[j - 1]) {
                        0
                    } else {
                        1
                    };
                    let mut m = prev[j] + 1;
                    if cur[j - 1] + 1 < m {
                        m = cur[j - 1] + 1;
                    }
                    if prev[j - 1] + cost < m {
                        m = prev[j - 1] + cost;
                    }
                    if i > 1
                        && j > 1
                        && a[i - 1].eq_ignore_ascii_case(&b[j - 2])
                        && a[i - 2].eq_ignore_ascii_case(&b[j - 1])
                        && pp[j - 2] + 1 < m
                    {
                        m = pp[j - 2] + 1;
                    }
                    cur[j] = m;
                    j += 1;
                }
                let mut k = 0;
                while k <= b.len() {
                    pp[k] = prev[k];
                    prev[k] = cur[k];
                    k += 1;
                }
                i += 1;
            }
            prev[b.len()]
        }

        /// Result of looking a literal up among an enum's wire values.
        pub enum Lookup {
            /// The value is member `.0`.
            Found(usize),
            /// Not a member; member `.0` is close enough to be a likely typo.
            Closest(usize),
            /// Not a member, nothing close.
            Unknown,
        }

        /// Looks `value` up in `members` (wire values).
        pub const fn lookup(value: &str, members: &[&str]) -> Lookup {
            let mut best = usize::MAX;
            let mut best_i = 0;
            let mut i = 0;
            while i < members.len() {
                if str_eq(value, members[i]) {
                    return Lookup::Found(i);
                }
                let d = distance(value, members[i]);
                if d < best {
                    best = d;
                    best_i = i;
                }
                i += 1;
            }
            if members.is_empty() {
                return Lookup::Unknown;
            }
            // Up to a third of the member's length, rounded up, counts as a typo.
            if best <= members[best_i].len().div_ceil(3) {
                Lookup::Closest(best_i)
            } else {
                Lookup::Unknown
            }
        }

        /// Position of `value` in `members`, if present.
        pub const fn position_i64(value: i64, members: &[i64]) -> Option<usize> {
            let mut i = 0;
            while i < members.len() {
                if members[i] == value {
                    return Some(i);
                }
                i += 1;
            }
            None
        }
    }

    #[cfg(test)]
    mod tests {
        use super::rt::*;

        #[test]
        fn suggestions() {
            let m = ["CONNECT", "DELETE", "GET", "POST", "PUT", "_OTHER"];
            let closest = |v: &str| match lookup(v, &m) {
                Lookup::Closest(i) => Some(m[i]),
                _ => None,
            };
            assert!(matches!(lookup("GET", &m), Lookup::Found(2)));
            assert_eq!(closest("GTE"), Some("GET"));
            assert_eq!(closest("get"), Some("GET"));
            assert_eq!(closest("PSOT"), Some("POST"));
            assert_eq!(closest("DELTE"), Some("DELETE"));
            assert_eq!(closest("OTHER"), Some("_OTHER"));
            assert!(matches!(lookup("BREW", &m), Lookup::Unknown));
        }
    }
}
