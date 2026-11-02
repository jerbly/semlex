//! Expansion engine for semlex, and the API for custom `registry!` macros.
//!
//! Most registries need only `semlex::registry!` and handler modules. For
//! annotations handlers can't express (emitting items per signal, reading
//! structured values), write a proc-macro crate that calls
//! [`registry_with_hooks`] with your [`AnnotationHook`]s:
//!
//! ```ignore
//! #[proc_macro]
//! pub fn registry(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
//!     semlex_expand::registry_from_env(input.into(), &[&SloHook]).into()
//! }
//!
//! struct SloHook;
//!
//! impl semlex_expand::AnnotationHook for SloHook {
//!     fn key(&self) -> &str { "org.slo" }
//!     fn on_metric(&self, metric: &Group, value: &Value, out: &mut Emit<'_>) {
//!         // read `value`, then `out.items(&["slo"], quote! { … })` or `out.error(…)`
//!     }
//! }
//! ```
//!
//! The registry crate invokes your macro instead of `semlex::registry!`, and still
//! depends on `semlex`. The stable API is [`registry`],
//! [`registry_with_hooks`], [`registry_from_env`], [`AnnotationHook`],
//! [`Emit`] and the [`schema`] types; everything else is `semlex`'s own
//! plumbing. See `docs/custom-macros.md`.

mod annotations;
mod callsite;
mod emit;
mod handler;
mod hook;
#[doc(hidden)]
pub mod options;

#[doc(hidden)]
pub use callsite::{assert_signal, attrs, end, entity, event, log, metric, record, span};
#[doc(hidden)]
pub use handler::handler;
pub use hook::{AnnotationHook, Emit};
/// The registry model hooks receive (`semlex-schema`).
pub use semlex_schema as schema;
/// Annotation values (`serde_json`).
pub use serde_json::Value;

use proc_macro2::TokenStream;
use quote::quote;
use std::path::Path;

/// `registry!("path/to/resolved.yaml", { options })`.
///
/// `manifest_dir` is the invoking crate's `CARGO_MANIFEST_DIR`, which the path
/// is resolved against; `crate_name` is its package name, used in messages.
pub fn registry(input: TokenStream, manifest_dir: &Path, crate_name: &str) -> TokenStream {
    registry_with_hooks(input, manifest_dir, crate_name, &[])
}

/// [`registry_with_hooks`] for the crate being compiled, from Cargo's
/// environment: the body of a custom `registry!` proc macro.
pub fn registry_from_env(input: TokenStream, hooks: &[&dyn AnnotationHook]) -> TokenStream {
    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    let crate_name = std::env::var("CARGO_PKG_NAME").unwrap_or_default();
    registry_with_hooks(input, &manifest_dir, &crate_name, hooks)
}

/// [`registry`] with annotation hooks: each hook's key counts as declared,
/// and its items are emitted alongside the registry's.
pub fn registry_with_hooks(
    input: TokenStream,
    manifest_dir: &Path,
    crate_name: &str,
    hooks: &[&dyn AnnotationHook],
) -> TokenStream {
    let input: options::RegistryInput = match syn::parse2(input) {
        Ok(i) => i,
        Err(e) => return e.to_compile_error(),
    };
    let lit = &input.path;
    let path = manifest_dir.join(lit.value());
    let error = |msg: String| syn::Error::new(lit.span(), msg).to_compile_error();

    let registry = match semlex_schema::load(&path) {
        Ok(r) => r,
        Err(e @ semlex_schema::Error::Io { .. }) => {
            return error(format!(
                "{e} (paths are relative to the crate's Cargo.toml directory)"
            ));
        }
        Err(e) => return error(e.to_string()),
    };
    let names = match semlex_schema::names::names(&registry) {
        Ok(n) => n,
        Err(errors) => return errors.iter().map(|e| error(e.to_string())).collect(),
    };

    let items = match emit::registry(&registry, &names, &input.options, crate_name, hooks) {
        Ok(items) => items,
        Err(errors) => {
            return errors
                .into_iter()
                .map(|(span, msg)| {
                    syn::Error::new(span.unwrap_or_else(|| lit.span()), msg).to_compile_error()
                })
                .collect();
        }
    };
    let tracked = path.to_string_lossy().into_owned();
    quote! {
        #items
        const _: &[u8] = include_bytes!(#tracked);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
    }

    #[test]
    fn valid_registry_tracks_the_file() {
        let out = registry(
            quote!("resolved/resolved.yaml", { annotations: { demo.case: ignore, demo.slo: ignore } }),
            &fixtures(),
            "demo",
        )
        .to_string();
        assert!(out.contains("include_bytes !"), "{out}");
        assert!(out.contains("fixtures/resolved/resolved.yaml"), "{out}");
    }

    #[test]
    fn errors_are_compile_errors() {
        let out = registry(quote!("missing.yaml"), &fixtures(), "demo").to_string();
        assert!(out.contains("compile_error"), "{out}");
        assert!(
            out.contains("relative to the crate's Cargo.toml directory"),
            "{out}"
        );
        let out = registry(quote!(42), &fixtures(), "demo").to_string();
        assert!(
            out.contains("expected the path of a resolved registry"),
            "{out}"
        );
        let out = registry(
            quote!("resolved/resolved.yaml", { colour: blue }),
            &fixtures(),
            "demo",
        )
        .to_string();
        assert!(out.contains("unknown `registry!` option `colour`"), "{out}");
    }
}
