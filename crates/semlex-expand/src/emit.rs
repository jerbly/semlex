//! What `registry!()` emits.
//!
//! All paths in the output start from `crate::` (the registry crate root, where
//! `registry!()` must be invoked) or `::semlex::` (the runtime).

use crate::annotations::{self, HandlerCall};
use crate::hook::{AnnotationHook, Emit};
use crate::options::Options;
use proc_macro2::{Ident, Span, TokenStream};
use quote::{format_ident, quote};
use semlex_schema::model::{Deprecated, EnumValue, Stability};
use semlex_schema::names::Names;
use semlex_schema::{
    AttributeDef, Group, GroupKindTag, Primitive, Registry, Requirement, ValueType,
};
use std::collections::BTreeMap;

/// An identifier from a mangled name, which may be raw (`r#type`).
pub(crate) fn ident(name: &str) -> Ident {
    match name.strip_prefix("r#") {
        Some(raw) => Ident::new_raw(raw, Span::call_site()),
        None => Ident::new(name, Span::call_site()),
    }
}

/// `r#type` → `type`, for building derived identifiers.
fn bare(name: &str) -> &str {
    name.strip_prefix("r#").unwrap_or(name)
}

fn level(s: Stability) -> &'static str {
    match s {
        Stability::Development => "development",
        Stability::Alpha => "alpha",
        Stability::Beta => "beta",
        Stability::ReleaseCandidate => "release_candidate",
        Stability::Stable => "stable",
    }
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").trim().to_string()
}

/// Text for a `#[doc = …]` attribute: a leading space, as `///` comments have.
fn doc(s: &str) -> String {
    format!(" {}", first_line(s))
}

fn deprecation_note(d: &Deprecated) -> String {
    let with = |head: String, note: &Option<String>| match note {
        Some(n) if !n.trim().is_empty() && n.trim() != head => format!("{head}: {}", first_line(n)),
        _ => head,
    };
    match d {
        // Weaver fills in "Replaced by `<renamed_to>`." when the note is
        // omitted; don't repeat the target.
        Deprecated::Renamed { renamed_to, note } => {
            let note = note
                .as_ref()
                .filter(|n| !n.contains(renamed_to.as_str()))
                .cloned();
            with(format!("renamed to `{renamed_to}`"), &note)
        }
        Deprecated::Obsoleted { note } => with("obsoleted".into(), note),
        Deprecated::Uncategorized { note } | Deprecated::Unspecified { note } => {
            with("deprecated".into(), note)
        }
    }
}

fn deprecated_attr(d: &Option<Deprecated>) -> TokenStream {
    match d {
        Some(d) => {
            let note = deprecation_note(d);
            quote! { #[deprecated(note = #note)] }
        }
        None => quote! {},
    }
}

/// Which items are emitted.
struct Selection<'a> {
    attributes: BTreeMap<&'a str, &'a AttributeDef>,
}

impl<'a> Selection<'a> {
    fn new(reg: &'a Registry, opts: &Options) -> Self {
        let keep = |s: Stability| opts.min_stability.is_none_or(|min| s >= min);
        let attributes = reg
            .attributes
            .iter()
            .filter(|a| {
                keep(a.stability) && !a.annotations.code_generation_exclude().unwrap_or(false)
            })
            .map(|a| (a.key.as_str(), a))
            .collect();
        Selection { attributes }
    }
}

/// A module tree keyed by mangled names.
#[derive(Default)]
pub(crate) struct Tree {
    pub(crate) items: TokenStream,
    children: BTreeMap<String, Tree>,
}

impl Tree {
    pub(crate) fn at(&mut self, path: &[String]) -> &mut Tree {
        let mut t = self;
        for p in path {
            t = t.children.entry(p.clone()).or_default();
        }
        t
    }

    /// Renders the tree; `what` describes the items, e.g. "Attributes".
    fn render(self, what: &str, prefix: &str) -> TokenStream {
        self.render_with(what, prefix, &quote!(pub), &quote!())
    }

    /// Renders the tree, with `vis` and `attrs` on its top-level modules.
    fn render_with(
        self,
        what: &str,
        prefix: &str,
        vis: &TokenStream,
        attrs: &TokenStream,
    ) -> TokenStream {
        let items = self.items;
        let children = self.children.into_iter().map(|(name, child)| {
            let dotted = if prefix.is_empty() {
                bare(&name).to_string()
            } else {
                format!("{prefix}.{}", bare(&name))
            };
            let doc = format!(" {what} under `{dotted}`.");
            let body = child.render(what, &dotted);
            let name = ident(&name);
            quote! { #[doc = #doc] #attrs #vis mod #name { #body } }
        });
        quote! { #items #(#children)* }
    }
}

/// Everything `registry!()` emits for a loaded registry.
/// A problem found while emitting: a message, and the option it belongs to
/// (`None`: the registry itself).
pub(crate) type EmitError = (Option<Span>, String);

pub(crate) fn registry(
    reg: &Registry,
    names: &Names,
    opts: &Options,
    org_crate: &str,
    hooks: &[&dyn AnnotationHook],
) -> Result<TokenStream, Vec<EmitError>> {
    let mut errors: Vec<EmitError> = annotations::validate(reg, opts, hooks);
    let mut handler_paths: BTreeMap<String, TokenStream> = BTreeMap::new();
    let sel = Selection::new(reg, opts);
    let unstable_hint =
        format!("enable the `semlex-unstable` feature of `{org_crate}` to use non-stable items");
    let any_unstable = sel
        .attributes
        .values()
        .any(|a| a.stability != Stability::Stable);

    let mut markers = TokenStream::new();
    let mut enums = TokenStream::new();
    let mut attr_tree = Tree::default();
    for (key, a) in &sel.attributes {
        let n = &names.attributes[*key];
        let marker = ident(&n.marker);
        let leaf = ident(&n.leaf);
        let brief = doc(&a.brief);
        let dep = deprecated_attr(&a.deprecated);

        let usable = if a.stability == Stability::Stable {
            quote! {}
        } else {
            let msg = format!(
                "attribute `{key}` is {}; {unstable_hint}",
                level(a.stability)
            );
            quote! { if !crate::__semlex_registry::ALLOW_UNSTABLE { panic!(#msg) } }
        };

        let handlers = annotations::handlers_for(key, &a.annotations, opts).unwrap_or_else(|e| {
            errors.push((None, e));
            Vec::new()
        });
        for h in &handlers {
            handler_paths
                .entry(h.path.to_string())
                .or_insert_with(|| h.path.clone());
        }
        let rust_type: Option<syn::Type> = match a.annotations.rust_type() {
            Ok(Some(t)) => match syn::parse_str(t) {
                Ok(ty) => Some(ty),
                Err(_) => {
                    errors.push((None, format!("attribute `{key}`: `code_generation.rust.type` `{t}` is not a Rust type")));
                    None
                }
            },
            Ok(None) => None,
            Err(e) => {
                errors.push((None, format!("attribute `{key}`: {e}")));
                None
            }
        };
        // Kind: base, then `rust.type`, then each handler's `Kind<…>`.
        let wrap = |base: TokenStream| {
            let mut k = base;
            if let Some(t) = &rust_type {
                k = quote!(::semlex::kind::Typed<#k, #t>);
            }
            for h in &handlers {
                let p = &h.path;
                k = quote!(#p::Kind<#k>);
            }
            k
        };
        let apply = apply_fn(&handlers);

        let (attr_impl, hooks) = match &a.ty {
            ValueType::Template(inner) => {
                let kind = wrap(kind_tokens(inner, None));
                (
                    quote! {
                        impl ::semlex::TemplateAttribute for #marker {
                            const PREFIX: &'static str = #key;
                            type Kind = #kind;
                            #apply
                        }
                    },
                    quote! {
                        /// This template attribute with a key suffix: `<prefix>.<suffix>`.
                        pub fn key(&self, suffix: impl Into<::std::borrow::Cow<'static, str>>) -> ::semlex::Templated<Self> {
                            ::semlex::Templated { attribute: *self, suffix: suffix.into() }
                        }
                    },
                )
            }
            ty => {
                let enum_name = n.enumeration.as_ref().map(|e| ident(&e.name));
                let kind = wrap(kind_tokens(ty, enum_name.as_ref()));
                let checks: TokenStream = handlers
                    .iter()
                    .map(|h| {
                        let (p, arg) = (&h.path, &h.arg);
                        quote! { #p::check_literal(value, #arg); }
                    })
                    .collect();
                // Only a plain string kind takes the uncopied `StaticStr`.
                let plain_kind = handlers.is_empty() && rust_type.is_none();
                let hooks = literal_hooks(
                    key,
                    ty,
                    enum_name.as_ref(),
                    &unstable_hint,
                    opts,
                    &checks,
                    plain_kind,
                );
                (
                    quote! {
                        impl ::semlex::Attribute for #marker {
                            const KEY: &'static str = #key;
                            type Kind = #kind;
                            #apply
                        }
                    },
                    hooks,
                )
            }
        };

        let bounded = match a.annotations.cardinality() {
            Ok(Some(semlex_schema::annotations::Cardinality::Unbounded)) => {
                let msg = format!(
                    "`{key}` is unbounded (annotation `cardinality: unbounded`) and can't be an attribute of a metric"
                );
                quote! { panic!(#msg) }
            }
            _ => quote! {},
        };
        markers.extend(quote! {
            #[doc = #brief]
            #[derive(Clone, Copy, Debug)]
            pub struct #marker;
            #attr_impl
            impl #marker {
                /// Compile-time check that this attribute may be used here.
                pub const fn check_usable(&self) { #usable }
                /// Compile-time check that this attribute may be added to a
                /// metric with `extra(…)`.
                pub const fn check_bounded(&self) { #bounded }
                #hooks
            }
        });

        if let (ValueType::Enum(def), Some(en)) = (&a.ty, &n.enumeration) {
            enums.extend(enum_type(key, a, def, en, opts));
        }

        attr_tree.at(&n.module).items.extend(quote! {
            #[doc = #brief]
            #dep
            pub const #leaf: crate::attr_types::#marker = crate::attr_types::#marker;
        });
    }

    for c in &opts.conversions {
        match (sel.attributes.get(c.key.as_str()), names.attributes.get(&c.key).and_then(|n| n.enumeration.as_ref())) {
            (Some(a), Some(en)) => enums.extend(conversion(&c.ty, a, en)),
            (Some(_), None) => errors.push((Some(c.span), format!("`conversions`: `{}` isn't an enum attribute; for other attributes use `code_generation.rust.type`", c.key))),
            (None, _) => errors.push((Some(c.span), format!("`conversions`: no attribute `{}` is emitted from this registry", c.key))),
        }
    }
    // First in the module, so a declared handler that isn't marked
    // `#[semlex::handler]` gets this self-explanatory error first.
    let handler_checks = handler_paths
        .values()
        .map(|p| quote! { const _: () = #p::__MARK_THIS_MODULE_WITH_SEMLEX_HANDLER; });
    markers = quote! { #(#handler_checks)* #markers };

    // `visibility: crate`: modules visible in this crate only; unused
    // generated items are expected, not dead code.
    let (vis, allow) = if opts.crate_only {
        (
            quote!(pub(crate)),
            quote!(#[allow(dead_code, unused_imports)]),
        )
    } else {
        (quote!(pub), quote!())
    };
    let groups = groups(reg, names, &sel, opts, &vis, &allow);
    let hook_items = run_hooks(reg, names, &sel, opts, hooks, &mut errors, &vis, &allow);
    let forwarders = forwarders(opts.crate_only);
    let attr_modules = attr_tree.render("Attributes", "");
    let keys: Vec<&str> = reg
        .attributes
        .iter()
        .filter(|a| !a.annotations.code_generation_exclude().unwrap_or(false))
        .map(|a| a.key.as_str())
        .collect();
    let template_prefixes: Vec<&str> = reg
        .attributes
        .iter()
        .filter(|a| matches!(a.ty, ValueType::Template(_)))
        .map(|a| a.key.as_str())
        .collect();
    let schema_url = &reg.schema_url;
    let scope_prefix = &opts.scope_prefix;
    let entity_identity = entity_identities(reg);
    let allow_unstable = if opts.allow_unstable {
        quote! { true }
    } else if any_unstable {
        quote! { cfg!(feature = "semlex-unstable") }
    } else {
        quote! { false }
    };

    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(quote! {
        #[doc(hidden)]
        #allow
        #vis use ::semlex as __semlex;

        #[doc(hidden)]
        #allow
        #vis mod __semlex_registry {
            /// Every attribute key in the registry, sorted.
            pub const ATTRIBUTE_KEYS: &[&str] = &[#(#keys),*];
            /// Prefixes of the registry's template attributes.
            pub const TEMPLATE_PREFIXES: &[&str] = &[#(#template_prefixes),*];
            /// The registry's schema URL.
            pub const SCHEMA_URL: &str = #schema_url;
            /// Prefix for call sites' instrumentation scope names.
            pub const SCOPE_PREFIX: &str = #scope_prefix;
            /// Whether the registry crate's `semlex-unstable` feature is enabled.
            pub const ALLOW_UNSTABLE: bool = #allow_unstable;
            #entity_identity
        }

        /// Attribute marker types, with their hooks.
        #[allow(non_camel_case_types)]
        #allow
        #vis mod attr_types {
            #markers
        }

        /// Enum attribute value types.
        #[allow(deprecated)]
        #allow
        #vis mod enums {
            #enums
        }

        /// Registry attributes, by dotted name: `attr::http::request::method`.
        #[allow(non_upper_case_globals)]
        #allow
        #vis mod attr {
            #attr_modules
        }

        #groups

        #hook_items

        #forwarders
    })
}

/// The registry crate's call-site macros: (name, doc, `semlex::__private` macro,
/// extra leading arguments after `$crate;`).
const FORWARDERS: &[(&str, &str, &str, &str)] = &[
    (
        "attrs",
        "`attrs![key: value, …]`: registry-checked `[KeyValue; N]`.",
        "attrs",
        "",
    ),
    (
        "metric",
        "`metric!(name: value, { key: value, … })`: records a measurement.",
        "metric",
        "",
    ),
    (
        "event",
        "`event!(name { key: value, … })`: emits an event.",
        "event",
        "",
    ),
    (
        "span",
        "`span!(type { key: value, … }, options…)`: starts a span.",
        "span",
        "",
    ),
    (
        "info",
        "`info!(key: value, …, \"message\", args…)`: a registry-checked `tracing` event.",
        "log",
        "info",
    ),
    (
        "warn",
        "`warn!(key: value, …, \"message\", args…)`: a registry-checked `tracing` event.",
        "log",
        "warn",
    ),
    (
        "error",
        "`error!(key: value, …, \"message\", args…)`: a registry-checked `tracing` event.",
        "log",
        "error",
    ),
    (
        "debug",
        "`debug!(key: value, …, \"message\", args…)`: a registry-checked `tracing` event.",
        "log",
        "debug",
    ),
    (
        "trace",
        "`trace!(key: value, …, \"message\", args…)`: a registry-checked `tracing` event.",
        "log",
        "trace",
    ),
    (
        "assert_span",
        "`assert_span!(telemetry, type { key: value, … })`: a finished span of this type has these attributes (`semlex::testing`).",
        "assert_signal",
        "span",
    ),
    (
        "assert_metric",
        "`assert_metric!(telemetry, name { key: value, … })`: this metric was recorded with these attributes (`semlex::testing`).",
        "assert_signal",
        "metric",
    ),
    (
        "assert_event",
        "`assert_event!(telemetry, name { key: value, … })`: this event was emitted with these attributes (`semlex::testing`).",
        "assert_signal",
        "event",
    ),
    (
        "entity",
        "`entity!(type { key: value, … })`: an entity's attributes for the Resource.",
        "entity",
        "",
    ),
    (
        "record",
        "`record!(span, key: value, …)`: sets attributes, returning the span.",
        "record",
        "",
    ),
    (
        "end",
        "`end!(span, error: &e, error.type: class)` or `end!(span, result: &r, error.type: |e| class)`: ends a span following the error conventions, with the checked `end()`.",
        "end",
        "",
    ),
];

/// The call-site macros. Exported (`#[macro_export]`) by default; with
/// `visibility: crate`, plain `macro_rules!` re-exported with `pub(crate) use`,
/// so a library's macros stay private and `crate::span!` works.
fn forwarders(crate_only: bool) -> TokenStream {
    let macros: TokenStream = FORWARDERS
        .iter()
        .map(|(name, doc, target, extra)| {
            let name = format_ident!("{name}");
            let target = format_ident!("{target}");
            let doc = format!(" {doc}");
            let extra: TokenStream = if extra.is_empty() {
                quote!()
            } else {
                let e: TokenStream = extra.parse().expect("forwarder argument");
                quote!(#e;)
            };
            let body = quote! {
                ($($t:tt)*) => { $crate::__semlex::__private::#target!($crate; #extra $($t)*) };
            };
            if crate_only {
                let internal = format_ident!("__semlex_{name}");
                quote! {
                    #[doc = #doc]
                    #[allow(unused_macros)]
                    macro_rules! #internal { #body }
                    #[doc(hidden)]
                    #[allow(unused_imports)]
                    pub(crate) use #internal;
                }
            } else {
                quote! {
                    #[doc = #doc]
                    #[macro_export]
                    macro_rules! #name { #body }
                }
            }
        })
        .collect();
    if !crate_only {
        return macros;
    }
    // The short names through a glob, which the crate's own items and
    // imports (`use tracing::warn;`) shadow instead of clashing with.
    let names = FORWARDERS.iter().map(|(name, ..)| {
        let (name, internal) = (format_ident!("{name}"), format_ident!("__semlex_{name}"));
        quote!(pub(crate) use super::#internal as #name;)
    });
    quote! {
        #macros
        #[doc(hidden)]
        #[allow(unused_imports)]
        pub(crate) mod __semlex_macros {
            #(#names)*
        }
        #[allow(unused_imports)]
        pub(crate) use self::__semlex_macros::*;
    }
}

/// `start` and the started-span methods for a span group. A started span is
/// the same `Builder` type in the `Started` phase, so the membership impls
/// serve both phases.
#[allow(clippy::too_many_arguments)]
fn span_items(
    reg: &Registry,
    g: &Group,
    kind: &semlex_schema::model::SpanKind,
    name: &semlex_schema::model::SpanName,
    required: &[&&semlex_schema::EffectiveAttribute],
    params: &[Ident],
    start_bounds: &[TokenStream],
    end_bounds: &[TokenStream],
    has_template: bool,
) -> TokenStream {
    use semlex_schema::model::{NamePart, SpanKind as K};
    let id = &g.id;
    let assoc = associations(reg, g);
    let opt_in = g.requirement_level == semlex_schema::model::SignalLevel::OptIn;
    let sets = params.iter().map(|_| quote!(::semlex::Set));
    let otel_kind = format_ident!(
        "{}",
        match kind {
            K::Client => "Client",
            K::Server => "Server",
            K::Producer => "Producer",
            K::Consumer => "Consumer",
            K::Internal => "Internal",
        }
    );

    // Names: templates, the keys they use, and whether the name can change
    // after start (a template uses an attribute not guaranteed at start).
    let at_start: Vec<&str> = required
        .iter()
        .filter(|a| a.sampling_relevant)
        .map(|a| a.key.as_str())
        .collect();
    let templates = name.templates.iter().map(|t| {
        let parts = t.parts.iter().map(|p| match p {
            NamePart::Literal { value } => quote!(::semlex::__private::NamePart::Lit(#value)),
            NamePart::Attribute { attribute } => {
                quote!(::semlex::__private::NamePart::Attr(#attribute))
            }
        });
        quote!(&[#(#parts),*])
    });
    let mut keys: Vec<&str> = name
        .templates
        .iter()
        .flat_map(|t| t.attributes.iter().map(String::as_str))
        .collect();
    keys.sort_unstable();
    keys.dedup();
    let can_change = keys.iter().any(|k| !at_start.contains(k));
    let templated = !name.templates.is_empty();

    let name_msg = format!("span `{id}` has no name template: pass `name: …` to `span!`");
    let name_note = "the registry describes this span's name in prose (`name.note`), so the call site must name it";
    let templated_impl = if templated {
        quote!(impl HasName for ::semlex::__private::NoName {})
    } else {
        quote!()
    };
    let with_template = if has_template {
        quote! {
            /// Sets a member template attribute after start.
            pub fn with_template<A: ::semlex::TemplateAttribute, V: ::semlex::IntoValue<A::Kind>>(mut self, attribute: ::semlex::Templated<A>, value: V) -> <Self as With<A>>::Out
            where
                Self: With<A>,
            {
                self.phase.0.set(::semlex::__private::kv_template(attribute, value), ::semlex::__private::INTERNAL);
                self.advance()
            }
        }
    } else {
        quote!()
    };

    quote! {
        /// This span type, for `SpanCore::start`.
        pub static TYPE: ::semlex::__private::SpanType = ::semlex::__private::SpanType {
            id: #id,
            kind: ::semlex::__private::opentelemetry::trace::SpanKind::#otel_kind,
            opt_in: #opt_in,
            names: ::semlex::__private::NameSpec {
                templates: &[#(#templates),*],
                keys: &[#(#keys),*],
                can_change: #can_change,
                fallback: #id,
            },
            associations: #assoc,
        };

        #[diagnostic::on_unimplemented(message = #name_msg, label = "this span needs a name", note = #name_note)]
        #[doc(hidden)]
        pub trait HasName {}
        impl HasName for ::semlex::__private::Named {}
        #templated_impl

        /// The started phase: the span itself.
        #[doc(hidden)]
        pub struct Started(::semlex::__private::SpanCore);

        /// This span, started, with every required attribute set: the type a
        /// helper can return or take (`&Ready`) to share a span's setup, record
        /// with membership checks, and `end()` it.
        pub type Ready = Builder<#(#sets,)* Started>;

        impl<#(#params),*> Builder<#(#params,)* Checked> {
            /// Starts the span with the attributes set so far, so samplers see them.
            pub fn start<N: HasName + ::semlex::__private::SpanName>(
                self,
                site: &'static ::semlex::__private::SpanSite,
                scope: &::semlex::__private::Scope,
                options: ::semlex::__private::StartOptions,
                name: N,
            ) -> Builder<#(#params,)* Started>
            where
                #(#start_bounds,)*
            {
                let core = ::semlex::__private::SpanCore::start(site, scope, &TYPE, self.attributes, options, name.into_name());
                Builder { attributes: ::std::vec::Vec::new(), phase: Started(core), _state: ::std::marker::PhantomData }
            }
        }

        impl<#(#params,)*> ::std::ops::Deref for Builder<#(#params,)* Started> {
            type Target = ::semlex::__private::SpanCore;
            fn deref(&self) -> &Self::Target { &self.phase.0 }
        }

        impl<#(#params),*> Builder<#(#params,)* Started> {
            /// Sets a member attribute after start (`record!`).
            pub fn with<A: ::semlex::Attribute, V: ::semlex::IntoValue<A::Kind>>(mut self, attribute: A, value: V) -> <Self as With<A>>::Out
            where
                Self: With<A>,
            {
                self.phase.0.set(::semlex::__private::kv(attribute, value), ::semlex::__private::INTERNAL);
                self.advance()
            }



            /// Sets an `unregistered("…")` attribute after start (no membership check).
            #[doc(hidden)]
            pub fn with_unregistered(mut self, attribute: ::semlex::__private::opentelemetry::KeyValue) -> Self {
                self.phase.0.set(attribute, ::semlex::__private::INTERNAL);
                self
            }

            #with_template

            /// Ends the span; every required attribute must be set.
            pub fn end(mut self)
            where
                #(#end_bounds,)*
            {
                self.phase.0.end(None, ::semlex::__private::INTERNAL);
            }

            /// Ends the span at a given time; every required attribute must be set.
            pub fn end_at(mut self, time: ::std::time::SystemTime)
            where
                #(#end_bounds,)*
            {
                self.phase.0.end(Some(time), ::semlex::__private::INTERNAL);
            }

            /// This span as [`Ready`], for a function that returns it: every
            /// required attribute must be set, and each missing one is named
            /// (returning the span directly would only show a type mismatch).
            pub fn ready(self) -> Ready
            where
                #(#end_bounds,)*
            {
                Builder { attributes: self.attributes, phase: self.phase, _state: ::std::marker::PhantomData }
            }

            /// Records the error, sets an error status and ends the span
            /// (required attributes aren't checked on error paths).
            pub fn end_with_error(mut self, error: &dyn ::std::error::Error) {
                self.phase.0.end_with_error(error);
            }

            /// Leaves semlex: the context holding the span.
            pub fn into_otel(self) -> ::semlex::__private::opentelemetry::Context {
                self.phase.0.into_otel()
            }
        }
    }
}

/// Signal-specific items: what a signal's finishing call needs.
/// Each entity's identity attribute keys, once, as `E<index>` constants in
/// `__semlex_registry::entity_identity` (index into the registry's entities).
fn entity_identities(reg: &Registry) -> TokenStream {
    let consts = reg
        .groups_of(GroupKindTag::Entity)
        .enumerate()
        .map(|(i, en)| {
            let c = format_ident!("E{i}");
            let keys = en
                .attributes
                .iter()
                .filter(|a| a.role == semlex_schema::Role::Identity)
                .map(|a| a.key.as_str());
            let doc = format!(" Identity of entity `{}`.", en.id);
            quote!(#[doc = #doc] pub const #c: &[&str] = &[#(#keys),*];)
        });
    quote! {
        /// Entity identity attribute keys, referenced by associations.
        pub mod entity_identity { #(#consts)* }
    }
}

/// A signal's `entity_associations` as a runtime `Associations` value. A
/// refinement without its own inherits its base's (Weaver 0.27 and 0.28 don't copy
/// them; see open-telemetry/weaver#1837).
fn associations(reg: &Registry, g: &Group) -> TokenStream {
    use semlex_schema::model::EntityAssociation as E;
    let kind = g.kind.tag().noun();
    let signal = &g.id;
    let own = &g.entity_associations;
    let exprs = match (&g.refines, own.is_empty()) {
        (semlex_schema::Refines::Base(base), true) => reg
            .group(g.kind.tag(), base)
            .map(|b| b.entity_associations.as_slice())
            .unwrap_or(own),
        _ => own.as_slice(),
    };
    fn expr(reg: &Registry, e: &E) -> TokenStream {
        match e {
            E::Entity { ty, .. } => {
                // Each entity's identity list is emitted once (see
                // `entity_identities`) and referenced here.
                let identity = match reg
                    .groups_of(GroupKindTag::Entity)
                    .position(|en| en.id == *ty)
                {
                    Some(i) => {
                        let c = format_ident!("E{i}");
                        quote!(crate::__semlex_registry::entity_identity::#c)
                    }
                    None => quote!(&[]),
                };
                quote!(::semlex::__private::Association::Entity { ty: #ty, identity: #identity })
            }
            E::OneOf { one_of } => {
                let v = one_of.iter().map(|e| expr(reg, e));
                quote!(::semlex::__private::Association::OneOf(&[#(#v),*]))
            }
            E::AllOf { all_of } => {
                let v = all_of.iter().map(|e| expr(reg, e));
                quote!(::semlex::__private::Association::AllOf(&[#(#v),*]))
            }
        }
    }
    let exprs = exprs.iter().map(|e| expr(reg, e));
    quote!(::semlex::__private::Associations { kind: #kind, signal: #signal, expressions: &[#(#exprs),*] })
}

fn signal_items(
    reg: &Registry,
    g: &Group,
    params: &[Ident],
    bounds: &[TokenStream],
) -> TokenStream {
    let opt_in = g.requirement_level == semlex_schema::model::SignalLevel::OptIn;
    let brief = first_line(&g.brief);
    let assoc = associations(reg, g);
    match &g.kind {
        semlex_schema::GroupKind::Metric {
            name,
            instrument,
            unit,
        } => {
            use semlex_schema::annotations::MetricValueType;
            use semlex_schema::model::Instrument as I;
            let int =
                g.annotations.metric_value_type().ok().flatten() == Some(MetricValueType::Int);
            let (ty, ctor, method, kind) = match (instrument, int) {
                (I::Counter, false) => (
                    quote!(Counter<f64>),
                    quote!(f64_counter),
                    quote!(add),
                    "F64",
                ),
                (I::Counter, true) => (
                    quote!(Counter<u64>),
                    quote!(u64_counter),
                    quote!(add),
                    "U64",
                ),
                (I::UpDownCounter, false) => (
                    quote!(UpDownCounter<f64>),
                    quote!(f64_up_down_counter),
                    quote!(add),
                    "F64",
                ),
                (I::UpDownCounter, true) => (
                    quote!(UpDownCounter<i64>),
                    quote!(i64_up_down_counter),
                    quote!(add),
                    "I64",
                ),
                (I::Histogram, false) => (
                    quote!(Histogram<f64>),
                    quote!(f64_histogram),
                    quote!(record),
                    "F64",
                ),
                (I::Histogram, true) => (
                    quote!(Histogram<u64>),
                    quote!(u64_histogram),
                    quote!(record),
                    "U64",
                ),
                (I::Gauge, false) => (quote!(Gauge<f64>), quote!(f64_gauge), quote!(record), "F64"),
                (I::Gauge, true) => (quote!(Gauge<i64>), quote!(i64_gauge), quote!(record), "I64"),
            };
            let kind = format_ident!(
                "{}",
                if kind == "F64" && unit == "s" {
                    "Seconds"
                } else {
                    kind
                }
            );
            quote! {
                /// The metric name on the wire.
                pub const NAME: &str = #name;
                /// Entity associations.
                pub static ASSOCIATIONS: ::semlex::__private::Associations = #assoc;
                /// The UCUM unit.
                pub const UNIT: &str = #unit;
                /// Whether the metric is opt-in (enabled with `semlex::Config::enable`).
                pub const OPT_IN: bool = #opt_in;
                /// The OpenTelemetry instrument.
                pub type Instrument = ::semlex::__private::opentelemetry::metrics::#ty;
                /// What measurements this metric takes.
                pub type Measure = ::semlex::__private::measure::#kind;
                /// Creates the instrument.
                pub fn create(meter: &::semlex::__private::opentelemetry::metrics::Meter) -> Instrument {
                    meter.#ctor(NAME).with_unit(UNIT).with_description(#brief).build()
                }
                impl<#(#params,)* C> Builder<#(#params,)* C> {
                    /// Records a measurement with the attributes set (all required ones).
                    pub fn measure<V: ::semlex::__private::IntoMeasurement<Measure>>(
                        self,
                        value: V,
                        site: &'static ::semlex::__private::MetricSite<Instrument>,
                        scope: &::semlex::__private::Scope,
                    )
                    where
                        #(#bounds,)*
                    {
                        if let Some(instrument) = site.instrument(NAME, OPT_IN, scope, &ASSOCIATIONS, create) {
                            instrument.#method(value.into_measurement(), &self.attributes);
                        }
                    }
                }
            }
        }
        semlex_schema::GroupKind::Event { event_name } => {
            let id = &g.id;
            let (name, extra) = match event_name {
                Some(n) => (quote!(#n), quote!()),
                None => {
                    let msg = format!(
                        "event refinement `{id}` can't be emitted yet: Weaver (as of 0.28) doesn't record which event it refines, so its `event_name` is unknown"
                    );
                    let note = "emit the base event instead; see https://github.com/open-telemetry/weaver/issues/1836";
                    (
                        quote!(""),
                        quote! {
                            #[diagnostic::on_unimplemented(message = #msg, label = "event refinements can't be emitted yet", note = #note)]
                            #[doc(hidden)]
                            pub trait BaseEventUnknown {}
                        },
                    )
                }
            };
            let unknown = if event_name.is_none() {
                quote!(C: BaseEventUnknown,)
            } else {
                quote!()
            };
            quote! {
                #extra
                /// The `event_name` on the wire.
                pub const EVENT_NAME: &str = #name;
                /// Entity associations.
                pub static ASSOCIATIONS: ::semlex::__private::Associations = #assoc;
                /// Whether the event is opt-in (enabled with `semlex::Config::enable`).
                pub const OPT_IN: bool = #opt_in;
                impl<#(#params,)* C> Builder<#(#params,)* C> {
                    /// Emits the event with the attributes set (all required ones),
                    /// in the current trace context, with its severity if it has one.
                    pub fn emit(
                        self,
                        site: &'static ::semlex::__private::EventSite,
                        scope: &::semlex::__private::Scope,
                        severity: ::core::option::Option<::semlex::__private::opentelemetry::logs::Severity>,
                        fallback: fn(&[::semlex::__private::opentelemetry::KeyValue]),
                    )
                    where
                        #unknown
                        #(#bounds,)*
                    {
                        site.emit(EVENT_NAME, OPT_IN, scope, &ASSOCIATIONS, severity, self.attributes, fallback);
                    }
                }
            }
        }
        semlex_schema::GroupKind::AttributeGroup => quote! {
            impl<#(#params),*> Builder<#(#params,)* Checked> {
                /// Membership check without a value (log macros' `group X { … }`).
                pub fn check_with<A>(self, _attribute: A) -> <Self as With<A>>::Out
                where
                    Self: With<A>,
                {
                    self.advance()
                }

                /// Requirement check: every required attribute was given.
                pub fn check(self)
                where
                    #(#bounds,)*
                {
                }
            }
        },
        semlex_schema::GroupKind::Entity => {
            let id = &g.id;
            quote! {
                impl<#(#params),*> Builder<#(#params,)* Checked> {
                    /// The entity's attributes, for the SDK `Resource`; every
                    /// identity attribute (and required description) must be set.
                    pub fn build(self) -> ::semlex::Entity
                    where
                        #(#bounds,)*
                    {
                        ::semlex::Entity::new(#id, self.attributes)
                    }
                }
            }
        }
        _ => quote!(),
    }
}

/// `Attribute::apply` composing the handlers' runtime transforms, if any.
fn apply_fn(handlers: &[HandlerCall]) -> TokenStream {
    if handlers.is_empty() {
        return quote!();
    }
    let body = handlers.iter().fold(quote!(value), |acc, h| {
        let (p, arg) = (&h.path, &h.arg);
        quote!(#p::apply(#acc, #arg))
    });
    quote! {
        fn apply(&self, value: ::semlex::Value) -> ::semlex::Value { #body }
    }
}

/// `From` and `IntoValue` impls converting an external type into an enum.
fn conversion(
    ty: &syn::Type,
    a: &AttributeDef,
    en: &semlex_schema::names::EnumNames,
) -> TokenStream {
    let e = ident(&en.name);
    let from = match &a.ty {
        ValueType::Enum(def) if def.value_type == Primitive::Int => {
            quote!(Self::from_i64(::core::convert::Into::<i64>::into(value)))
        }
        _ => quote!(Self::from_wire(::core::convert::AsRef::<str>::as_ref(
            &value
        ))),
    };
    let from_ref = match &a.ty {
        ValueType::Enum(def) if def.value_type == Primitive::Int => {
            quote!(Self::from_i64(::core::convert::Into::<i64>::into(*value)))
        }
        _ => quote!(Self::from_wire(::core::convert::AsRef::<str>::as_ref(
            value
        ))),
    };
    quote! {
        impl ::core::convert::From<#ty> for #e {
            fn from(value: #ty) -> Self { #from }
        }
        impl ::core::convert::From<&#ty> for #e {
            fn from(value: &#ty) -> Self { #from_ref }
        }
        impl ::semlex::IntoValue<#e> for #ty {
            fn into_value(self) -> ::semlex::Value { ::semlex::IntoValue::<#e>::into_value(#e::from(self)) }
        }
        impl ::semlex::IntoValue<#e> for &#ty {
            fn into_value(self) -> ::semlex::Value { ::semlex::IntoValue::<#e>::into_value(#e::from(self)) }
        }
    }
}

/// The `Kind` an attribute's values convert to.
fn kind_tokens(ty: &ValueType, enum_name: Option<&Ident>) -> TokenStream {
    let k = |name: &str| {
        let name = format_ident!("{name}");
        quote! { ::semlex::kind::#name }
    };
    match ty {
        ValueType::Primitive(Primitive::String) => k("StringValue"),
        ValueType::Primitive(Primitive::Int) => k("IntValue"),
        ValueType::Primitive(Primitive::Double) => k("DoubleValue"),
        ValueType::Primitive(Primitive::Boolean) => k("BoolValue"),
        ValueType::Array(Primitive::String) => k("StringArrayValue"),
        ValueType::Array(Primitive::Int) => k("IntArrayValue"),
        ValueType::Array(Primitive::Double) => k("DoubleArrayValue"),
        ValueType::Array(Primitive::Boolean) => k("BoolArrayValue"),
        ValueType::Any => k("AnyValue"),
        ValueType::Template(inner) => kind_tokens(inner, enum_name),
        ValueType::Enum(_) => {
            let e = enum_name.expect("enum attributes have enum names");
            quote! { crate::enums::#e }
        }
    }
}

/// `check_literal` and `literal` (and `check_custom` for enums), per type.
fn literal_hooks(
    key: &str,
    ty: &ValueType,
    enum_name: Option<&Ident>,
    unstable_hint: &str,
    opts: &Options,
    handler_checks: &TokenStream,
    plain_kind: bool,
) -> TokenStream {
    let passthrough = quote! {
        /// Literals can't be checked further; the value check reports the type.
        pub const fn check_literal<T>(&self, value: T) { ::std::mem::forget(value) }
        /// Passes a literal through to the value check.
        pub const fn literal<T>(&self, value: T) -> T { value }
    };
    // With no handler to run, a literal takes the value path, so a wrong
    // kind (`5` for a string) gets the usual "is not a valid value" message.
    let plain = |lit: TokenStream, out: TokenStream| {
        if handler_checks.is_empty() {
            return passthrough.clone();
        }
        quote! {
            /// Compile-time check of a literal value.
            #[allow(unused_variables)]
            pub const fn check_literal(&self, value: #lit) { #handler_checks }
            /// A checked literal as a value.
            pub const fn literal(&self, value: #out) -> #out { value }
        }
    };
    match ty {
        // String literals become `StaticStr`, recorded without a copy.
        ValueType::Primitive(Primitive::String) if plain_kind => quote! {
            /// Literals can't be checked further; the value check reports the type.
            pub const fn check_literal<T>(&self, value: T) { ::std::mem::forget(value) }
            /// A string literal as a value, uncopied; other literals pass through.
            pub fn literal<T: ::semlex::__private::StringLiteral>(&self, value: T) -> T::Out { value.literal() }
        },
        ValueType::Primitive(Primitive::String) => plain(quote!(&str), quote!(&'static str)),
        ValueType::Primitive(Primitive::Int) => plain(quote!(i64), quote!(i64)),
        ValueType::Primitive(Primitive::Double) => plain(quote!(f64), quote!(f64)),
        ValueType::Primitive(Primitive::Boolean) => plain(quote!(bool), quote!(bool)),
        // Literals can't be arrays or `any`; pass them through so the value
        // type check reports the mismatch.
        ValueType::Array(_) | ValueType::Any | ValueType::Template(_) => passthrough,
        ValueType::Enum(def) => {
            let e = enum_name.expect("enum attributes have enum names");
            let keep = |s: Stability| opts.min_stability.is_none_or(|min| s >= min);
            let members: Vec<_> = def
                .members
                .iter()
                .filter(|m| emitted_member(m) && keep(m.stability))
                .collect();
            let unstable: Vec<String> = members
                .iter()
                .map(|m| {
                    if m.stability == Stability::Stable {
                        String::new()
                    } else {
                        format!(
                            "value {} of `{key}` is {}; {unstable_hint}",
                            wire_text(&m.value),
                            level(m.stability)
                        )
                    }
                })
                .collect();
            let stable: Vec<bool> = members
                .iter()
                .map(|m| m.stability == Stability::Stable)
                .collect();
            let known = |vals: Vec<String>| {
                let shown = vals.iter().take(8).cloned().collect::<Vec<_>>().join(", ");
                let more = if vals.len() > 8 {
                    format!(" and {} more", vals.len() - 8)
                } else {
                    String::new()
                };
                format!("{shown}{more}")
            };
            match def.value_type {
                Primitive::String => {
                    let open = open_only(def);
                    let wires: Vec<String> = members.iter().map(|m| wire_text(&m.value)).collect();
                    let strs: Vec<&str> = members
                        .iter()
                        .map(|m| match &m.value {
                            EnumValue::String(s) => s.as_str(),
                            _ => unreachable!("string enum"),
                        })
                        .collect();
                    let did_you_mean: Vec<String> = strs
                        .iter()
                        .map(|s| format!("unknown value for `{key}`; did you mean \"{s}\"? Use `custom(\"…\")` for values outside the registry"))
                        .collect();
                    let unknown = format!(
                        "unknown value for `{key}`; known values: {}. Use `custom(\"…\")` for values outside the registry",
                        known(wires)
                    );
                    let in_registry = format!(
                        "this value is in the registry for `{key}`: write it as a plain literal instead of `custom(…)`"
                    );
                    quote! {
                        const MEMBERS: &'static [&'static str] = &[#(#strs),*];
                        const STABLE: &'static [bool] = &[#(#stable),*];
                        const DID_YOU_MEAN: &'static [&'static str] = &[#(#did_you_mean),*];
                        const UNSTABLE: &'static [&'static str] = &[#(#unstable),*];
                        /// Compile-time check of a literal value.
                        pub const fn check_literal(&self, value: &str) {
                            #handler_checks
                            if #open {
                                return;
                            }
                            use ::semlex::__private::rt::{Lookup, lookup};
                            match lookup(value, Self::MEMBERS) {
                                Lookup::Found(i) => {
                                    if !Self::STABLE[i] && !crate::__semlex_registry::ALLOW_UNSTABLE {
                                        panic!("{}", Self::UNSTABLE[i])
                                    }
                                }
                                Lookup::Closest(i) => panic!("{}", Self::DID_YOU_MEAN[i]),
                                Lookup::Unknown => panic!(#unknown),
                            }
                        }
                        /// Compile-time check that a `custom(…)` literal isn't a registry value.
                        pub const fn check_custom(&self, value: &str) {
                            if let ::semlex::__private::rt::Lookup::Found(_) = ::semlex::__private::rt::lookup(value, Self::MEMBERS) {
                                panic!(#in_registry)
                            }
                        }
                        /// A checked literal as a value.
                        pub const fn literal(&self, value: &'static str) -> crate::enums::#e {
                            crate::enums::#e::from_static(value)
                        }
                    }
                }
                Primitive::Int => {
                    let ints: Vec<i64> = members
                        .iter()
                        .map(|m| match m.value {
                            EnumValue::Int(i) => i,
                            _ => unreachable!("int enum"),
                        })
                        .collect();
                    let unknown = format!(
                        "unknown value for `{key}`; known values: {}. Use `custom(…)` for values outside the registry",
                        known(ints.iter().map(|i| i.to_string()).collect())
                    );
                    let in_registry = format!(
                        "this value is in the registry for `{key}`: write it as a plain literal instead of `custom(…)`"
                    );
                    quote! {
                        const MEMBERS: &'static [i64] = &[#(#ints),*];
                        const STABLE: &'static [bool] = &[#(#stable),*];
                        const UNSTABLE: &'static [&'static str] = &[#(#unstable),*];
                        /// Compile-time check of a literal value.
                        pub const fn check_literal(&self, value: i64) {
                            #handler_checks
                            match ::semlex::__private::rt::position_i64(value, Self::MEMBERS) {
                                Some(i) => {
                                    if !Self::STABLE[i] && !crate::__semlex_registry::ALLOW_UNSTABLE {
                                        panic!("{}", Self::UNSTABLE[i])
                                    }
                                }
                                None => panic!(#unknown),
                            }
                        }
                        /// Compile-time check that a `custom(…)` literal isn't a registry value.
                        pub const fn check_custom(&self, value: i64) {
                            if ::semlex::__private::rt::position_i64(value, Self::MEMBERS).is_some() {
                                panic!(#in_registry)
                            }
                        }
                        /// A checked literal as a value.
                        pub const fn literal(&self, value: i64) -> crate::enums::#e {
                            crate::enums::#e::from_i64(value)
                        }
                    }
                }
                Primitive::Boolean | Primitive::Double => {
                    let bools: Vec<bool> = members
                        .iter()
                        .filter_map(|m| match m.value {
                            EnumValue::Bool(b) => Some(b),
                            _ => None,
                        })
                        .collect();
                    let unknown = format!(
                        "unknown value for `{key}`; known values: {}",
                        known(bools.iter().map(|b| b.to_string()).collect())
                    );
                    quote! {
                        const MEMBERS: &'static [bool] = &[#(#bools),*];
                        const STABLE: &'static [bool] = &[#(#stable),*];
                        const UNSTABLE: &'static [&'static str] = &[#(#unstable),*];
                        /// Compile-time check of a literal value.
                        pub const fn check_literal(&self, value: bool) {
                            #handler_checks
                            let mut i = 0;
                            while i < Self::MEMBERS.len() {
                                if Self::MEMBERS[i] == value {
                                    if !Self::STABLE[i] && !crate::__semlex_registry::ALLOW_UNSTABLE {
                                        panic!("{}", Self::UNSTABLE[i])
                                    }
                                    return;
                                }
                                i += 1;
                            }
                            panic!(#unknown)
                        }
                        /// A checked literal as a value.
                        pub const fn literal(&self, value: bool) -> crate::enums::#e {
                            crate::enums::#e::from_bool(value)
                        }
                    }
                }
            }
        }
    }
}

fn emitted_member(m: &semlex_schema::EnumMember) -> bool {
    !m.annotations.code_generation_exclude().unwrap_or(false)
}

/// A string enum whose only members are `_OTHER` (like OTel's `error.type`):
/// an open set with nothing to check against, so it takes plain strings.
/// Decided on the registry's members, not the emitted ones, so an enum whose
/// members are all filtered out (by `stability:` or `code_generation.exclude`)
/// isn't mistaken for open.
fn open_only(def: &semlex_schema::EnumDef) -> bool {
    !def.members.is_empty()
        && def
            .members
            .iter()
            .all(|m| matches!(&m.value, EnumValue::String(s) if s == "_OTHER"))
}

fn wire_text(v: &EnumValue) -> String {
    match v {
        EnumValue::String(s) => format!("\"{s}\""),
        EnumValue::Int(i) => i.to_string(),
        EnumValue::Bool(b) => b.to_string(),
    }
}

/// An enum attribute's value type.
fn enum_type(
    key: &str,
    a: &AttributeDef,
    def: &semlex_schema::EnumDef,
    names: &semlex_schema::names::EnumNames,
    opts: &Options,
) -> TokenStream {
    let e = ident(&names.name);
    let brief = doc(&format!("Values of `{key}`: {}", first_line(&a.brief)));
    let keep = |s: Stability| opts.min_stability.is_none_or(|min| s >= min);
    let members: Vec<(&semlex_schema::EnumMember, Ident)> = def
        .members
        .iter()
        .zip(&names.variants)
        .filter(|(m, _)| emitted_member(m) && keep(m.stability))
        .map(|(m, v)| (m, ident(v)))
        .collect();
    let variants = members.iter().map(|(m, v)| {
        let doc = doc(&m
            .brief
            .as_deref()
            .map(first_line)
            .unwrap_or_else(|| wire_text(&m.value)));
        let dep = deprecated_attr(&m.deprecated);
        quote! { #[doc = #doc] #dep #v, }
    });
    let vs: Vec<&Ident> = members.iter().map(|(_, v)| v).collect();
    match def.value_type {
        Primitive::String => {
            // An open set with nothing to check against: plain strings too.
            let open_strings = if open_only(def) {
                quote! {
                    impl ::semlex::IntoValue<#e> for &str {
                        fn into_value(self) -> ::semlex::__private::opentelemetry::Value {
                            ::semlex::__private::opentelemetry::Value::from(self.to_owned())
                        }
                    }
                    impl ::semlex::IntoValue<#e> for &::std::string::String {
                        fn into_value(self) -> ::semlex::__private::opentelemetry::Value {
                            ::semlex::__private::opentelemetry::Value::from(self.clone())
                        }
                    }
                    impl ::semlex::IntoValue<#e> for ::std::string::String {
                        fn into_value(self) -> ::semlex::__private::opentelemetry::Value {
                            ::semlex::__private::opentelemetry::Value::from(self)
                        }
                    }
                    impl ::semlex::IntoValue<#e> for ::std::borrow::Cow<'static, str> {
                        fn into_value(self) -> ::semlex::__private::opentelemetry::Value {
                            ::semlex::__private::opentelemetry::Value::from(self)
                        }
                    }
                    impl ::semlex::IntoValue<#e> for ::std::sync::Arc<str> {
                        fn into_value(self) -> ::semlex::__private::opentelemetry::Value {
                            ::semlex::__private::opentelemetry::Value::from(self)
                        }
                    }
                }
            } else {
                quote!()
            };
            let wires: Vec<&str> = members
                .iter()
                .map(|(m, _)| match &m.value {
                    EnumValue::String(s) => s.as_str(),
                    _ => unreachable!("string enum"),
                })
                .collect();
            quote! {
                #[doc = #brief]
                #[derive(Debug, Clone, PartialEq, Eq, Hash)]
                #[non_exhaustive]
                pub enum #e {
                    #(#variants)*
                    /// A value outside the registry.
                    Custom(::std::borrow::Cow<'static, str>),
                }
                impl #e {
                    /// The wire value.
                    pub fn as_str(&self) -> &str {
                        match self {
                            #(Self::#vs => #wires,)*
                            Self::Custom(v) => v,
                        }
                    }
                    /// The member with this wire value, or `Custom` (owned).
                    pub fn from_wire(value: &str) -> Self {
                        match value {
                            #(#wires => Self::#vs,)*
                            other => Self::Custom(::std::borrow::Cow::Owned(other.to_string())),
                        }
                    }
                    /// The member with this wire value, or `Custom`.
                    pub const fn from_static(value: &'static str) -> Self {
                        #(if ::semlex::__private::rt::str_eq(value, #wires) { return Self::#vs; })*
                        Self::Custom(::std::borrow::Cow::Borrowed(value))
                    }
                }
                impl ::std::fmt::Display for #e {
                    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                        f.write_str(self.as_str())
                    }
                }
                impl ::semlex::IntoValue<#e> for #e {
                    fn into_value(self) -> ::semlex::__private::opentelemetry::Value {
                        match self {
                            #(Self::#vs => ::semlex::__private::opentelemetry::Value::from(#wires),)*
                            Self::Custom(v) => ::semlex::__private::opentelemetry::Value::from(v),
                        }
                    }
                }
                /// By reference, so one value can be recorded in several places
                /// (the enum isn't `Copy`: `Custom` holds a string).
                impl ::semlex::IntoValue<#e> for &#e {
                    fn into_value(self) -> ::semlex::__private::opentelemetry::Value {
                        match self {
                            #(#e::#vs => ::semlex::__private::opentelemetry::Value::from(#wires),)*
                            #e::Custom(v) => ::semlex::__private::opentelemetry::Value::from(v.clone()),
                        }
                    }
                }
                impl ::semlex::__private::LogKind for #e {
                    type Field = ::semlex::__private::tracing::field::DisplayValue<String>;
                    fn field(value: ::semlex::Value) -> Self::Field {
                        ::semlex::__private::text_field(value)
                    }
                }
                #open_strings
                impl ::semlex::IntoValue<#e> for ::semlex::Custom<&'static str> {
                    fn into_value(self) -> ::semlex::__private::opentelemetry::Value {
                        ::semlex::__private::opentelemetry::Value::from(self.0)
                    }
                }
                impl ::semlex::IntoValue<#e> for ::semlex::Custom<String> {
                    fn into_value(self) -> ::semlex::__private::opentelemetry::Value {
                        ::semlex::__private::opentelemetry::Value::from(self.0)
                    }
                }
            }
        }
        Primitive::Int => {
            let ints: Vec<i64> = members
                .iter()
                .map(|(m, _)| match m.value {
                    EnumValue::Int(i) => i,
                    _ => unreachable!("int enum"),
                })
                .collect();
            quote! {
                #[doc = #brief]
                #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
                #[non_exhaustive]
                pub enum #e {
                    #(#variants)*
                    /// A value outside the registry.
                    Custom(i64),
                }
                impl #e {
                    /// The wire value.
                    pub const fn value(&self) -> i64 {
                        match self {
                            #(Self::#vs => #ints,)*
                            Self::Custom(v) => *v,
                        }
                    }
                    /// The member with this wire value, or `Custom`.
                    pub const fn from_i64(value: i64) -> Self {
                        match value {
                            #(#ints => Self::#vs,)*
                            v => Self::Custom(v),
                        }
                    }
                }
                impl ::semlex::IntoValue<#e> for #e {
                    fn into_value(self) -> ::semlex::__private::opentelemetry::Value {
                        ::semlex::__private::opentelemetry::Value::I64(self.value())
                    }
                }
                impl ::semlex::IntoValue<#e> for &#e {
                    fn into_value(self) -> ::semlex::__private::opentelemetry::Value {
                        ::semlex::__private::opentelemetry::Value::I64(self.value())
                    }
                }
                impl ::semlex::__private::LogKind for #e {
                    type Field = i64;
                    fn field(value: ::semlex::Value) -> i64 {
                        match value {
                            ::semlex::Value::I64(i) => i,
                            _ => 0,
                        }
                    }
                }
                impl ::semlex::IntoValue<#e> for ::semlex::Custom<i64> {
                    fn into_value(self) -> ::semlex::__private::opentelemetry::Value {
                        ::semlex::__private::opentelemetry::Value::I64(self.0)
                    }
                }
            }
        }
        Primitive::Boolean | Primitive::Double => {
            let bools: Vec<bool> = members
                .iter()
                .filter_map(|(m, _)| match m.value {
                    EnumValue::Bool(b) => Some(b),
                    _ => None,
                })
                .collect();
            // Every member of a boolean enum is one of two values; a value
            // without a member maps to the first member as a fallback.
            let fallback = vs
                .first()
                .map(|v| quote!(Self::#v))
                .unwrap_or_else(|| quote!(unreachable!()));
            quote! {
                #[doc = #brief]
                #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
                #[non_exhaustive]
                pub enum #e {
                    #(#variants)*
                }
                impl #e {
                    /// The wire value.
                    pub const fn value(&self) -> bool {
                        match self {
                            #(Self::#vs => #bools,)*
                        }
                    }
                    /// The member with this wire value.
                    pub const fn from_bool(value: bool) -> Self {
                        match value {
                            #(#bools => Self::#vs,)*
                            #[allow(unreachable_patterns)]
                            _ => #fallback,
                        }
                    }
                }
                impl ::semlex::IntoValue<#e> for #e {
                    fn into_value(self) -> ::semlex::__private::opentelemetry::Value {
                        ::semlex::__private::opentelemetry::Value::Bool(self.value())
                    }
                }
                impl ::semlex::IntoValue<#e> for &#e {
                    fn into_value(self) -> ::semlex::__private::opentelemetry::Value {
                        ::semlex::__private::opentelemetry::Value::Bool(self.value())
                    }
                }
                impl ::semlex::__private::LogKind for #e {
                    type Field = bool;
                    fn field(value: ::semlex::Value) -> bool {
                        matches!(value, ::semlex::Value::Bool(true))
                    }
                }
            }
        }
    }
}

/// Membership and requirement traits and the typestate builder for each
/// group.
/// Whether a group is emitted.
fn emitted(g: &Group, sel: &Selection<'_>, opts: &Options) -> bool {
    let keep = |s: Stability| opts.min_stability.is_none_or(|min| s >= min);
    if !keep(g.stability) || g.annotations.code_generation_exclude().unwrap_or(false) {
        return false;
    }
    if !opts.groups.is_empty()
        && !opts
            .groups
            .iter()
            .any(|p| crate::options::matches(p, &g.id))
    {
        return false;
    }
    // A group whose required attribute isn't emitted can never be completed.
    !g.required()
        .any(|a| !sel.attributes.contains_key(a.key.as_str()))
}

/// Runs each hook on the emitted items carrying its annotation; returns their
/// items, rooted at the registry crate.
#[allow(clippy::too_many_arguments)]
fn run_hooks(
    reg: &Registry,
    names: &Names,
    sel: &Selection<'_>,
    opts: &Options,
    hooks: &[&dyn AnnotationHook],
    errors: &mut Vec<EmitError>,
    vis: &TokenStream,
    allow: &TokenStream,
) -> TokenStream {
    let mut tree = Tree::default();
    for hook in hooks {
        let key = hook.key();
        let segs: Vec<&str> = key.split('.').collect();
        for a in sel.attributes.values() {
            if let Some(v) = a.annotations.at(&segs) {
                let place = format!("attribute `{}`", a.key);
                hook.on_attribute(a, v, &mut Emit::new(names, key, place, &mut tree, errors));
            }
        }
        for g in reg.groups.iter().filter(|g| emitted(g, sel, opts)) {
            let Some(v) = g.annotations.at(&segs) else {
                continue;
            };
            let place = format!("{} `{}`", g.kind.tag().noun(), g.id);
            let out = &mut Emit::new(names, key, place, &mut tree, errors);
            match g.kind.tag() {
                GroupKindTag::Span => hook.on_span(g, v, out),
                GroupKindTag::Metric => hook.on_metric(g, v, out),
                GroupKindTag::Event => hook.on_event(g, v, out),
                GroupKindTag::Entity => hook.on_entity(g, v, out),
                GroupKindTag::AttributeGroup => {
                    out.error("hooks don't handle attribute groups");
                }
            }
        }
    }
    tree.render_with("Items from annotation hooks", "", vis, allow)
}

fn groups(
    reg: &Registry,
    names: &Names,
    sel: &Selection<'_>,
    opts: &Options,
    vis: &TokenStream,
    allow: &TokenStream,
) -> TokenStream {
    let mut roots: BTreeMap<GroupKindTag, Tree> = BTreeMap::new();
    for g in &reg.groups {
        if !emitted(g, sel, opts) {
            continue;
        }
        let path = &names.groups[&(g.kind.tag(), g.id.clone())];
        roots
            .entry(g.kind.tag())
            .or_default()
            .at(path)
            .items
            .extend(group(reg, g, names, sel));
    }
    // Every root exists, even when empty, so call sites can always alias it.
    for tag in [
        GroupKindTag::Span,
        GroupKindTag::Metric,
        GroupKindTag::Event,
        GroupKindTag::Entity,
        GroupKindTag::AttributeGroup,
    ] {
        roots.entry(tag).or_default();
    }
    let mut out = TokenStream::new();
    for (tag, tree) in roots {
        let (root, doc) = match tag {
            GroupKindTag::Span => ("spans", " Span builders, by span type."),
            GroupKindTag::Metric => ("metrics", " Metric builders, by metric name."),
            GroupKindTag::Event => ("events", " Event builders, by event name."),
            GroupKindTag::Entity => ("entities", " Entity builders, by entity type."),
            GroupKindTag::AttributeGroup => ("groups", " Public attribute groups, by id."),
        };
        let what = match tag {
            GroupKindTag::AttributeGroup => "Attribute groups",
            other => match other.noun() {
                "span" => "Spans",
                "metric" => "Metrics",
                "event" => "Events",
                _ => "Entities",
            },
        };
        let root = format_ident!("{root}");
        let body = tree.render(what, "");
        out.extend(quote! {
            #[doc = #doc]
            #[allow(non_camel_case_types)]
            #allow
            #vis mod #root { #body }
        });
    }
    out
}

fn group(reg: &Registry, g: &Group, names: &Names, sel: &Selection<'_>) -> TokenStream {
    let noun = g.kind.tag().noun();
    let id = &g.id;
    let members: Vec<_> = g
        .attributes
        .iter()
        .filter(|a| sel.attributes.contains_key(a.key.as_str()))
        .collect();
    let marker = |key: &str| ident(&names.attributes[key].marker);
    let required: Vec<_> = members
        .iter()
        .filter(|a| a.requirement == Requirement::Required)
        .collect();
    let params: Vec<Ident> = (0..required.len()).map(|i| format_ident!("P{i}")).collect();
    let is_metric = matches!(g.kind.tag(), GroupKindTag::Metric);
    let is_span = matches!(g.kind.tag(), GroupKindTag::Span);
    // `unregistered("…")` is allowed on every signal but entities.
    let with_unregistered = if !matches!(
        g.kind.tag(),
        GroupKindTag::Entity | GroupKindTag::AttributeGroup
    ) {
        quote! {
            /// Sets an `unregistered("…")` attribute (no membership check).
            #[doc(hidden)]
            pub fn with_unregistered(mut self, attribute: ::semlex::__private::opentelemetry::KeyValue) -> Self {
                self.attributes.push(attribute);
                self
            }
        }
    } else {
        quote!()
    };
    // `extra(…)` is for attributes the signal doesn't define: the keys it
    // does, for the call sites' check.
    let members_impl = if with_unregistered.is_empty() {
        quote!()
    } else {
        let member_keys = members.iter().map(|a| a.key.as_str());
        quote! {
            impl<#(#params,)* C> ::semlex::__private::Members for Builder<#(#params,)* C> {
                const MEMBERS: &'static [&'static str] = &[#(#member_keys),*];
            }
        }
    };

    let with_msg = format!("`{{A}}` is not an attribute of {noun} `{id}`");
    let with_label = format!("not an attribute of this {noun}");
    const MAX_KEYS: usize = 25;
    let mut keys: Vec<String> = members
        .iter()
        .take(MAX_KEYS)
        .map(|a| format!("`{}`", a.key))
        .collect();
    if members.len() > MAX_KEYS {
        keys.push(format!("and {} more", members.len() - MAX_KEYS));
    }
    let defines = if keys.is_empty() {
        "no attributes".to_string()
    } else {
        keys.join(", ")
    };
    let with_note = format!(
        "{noun} `{id}` defines: {defines}. Use one of those, or add this attribute to the {noun} in the registry (attribute types are named after their keys with `.` as `_`)"
    );
    let mut out = quote! {
        #[diagnostic::on_unimplemented(message = #with_msg, label = #with_label, note = #with_note)]
        /// Membership: implemented by the builder for each member attribute.
        pub trait With<A> {
            /// The builder after setting `A`.
            type Out;
            #[doc(hidden)]
            fn advance(self) -> Self::Out;
        }

        /// The builder's check state. Bounds on it that never hold carry
        /// attribute-specific errors.
        #[doc(hidden)]
        pub struct Checked;

        /// No attribute: a second `With` candidate, so rustc never infers
        /// the attribute type from a lone impl and always reports this
        /// group's own messages.
        #[doc(hidden)]
        pub enum __NotAnAttribute {}
    };
    for a in &required {
        let m = &names.attributes[a.key.as_str()].marker;
        let t = format_ident!("Required_{}", bare(m));
        let key = &a.key;
        let msg = format!("missing required attribute `{key}` on {noun} `{id}`");
        let label = format!("`{key}` is required here");
        let note = match g.kind.tag() {
            GroupKindTag::Span => {
                format!("set it in `span!`, or with `record!(span, {key}: <value>)` before `end()`")
            }
            GroupKindTag::Metric => format!("add `{key}: <value>` to the `metric!` attributes"),
            GroupKindTag::Event => format!("add `{key}: <value>` to the `event!` attributes"),
            GroupKindTag::Entity => format!("add `{key}: <value>` to `entity!`"),
            GroupKindTag::AttributeGroup => {
                format!("add `{key}: <value>` inside `group {id} {{ … }}`")
            }
        };
        let req_doc = format!(" Requirement: `{key}` is set.");
        out.extend(quote! {
            #[diagnostic::on_unimplemented(message = #msg, label = #label, note = #note)]
            #[doc = #req_doc]
            pub trait #t {}
            impl #t for ::semlex::Set {}
        });
    }
    if matches!(g.kind.tag(), GroupKindTag::Span) {
        for a in required.iter().filter(|a| a.sampling_relevant) {
            let m = &names.attributes[a.key.as_str()].marker;
            let t = format_ident!("AtStart_{}", bare(m));
            let key = &a.key;
            let msg = format!("missing sampling-relevant attribute `{key}` on span `{id}`");
            let label = format!("`{key}` must be set when the span starts");
            let note = format!(
                "add `{key}: <value>` to `span!`; samplers only see attributes given at start"
            );
            let sr_doc = format!(" Sampling-relevant: `{key}` is set at start.");
            out.extend(quote! {
                #[diagnostic::on_unimplemented(message = #msg, label = #label, note = #note)]
                #[doc = #sr_doc]
                pub trait #t {}
                impl #t for ::semlex::Set {}
            });
        }
    }

    let brief = doc(&g.brief);
    let dep = deprecated_attr(&g.deprecated);
    let params2: Vec<Ident> = params.iter().map(|p| format_ident!("{p}2")).collect();
    // Only what the group uses: `retype` for required attributes, `with_template`
    // for template members (generated code volume matters at OTel scale).
    let retype = if params.is_empty() {
        quote!()
    } else {
        quote! {
            fn retype<#(#params2),*>(self) -> Builder<#(#params2,)* C> {
                Builder { attributes: self.attributes, phase: self.phase, _state: ::std::marker::PhantomData }
            }
        }
    };
    let has_template = members.iter().any(|a| {
        sel.attributes
            .get(a.key.as_str())
            .is_some_and(|d| matches!(d.ty, ValueType::Template(_)))
    });
    let with_template = if has_template {
        quote! {
            /// Sets a member template attribute: `attr::….key("suffix")`.
            pub fn with_template<A: ::semlex::TemplateAttribute, V: ::semlex::IntoValue<A::Kind>>(mut self, attribute: ::semlex::Templated<A>, value: V) -> <Self as With<A>>::Out
            where
                Self: With<A>,
            {
                self.attributes.push(::semlex::__private::kv_template(attribute, value));
                self.advance()
            }
        }
    } else {
        quote!()
    };
    out.extend(quote! {
        #[doc = #brief]
        pub struct Builder<#(#params = ::semlex::Unset,)* C = Checked> {
            attributes: ::std::vec::Vec<::semlex::__private::opentelemetry::KeyValue>,
            phase: C,
            _state: ::std::marker::PhantomData<(#(#params,)*)>,
        }

        #[doc = #brief]
        #dep
        pub fn builder() -> Builder {
            Builder { attributes: ::std::vec::Vec::new(), phase: Checked, _state: ::std::marker::PhantomData }
        }

        impl<#(#params,)* C> Builder<#(#params,)* C> {
            #retype
        }

        #members_impl

        impl<#(#params),*> Builder<#(#params,)* Checked> {
            /// Sets a member attribute.
            pub fn with<A: ::semlex::Attribute, V: ::semlex::IntoValue<A::Kind>>(mut self, attribute: A, value: V) -> <Self as With<A>>::Out
            where
                Self: With<A>,
            {
                self.attributes.push(::semlex::__private::kv(attribute, value));
                self.advance()
            }


            #with_unregistered

            #with_template

        }
    });
    let bounds: Vec<TokenStream> = required
        .iter()
        .zip(&params)
        .map(|(a, p)| {
            let t = format_ident!(
                "Required_{}",
                bare(&names.attributes[a.key.as_str()].marker)
            );
            quote!(#p: #t)
        })
        .collect();
    out.extend(signal_items(reg, g, &params, &bounds));
    if let semlex_schema::GroupKind::Span { kind, name } = &g.kind {
        let (start_bounds, end_bounds): (Vec<_>, Vec<_>) = required
            .iter()
            .zip(&params)
            .partition::<Vec<_>, _>(|(a, _)| a.sampling_relevant);
        let start_bounds: Vec<TokenStream> = start_bounds
            .into_iter()
            .map(|(a, p)| {
                let t = format_ident!("AtStart_{}", bare(&names.attributes[a.key.as_str()].marker));
                quote!(#p: #t)
            })
            .collect();
        let end_bounds: Vec<TokenStream> = end_bounds
            .into_iter()
            .map(|(a, p)| {
                let t = format_ident!(
                    "Required_{}",
                    bare(&names.attributes[a.key.as_str()].marker)
                );
                quote!(#p: #t)
            })
            .collect();
        out.extend(span_items(
            reg,
            g,
            kind,
            name,
            &required,
            &params,
            &start_bounds,
            &end_bounds,
            has_template,
        ));
    }
    out.extend(quote! {
        #[diagnostic::do_not_recommend]
        impl<#(#params,)* C> With<__NotAnAttribute> for Builder<#(#params,)* C> {
            type Out = Self;
            fn advance(self) -> Self::Out { self }
        }
    });
    for a in &members {
        let m = marker(&a.key);
        // `cardinality: unbounded` (as this metric sees it) can't be a metric attribute.
        if is_metric
            && a.annotations.cardinality().ok().flatten()
                == Some(semlex_schema::annotations::Cardinality::Unbounded)
        {
            let t = format_ident!(
                "Unbounded_{}",
                bare(&names.attributes[a.key.as_str()].marker)
            );
            let key = &a.key;
            let msg = format!(
                "`{key}` is unbounded (annotation `cardinality: unbounded`) and can't be an attribute of metric `{id}`"
            );
            let label = "unbounded values make metric cardinality explode";
            let note = format!(
                "use a bounded attribute, or mark `{key}` `cardinality: bounded` on this metric's reference in the registry if its values really are bounded here"
            );
            out.extend(quote! {
                #[diagnostic::on_unimplemented(message = #msg, label = #label, note = #note)]
                #[doc(hidden)]
                pub trait #t {}
                impl<#(#params,)* C: #t> With<crate::attr_types::#m> for Builder<#(#params,)* C> {
                    type Out = Self;
                    fn advance(self) -> Self::Out { self }
                }
            });
            continue;
        }
        let out_params: Vec<TokenStream> = required
            .iter()
            .zip(&params)
            .map(|(r, p)| {
                if r.key == a.key {
                    quote!(::semlex::Set)
                } else {
                    quote!(#p)
                }
            })
            .collect();
        let (out_ty, advance) = if a.requirement == Requirement::Required {
            (quote!(Builder<#(#out_params,)* C>), quote!(self.retype()))
        } else {
            (quote!(Self), quote!(self))
        };
        // On spans, a required sampling-relevant attribute can only be set
        // before start: `Checked` implements this bound, `Started` doesn't.
        let phase_bound = if is_span
            && a.requirement == Requirement::Required
            && a.sampling_relevant
        {
            let t = format_ident!(
                "BeforeStart_{}",
                bare(&names.attributes[a.key.as_str()].marker)
            );
            let key = &a.key;
            let msg = format!(
                "`{key}` is sampling-relevant on span `{id}` and can't be recorded after start"
            );
            out.extend(quote! {
                #[diagnostic::on_unimplemented(message = #msg, label = "set this in `span!` instead", note = "samplers only see attributes given when the span starts")]
                #[doc(hidden)]
                pub trait #t {}
                impl #t for Checked {}
            });
            quote!(: #t)
        } else {
            quote!()
        };
        // Not listed in "the trait is implemented for …" help (the note lists
        // the keys instead), unless a bound carries a message rustc must reach.
        let hidden = if phase_bound.is_empty() {
            quote!(#[diagnostic::do_not_recommend])
        } else {
            quote!()
        };
        out.extend(quote! {
            #hidden
            impl<#(#params,)* C #phase_bound> With<crate::attr_types::#m> for Builder<#(#params,)* C> {
                type Out = #out_ty;
                fn advance(self) -> Self::Out { #advance }
            }
        });
    }
    out
}
