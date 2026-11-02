//! Runtime support for `span!` and `record!`.
//!
//! A started span lives in a [`Context`]; the generated typestate `Span`
//! types wrap a [`SpanCore`] and operate on it. Dropping a core that hasn't
//! ended ends it, as a fallback.

use crate::init;
use crate::signal::Scope;
use opentelemetry::global::BoxedTracer;
use opentelemetry::trace::{Link, SpanKind, SpanRef, Status, TraceContextExt, Tracer};
use opentelemetry::{Context, KeyValue};
use std::borrow::Cow;
use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;

/// A piece of a span-name template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamePart {
    /// Literal text.
    Lit(&'static str),
    /// An attribute's value.
    Attr(&'static str),
}

/// How a span type is named, from the registry's `name.templates`.
#[derive(Debug)]
pub struct NameSpec {
    /// Templates in order; the first whose attributes are all present,
    /// non-empty and not `_OTHER` wins.
    pub templates: &'static [&'static [NamePart]],
    /// Keys the templates reference.
    pub keys: &'static [&'static str],
    /// Whether a template references an attribute that may arrive after the
    /// span starts, so the name must be re-evaluated at `end()`.
    pub can_change: bool,
    /// The name when no template applies (the span type).
    pub fallback: &'static str,
}

/// Evaluates templates against attribute values (V2 span-name algorithm).
pub fn evaluate(
    templates: &[&[NamePart]],
    lookup: impl Fn(&str) -> Option<String>,
) -> Option<String> {
    'templates: for t in templates {
        let mut name = String::new();
        for part in *t {
            match part {
                NamePart::Lit(s) => name.push_str(s),
                NamePart::Attr(key) => match lookup(key) {
                    Some(v) if !v.is_empty() && v != "_OTHER" => name.push_str(&v),
                    _ => continue 'templates,
                },
            }
        }
        return Some(name);
    }
    None
}

/// A span call site's tracer.
///
/// After `semlex::init` the tracer is created once. Without `init` (library
/// mode) each span gets its tracer from the global tracer provider as it is
/// now, so spans follow whatever provider the application has installed.
pub struct SpanSite {
    tracer: OnceLock<BoxedTracer>,
    observed: AtomicBool,
}

/// The tracer for one span start: the call site's cached one, or one from
/// the current global provider.
enum SiteTracer<'a> {
    Cached(&'a BoxedTracer),
    Global(BoxedTracer),
}

impl std::ops::Deref for SiteTracer<'_> {
    type Target = BoxedTracer;
    fn deref(&self) -> &BoxedTracer {
        match self {
            SiteTracer::Cached(t) => t,
            SiteTracer::Global(t) => t,
        }
    }
}

impl Default for SpanSite {
    fn default() -> Self {
        Self::new()
    }
}

impl SpanSite {
    /// An empty site (one `static` per call site).
    pub const fn new() -> Self {
        Self {
            tracer: OnceLock::new(),
            observed: AtomicBool::new(false),
        }
    }

    fn tracer(
        &self,
        scope: &Scope,
        associations: &'static crate::entity::Associations,
    ) -> SiteTracer<'_> {
        if init::owns_tracer_provider() {
            return SiteTracer::Cached(self.tracer.get_or_init(|| {
                crate::entity::observe(associations);
                opentelemetry::global::tracer_with_scope(scope.instrumentation_scope())
            }));
        }
        // The application owns the global tracer provider: follow it.
        if !self.observed.load(Ordering::Relaxed) && !self.observed.swap(true, Ordering::Relaxed) {
            crate::entity::observe(associations);
        }
        SiteTracer::Global(opentelemetry::global::tracer_with_scope(
            scope.instrumentation_scope(),
        ))
    }
}

/// `span!` options other than the attributes.
#[derive(Default)]
pub struct StartOptions {
    parent: Option<Context>,
    links: Vec<Link>,
    start_time: Option<SystemTime>,
}

impl StartOptions {
    /// No options: the current context is the parent.
    pub fn new() -> Self {
        Self::default()
    }
    /// `parent: &cx`.
    pub fn parent(mut self, cx: &Context) -> Self {
        self.parent = Some(cx.clone());
        self
    }
    /// `links: [...]`.
    pub fn links(mut self, links: impl IntoIterator<Item = Link>) -> Self {
        self.links = links.into_iter().collect();
        self
    }
    /// `start_time: t`.
    pub fn start_time(mut self, t: SystemTime) -> Self {
        self.start_time = Some(t);
        self
    }
}

/// `name: …` given to `span!`.
pub struct Named(pub Cow<'static, str>);

/// No `name:` given to `span!`.
pub struct NoName;

/// The explicit name, if any.
pub trait SpanName {
    /// The name.
    fn into_name(self) -> Option<Cow<'static, str>>;
}

impl SpanName for Named {
    fn into_name(self) -> Option<Cow<'static, str>> {
        Some(self.0)
    }
}

impl SpanName for NoName {
    fn into_name(self) -> Option<Cow<'static, str>> {
        None
    }
}

/// `name: expr` at a call site.
pub fn named(name: impl Into<Cow<'static, str>>) -> Named {
    Named(name.into())
}

/// What a span type is, for [`SpanCore::start`].
#[derive(Debug)]
pub struct SpanType {
    /// The span type id, e.g. `http.server.request`.
    pub id: &'static str,
    /// Span kind.
    pub kind: SpanKind,
    /// Whether the span type is opt-in.
    pub opt_in: bool,
    /// Naming rules.
    pub names: NameSpec,
    /// Entity associations.
    pub associations: crate::entity::Associations,
}

struct NameTracker {
    spec: &'static NameSpec,
    current: String,
    values: Vec<(&'static str, String)>,
}

/// A started span, inside a [`Context`]. Inactive (a disabled opt-in span)
/// means it records nothing and never touches the parent; its children parent
/// to the current context.
pub struct SpanCore {
    /// The context with this span in it (active), or an empty one (inactive).
    own: Context,
    /// The context children should use.
    children: Context,
    active: bool,
    ended: AtomicBool,
    name: Option<Mutex<NameTracker>>,
    /// The call site's scope, for the span's `exception` events.
    scope: Scope,
}

impl SpanCore {
    /// Starts a span with the attributes set in `span!`, so samplers see them.
    pub fn start(
        site: &'static SpanSite,
        scope: &Scope,
        ty: &'static SpanType,
        attributes: Vec<KeyValue>,
        options: StartOptions,
        name: Option<Cow<'static, str>>,
    ) -> Self {
        let tracer =
            init::signal_enabled(ty.id, ty.opt_in).then(|| site.tracer(scope, &ty.associations));
        let Some(tracer) = tracer else {
            return Self {
                own: Context::new(),
                children: Context::current(),
                scope: *scope,
                active: false,
                ended: AtomicBool::new(false),
                name: None,
            };
        };
        let lookup = |key: &str| {
            attributes
                .iter()
                .find(|kv| kv.key.as_str() == key)
                .map(|kv| kv.value.as_str().into_owned())
        };
        let explicit = name.is_some();
        let span_name: Cow<'static, str> = match name {
            Some(n) => n,
            None => evaluate(ty.names.templates, lookup)
                .map(Cow::Owned)
                .unwrap_or(Cow::Borrowed(ty.names.fallback)),
        };
        let tracker = (!explicit && ty.names.can_change).then(|| NameTracker {
            spec: &ty.names,
            current: span_name.to_string(),
            values: ty
                .names
                .keys
                .iter()
                .filter_map(|k| {
                    attributes
                        .iter()
                        .find(|kv| kv.key.as_str() == *k)
                        .map(|kv| (*k, kv.value.as_str().into_owned()))
                })
                .collect(),
        });
        let mut builder = tracer
            .span_builder(span_name)
            .with_kind(ty.kind.clone())
            .with_attributes(attributes);
        if !options.links.is_empty() {
            builder = builder.with_links(options.links);
        }
        if let Some(t) = options.start_time {
            builder = builder.with_start_time(t);
        }
        let parent = options.parent.unwrap_or_else(Context::current);
        let span = builder.start_with_context(&*tracer, &parent);
        let own = parent.with_span(span);
        Self {
            children: own.clone(),
            own,
            scope: *scope,
            active: true,
            ended: AtomicBool::new(false),
            name: tracker.map(Mutex::new),
        }
    }

    /// Sets an attribute (from `record!`). Takes [`Internal`] so that only
    /// the macros' expansions call it: a started span derefs to `SpanCore`,
    /// and an open `set` would record any key and value unchecked.
    pub fn set(&self, kv: KeyValue, _: Internal) {
        if !self.active {
            return;
        }
        if let Some(t) = &self.name
            && let Ok(mut t) = t.lock()
            && let Some(k) = t.spec.keys.iter().find(|k| **k == kv.key.as_str())
        {
            t.values.retain(|(key, _)| key != k);
            t.values.push((k, kv.value.as_str().into_owned()));
        }
        self.own.span().set_attribute(kv);
    }

    /// Ends the span, re-evaluating its name if attributes it depends on
    /// arrived after start. Takes [`Internal`]: the typed `end()` checks the
    /// required attributes, this doesn't.
    pub fn end(&self, at: Option<SystemTime>, _: Internal) {
        if self.ended.swap(true, Ordering::AcqRel) {
            return;
        }
        if !self.active {
            return;
        }
        let span = self.own.span();
        if let Some(t) = &self.name
            && let Ok(t) = t.lock()
        {
            let lookup = |key: &str| {
                t.values
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| v.clone())
            };
            if let Some(name) = evaluate(t.spec.templates, lookup)
                && name != t.current
            {
                span.update_name(name);
            }
        }
        match at {
            Some(t) => span.end_with_timestamp(t),
            None => span.end(),
        }
    }

    /// Records the error (see [`record_error`](Self::record_error)) and ends
    /// the span.
    pub fn end_with_error(&self, error: &dyn Error) {
        if !self.ended.load(Ordering::Acquire) {
            self.record_error_typed(error, None);
        }
        self.end(None, INTERNAL);
    }

    /// The context for child spans (and for `Context::attach`).
    pub fn child_context(&self) -> Context {
        self.children.clone()
    }

    /// Runs `f` with this span as the current span, so spans started in it
    /// are its children and `record!(current, …)` records on it. For async
    /// code, use `opentelemetry::trace::FutureExt::with_context(fut,
    /// span.child_context())` instead: a guard held across `.await` would
    /// leak the context to other tasks.
    pub fn in_scope<T>(&self, f: impl FnOnce() -> T) -> T {
        let _guard = self.child_context().attach();
        f()
    }

    /// Sets the span status.
    pub fn set_status(&self, status: Status) {
        if self.active {
            self.own.span().set_status(status);
        }
    }

    /// Records an error without ending the span: the conventions' log-based
    /// `exception` event in this span's trace context (`exception.message`,
    /// and the source chain in `exception.stacktrace`; span events are
    /// deprecated in OpenTelemetry), and status `Error` with the error's
    /// message, like `end!` and `end_with_error`. If the operation recovers
    /// (e.g. a retry succeeds), set `Status::Ok`, which overrides `Error`.
    pub fn record_error(&self, error: &dyn Error) {
        self.record_error_typed(error, None);
    }

    /// The one error path behind `record_error`, `end_with_error` and `end!`:
    /// the `exception` record (with `exception.type` when known) and status
    /// `Error` with the message.
    pub(crate) fn record_error_typed(&self, error: &dyn Error, ty: Option<&'static str>) {
        if !self.active {
            return;
        }
        let message = error.to_string();
        let mut chain = String::new();
        let mut source = error.source();
        let mut i = 0;
        while let Some(e) = source {
            if i == 0 {
                chain.push_str(&message);
                chain.push_str("\n\nCaused by:");
            }
            use std::fmt::Write as _;
            let _ = write!(chain, "\n    {i}: {e}");
            i += 1;
            source = e.source();
        }
        let stacktrace = (!chain.is_empty()).then_some(chain);
        {
            // The record joins this span's trace.
            let _in_span = self.own.clone().attach();
            crate::log::exception(&self.scope, ty, message.clone(), stacktrace);
        }
        self.own.span().set_status(Status::error(message));
    }

    /// Adds a span event. Span events are deprecated in OpenTelemetry: use
    /// `event!` (log-based events, in this span's context when it's current),
    /// which the registry defines and live check can match.
    #[deprecated(
        note = "span events are deprecated in OpenTelemetry; emit a log-based event with `event!` under `span.child_context()`"
    )]
    pub fn add_event(&self, name: impl Into<Cow<'static, str>>, attributes: Vec<KeyValue>) {
        if self.active {
            self.own.span().add_event(name, attributes);
        }
    }

    /// The underlying OpenTelemetry span, for anything semlex doesn't cover.
    /// Borrowed: it can't change the typestate. A no-op span when inactive.
    pub fn otel_mut(&self) -> SpanRef<'_> {
        self.own.span()
    }

    /// Leaves semlex: the context holding the span, which is no longer ended
    /// on drop. Required attributes are no longer checked.
    pub fn into_otel(mut self) -> Context {
        self.ended.store(true, Ordering::Release);
        std::mem::take(&mut self.own)
    }
}

impl Drop for SpanCore {
    fn drop(&mut self) {
        self.end(None, INTERNAL);
    }
}

/// A token for `SpanCore`'s unchecked operations ([`SpanCore::set`],
/// [`SpanCore::end`]), so that only the macros' expansions reach them.
#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct Internal(());

/// The [`Internal`] token, for generated code.
#[doc(hidden)]
pub const INTERNAL: Internal = Internal(());

/// Any started span, whatever its type and whichever attributes are set:
/// what a shared helper can do with a span it didn't start.
///
/// A started span's type spells out which required attributes are set, so
/// helpers can't name it; take `impl StartedSpan` instead:
///
/// ```ignore
/// fn fail(span: impl semlex::StartedSpan, error: &std::io::Error) {
///     crate::error!(group exception { exception.message: error.to_string() }, "failed");
///     span.end_with_error(error);
/// }
/// ```
///
/// There's no successful `end()` here: it's only callable once the registry's
/// required attributes are set, which only the span's own type can show.
/// Return the span to the caller to end it, or call `end_with_error`.
pub trait StartedSpan {
    /// The context for child spans (and for `Context::attach`).
    fn child_context(&self) -> Context;
    /// Runs `f` with this span as the current span (see
    /// [`SpanCore::in_scope`]).
    fn in_scope<T>(&self, f: impl FnOnce() -> T) -> T
    where
        Self: Sized;
    /// Sets the span status.
    fn set_status(&self, status: Status);
    /// Records an error without ending the span: the log-based `exception`
    /// event and status `Error` with the error's message.
    fn record_error(&self, error: &dyn Error);
    /// Records the error, sets an error status and ends the span (required
    /// attributes aren't checked on error paths).
    fn end_with_error(self, error: &dyn Error)
    where
        Self: Sized;
}

impl<T: std::ops::Deref<Target = SpanCore>> StartedSpan for T {
    fn child_context(&self) -> Context {
        SpanCore::child_context(self)
    }
    fn in_scope<R>(&self, f: impl FnOnce() -> R) -> R {
        SpanCore::in_scope(self, f)
    }
    fn set_status(&self, status: Status) {
        SpanCore::set_status(self, status);
    }
    fn record_error(&self, error: &dyn Error) {
        SpanCore::record_error(self, error);
    }
    fn end_with_error(self, error: &dyn Error) {
        SpanCore::end_with_error(&self, error);
    }
}

/// `record!(current, …)`: sets attributes on the current span (registry-wide
/// checks only; no group membership).
pub fn record_current(attributes: impl IntoIterator<Item = KeyValue>) {
    let cx = Context::current();
    let span = cx.span();
    for kv in attributes {
        span.set_attribute(kv);
    }
}

#[cfg(test)]
mod tests {
    use super::NamePart::{Attr, Lit};
    use super::*;

    const HTTP: &[&[NamePart]] = &[
        &[Attr("http.request.method"), Lit(" "), Attr("http.route")],
        &[Attr("http.request.method")],
        &[Lit("HTTP")],
    ];

    fn eval(values: &[(&str, &str)]) -> Option<String> {
        evaluate(HTTP, |k| {
            values
                .iter()
                .find(|(key, _)| *key == k)
                .map(|(_, v)| v.to_string())
        })
    }

    #[test]
    fn first_complete_template_wins() {
        assert_eq!(
            eval(&[
                ("http.request.method", "GET"),
                ("http.route", "/users/{id}")
            ])
            .unwrap(),
            "GET /users/{id}"
        );
        assert_eq!(eval(&[("http.request.method", "GET")]).unwrap(), "GET");
    }

    #[test]
    fn other_and_empty_values_skip_a_template() {
        assert_eq!(
            eval(&[("http.request.method", "_OTHER"), ("http.route", "/x")]).unwrap(),
            "HTTP"
        );
        assert_eq!(
            eval(&[("http.request.method", "GET"), ("http.route", "")]).unwrap(),
            "GET"
        );
        assert_eq!(eval(&[]).unwrap(), "HTTP");
    }

    #[test]
    fn no_template_applies() {
        assert_eq!(evaluate(&[&[Attr("a")]], |_| None), None);
    }
}
