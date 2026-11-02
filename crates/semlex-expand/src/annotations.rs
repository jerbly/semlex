//! Annotation namespaces: the declaration rule and the
//! handlers that apply to each attribute.

use crate::emit::EmitError;
use crate::hook::AnnotationHook;
use crate::options::{Handling, Options};
use proc_macro2::TokenStream;
use quote::quote;
use semlex_schema::{Annotations, Registry};
use serde_json::Value;
use std::collections::BTreeSet;

/// Namespaces semlex interprets itself.
pub(crate) const BUILT_IN: &[&str] = &["code_generation", "cardinality", "sensitivity"];

/// Namespaces used by Weaver and upstream OpenTelemetry semconv that semlex
/// ignores, so registries importing OTel need no declarations for them.
pub(crate) const UPSTREAM: &[&str] = &["dependency_resolution", "naming_conventions", "stability"];

/// Every leaf path in an annotation map, e.g. `demo.case`.
fn leaves(prefix: &mut Vec<String>, v: &Value, out: &mut Vec<Vec<String>>) {
    match v {
        Value::Object(m) if !m.is_empty() => {
            for (k, child) in m {
                prefix.push(k.clone());
                leaves(prefix, child, out);
                prefix.pop();
            }
        }
        _ => out.push(prefix.clone()),
    }
}

fn all_leaves(a: &Annotations) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for (k, v) in a.as_map() {
        leaves(&mut vec![k.clone()], v, &mut out);
    }
    out
}

/// Checks that every annotation in the registry is built in, upstream, or
/// declared, and that declarations are usable.
pub(crate) fn validate(
    reg: &Registry,
    opts: &Options,
    hooks: &[&dyn AnnotationHook],
) -> Vec<EmitError> {
    let mut errors = Vec::new();
    for d in &opts.annotations {
        let ns = d.path[0].as_str();
        if BUILT_IN.contains(&ns) {
            errors.push((Some(d.span), format!("annotation namespace `{ns}` is built in to semlex; remove it from `annotations`")));
        }
    }
    let hook_paths: Vec<Vec<String>> = hooks
        .iter()
        .map(|h| h.key().split('.').map(String::from).collect())
        .collect();
    for (i, p) in hook_paths.iter().enumerate() {
        let key = p.join(".");
        if p.iter().any(String::is_empty) {
            errors.push((
                None,
                format!("annotation hook key `{key}` isn't a dotted path"),
            ));
        } else if BUILT_IN.contains(&p[0].as_str()) || UPSTREAM.contains(&p[0].as_str()) {
            errors.push((
                None,
                format!(
                    "annotation hook key `{key}`: namespace `{}` is reserved",
                    p[0]
                ),
            ));
        }
        if hook_paths[..i].contains(p) {
            errors.push((None, format!("two annotation hooks handle `{key}`")));
        }
        if let Some(d) = opts.annotations.iter().find(|d| d.path == *p) {
            errors.push((Some(d.span), format!("annotation `{key}` is handled by this macro's hook; remove it from `annotations`")));
        }
    }
    let declared: Vec<&[String]> = opts
        .annotations
        .iter()
        .map(|d| d.path.as_slice())
        .chain(hook_paths.iter().map(Vec::as_slice))
        .collect();
    let covered = |leaf: &[String]| {
        BUILT_IN.contains(&leaf[0].as_str())
            || UPSTREAM.contains(&leaf[0].as_str())
            || declared
                .iter()
                .any(|d| leaf.len() >= d.len() && leaf[..d.len()] == **d)
    };

    // (leaf path, where) pairs, de-duplicated.
    let mut undeclared: BTreeSet<(String, String)> = BTreeSet::new();
    let mut check = |a: &Annotations, place: String| {
        for leaf in all_leaves(a) {
            if !covered(&leaf) {
                undeclared.insert((leaf.join("."), place.clone()));
            }
        }
    };
    for a in &reg.attributes {
        check(&a.annotations, format!("attribute `{}`", a.key));
        if let semlex_schema::ValueType::Enum(def) = &a.ty {
            for m in &def.members {
                check(&m.annotations, format!("member `{}` of `{}`", m.id, a.key));
            }
        }
    }
    for g in &reg.groups {
        let noun = g.kind.tag().noun();
        check(&g.annotations, format!("{noun} `{}`", g.id));
        for a in &g.attributes {
            check(
                &a.annotations,
                format!("attribute `{}` on {noun} `{}`", a.key, g.id),
            );
        }
    }
    let known = if declared.is_empty() {
        "none".to_string()
    } else {
        declared
            .iter()
            .map(|d| format!("`{}`", d.join(".")))
            .collect::<Vec<_>>()
            .join(", ")
    };
    const MAX: usize = 10;
    for (path, place) in undeclared.iter().take(MAX) {
        errors.push((None, format!(
            "annotation `{path}` on {place} isn't declared: add it to the `annotations` option of `registry!` as a `#[semlex::handler]` module or `ignore` (declared: {known})"
        )));
    }
    if undeclared.len() > MAX {
        errors.push((
            None,
            format!(
                "… and {} more undeclared annotations",
                undeclared.len() - MAX
            ),
        ));
    }
    errors
}

/// A handler call for one attribute: the module path and its literal argument.
pub(crate) struct HandlerCall {
    /// The `#[semlex::handler]` module.
    pub path: TokenStream,
    /// The annotation's value, as a Rust literal.
    pub arg: TokenStream,
}

fn literal(v: &Value) -> Option<TokenStream> {
    Some(match v {
        Value::String(s) => quote!(#s),
        Value::Bool(b) => quote!(#b),
        Value::Number(n) if n.is_i64() => {
            let i = n.as_i64().expect("is_i64");
            quote!(#i)
        }
        Value::Number(n) => {
            let f = n.as_f64()?;
            quote!(#f)
        }
        _ => return None,
    })
}

/// The handlers that apply to an attribute definition, built-ins first, then
/// declared handlers in declaration order.
pub(crate) fn handlers_for(
    key: &str,
    a: &Annotations,
    opts: &Options,
) -> Result<Vec<HandlerCall>, String> {
    let mut out = Vec::new();
    match a.sensitivity() {
        Ok(Some(_)) => out.push(HandlerCall {
            path: quote!(::semlex::handlers::sensitivity),
            arg: quote!("pii"),
        }),
        Ok(None) => {}
        Err(e) => return Err(format!("attribute `{key}`: {e}")),
    }
    for d in &opts.annotations {
        let Handling::Handler(path) = &d.handling else {
            continue;
        };
        let segs: Vec<&str> = d.path.iter().map(String::as_str).collect();
        if let Some(v) = a.at(&segs) {
            let arg = literal(v).ok_or_else(|| {
                format!(
                    "annotation `{}` on attribute `{key}` must be a string, number or boolean to be passed to its handler, found `{v}`",
                    d.path.join(".")
                )
            })?;
            out.push(HandlerCall {
                path: quote!(#path),
                arg,
            });
        }
    }
    Ok(out)
}
