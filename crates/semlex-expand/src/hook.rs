//! Annotation hooks for custom macro crates.

use crate::emit::{EmitError, Tree, ident};
use proc_macro2::TokenStream;
use quote::quote;
use semlex_schema::names::Names;
use semlex_schema::{AttributeDef, Group, GroupKindTag};
use serde_json::Value;

/// Module names `registry!()` emits at the registry crate root; hooks can't emit
/// items under them.
const RESERVED: &[&str] = &[
    "attr",
    "attr_types",
    "enums",
    "spans",
    "metrics",
    "events",
    "entities",
    "groups",
];

/// Handles one annotation in a custom `registry!` macro.
///
/// A hook owns the annotation path [`key`](Self::key) (e.g. `org.slo`): the
/// path and everything below it count as declared, and each item carrying it
/// is passed to the matching method with the annotation's value (any YAML
/// shape). Only emitted items are passed (after `stability`, `groups` and
/// `code_generation.exclude`).
///
/// Every method defaults to an error, so an annotation on an item kind the
/// hook doesn't handle fails the build instead of doing nothing.
pub trait AnnotationHook {
    /// The dotted annotation path this hook handles, e.g. `org.slo`.
    fn key(&self) -> &str;

    /// An attribute definition carrying the annotation.
    fn on_attribute(&self, attribute: &AttributeDef, value: &Value, out: &mut Emit<'_>) {
        let _ = (attribute, value);
        out.unsupported("attributes");
    }

    /// A span (or span refinement) carrying the annotation.
    fn on_span(&self, span: &Group, value: &Value, out: &mut Emit<'_>) {
        let _ = (span, value);
        out.unsupported("spans");
    }

    /// A metric (or metric refinement) carrying the annotation.
    fn on_metric(&self, metric: &Group, value: &Value, out: &mut Emit<'_>) {
        let _ = (metric, value);
        out.unsupported("metrics");
    }

    /// An event carrying the annotation.
    fn on_event(&self, event: &Group, value: &Value, out: &mut Emit<'_>) {
        let _ = (event, value);
        out.unsupported("events");
    }

    /// An entity carrying the annotation.
    fn on_entity(&self, entity: &Group, value: &Value, out: &mut Emit<'_>) {
        let _ = (entity, value);
        out.unsupported("entities");
    }
}

/// Where a hook's items and errors go.
pub struct Emit<'a> {
    names: &'a Names,
    key: &'a str,
    place: String,
    pub(crate) tree: &'a mut Tree,
    pub(crate) errors: &'a mut Vec<EmitError>,
}

impl<'a> Emit<'a> {
    pub(crate) fn new(
        names: &'a Names,
        key: &'a str,
        place: String,
        tree: &'a mut Tree,
        errors: &'a mut Vec<EmitError>,
    ) -> Self {
        Emit {
            names,
            key,
            place,
            tree,
            errors,
        }
    }

    /// Emits `items` in the module at `path` from the registry crate root, e.g.
    /// `&["slo", "http"]` for `crate::slo::http`. Modules are shared between
    /// calls and hooks; segments are mangled like registry names (`type` →
    /// `r#type`). The root modules `registry!()` emits are reserved.
    pub fn items(&mut self, path: &[&str], items: TokenStream) {
        if let Some(first) = path.first()
            && (RESERVED.contains(first) || first.starts_with("__semlex"))
        {
            self.error(format!(
                "module `{first}` is emitted by `registry!()`; emit hook items elsewhere"
            ));
            return;
        }
        let path: Vec<String> = path
            .iter()
            .map(|s| semlex_schema::names::ident(s))
            .collect();
        self.tree.at(&path).items.extend(items);
    }

    /// Fails the build with `message`, prefixed with the annotation and the
    /// item it's on.
    pub fn error(&mut self, message: impl std::fmt::Display) {
        self.errors.push((
            None,
            format!("annotation `{}` on {}: {message}", self.key, self.place),
        ));
    }

    /// The path of a signal's generated module, e.g.
    /// `crate::metrics::http::server::request::duration`.
    pub fn signal_path(&self, group: &Group) -> Option<TokenStream> {
        let root = match group.kind.tag() {
            GroupKindTag::Span => "spans",
            GroupKindTag::Metric => "metrics",
            GroupKindTag::Event => "events",
            GroupKindTag::Entity => "entities",
            GroupKindTag::AttributeGroup => "groups",
        };
        let segs = self
            .names
            .groups
            .get(&(group.kind.tag(), group.id.clone()))?;
        Some(path(root, segs))
    }

    /// The path of an attribute's constant, e.g. `crate::attr::http::route`.
    pub fn attribute_path(&self, key: &str) -> Option<TokenStream> {
        let n = self.names.attributes.get(key)?;
        let mut segs = n.module.clone();
        segs.push(n.leaf.clone());
        Some(path("attr", &segs))
    }

    fn unsupported(&mut self, what: &str) {
        self.error(format!("its hook doesn't handle {what}"));
    }
}

fn path(root: &str, segs: &[String]) -> TokenStream {
    let root = ident(root);
    let segs = segs.iter().map(|s| ident(s));
    quote!(crate::#root #(::#segs)*)
}
