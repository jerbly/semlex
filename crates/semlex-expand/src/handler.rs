//! `#[semlex::handler]` on an annotation handler module.
//!
//! `registry!()` can't see inside a module, so it calls every hook on every
//! handler. This attribute adds no-op defaults for the hooks a module leaves
//! out, and a marker `registry!()` checks first:
//!
//! - `pub const fn check_literal(value, arg)`: compile-time literal check;
//! - `pub fn apply(value: ::semlex::Value, arg) -> ::semlex::Value`: runtime transform;
//! - `pub type Kind<K>`: the value kind an attribute takes (identity by default).

use proc_macro2::TokenStream;
use quote::quote;
use syn::{Item, ItemMod};

/// Expands `#[semlex::handler]` on `item`.
pub fn handler(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        return syn::Error::new_spanned(attr, "`#[semlex::handler]` takes no arguments")
            .to_compile_error();
    }
    let mut module: ItemMod = match syn::parse2(item) {
        Ok(m) => m,
        Err(e) => {
            return syn::Error::new(
                e.span(),
                "`#[semlex::handler]` goes on an inline module: `pub mod name { … }`",
            )
            .to_compile_error();
        }
    };
    let Some((_, items)) = module.content.as_mut() else {
        return syn::Error::new_spanned(
            &module,
            "`#[semlex::handler]` needs an inline module body, not `mod name;`",
        )
        .to_compile_error();
    };
    let has_fn = |items: &[Item], name: &str| {
        items
            .iter()
            .any(|i| matches!(i, Item::Fn(f) if f.sig.ident == name))
    };
    let has_type = |items: &[Item], name: &str| {
        items
            .iter()
            .any(|i| matches!(i, Item::Type(t) if t.ident == name))
    };

    let mut defaults: Vec<Item> = Vec::new();
    if !has_fn(items, "check_literal") {
        defaults.push(syn::parse_quote! {
            /// No compile-time check (default added by `#[semlex::handler]`).
            #[allow(dead_code)]
            pub const fn check_literal<V, A>(value: V, arg: A) {
                ::core::mem::forget(value);
                ::core::mem::forget(arg);
            }
        });
    }
    if !has_fn(items, "apply") {
        defaults.push(syn::parse_quote! {
            /// No runtime transform (default added by `#[semlex::handler]`).
            #[allow(dead_code)]
            pub fn apply<A>(value: ::semlex::Value, _arg: A) -> ::semlex::Value {
                value
            }
        });
    }
    if !has_type(items, "Kind") {
        defaults.push(syn::parse_quote! {
            /// The attribute's value kind, unchanged (default added by `#[semlex::handler]`).
            pub type Kind<K> = K;
        });
    }
    defaults.push(syn::parse_quote! {
        #[doc(hidden)]
        pub const __MARK_THIS_MODULE_WITH_SEMLEX_HANDLER: () = ();
    });
    items.extend(defaults);
    quote!(#module)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_only_missing_hooks() {
        let out = handler(
            quote!(),
            quote! {
                pub mod case {
                    pub const fn check_literal(value: &str, arg: &str) {}
                }
            },
        )
        .to_string();
        assert_eq!(out.matches("fn check_literal").count(), 1, "{out}");
        assert!(out.contains("pub fn apply"), "{out}");
        assert!(out.contains("pub type Kind"), "{out}");
        assert!(
            out.contains("__MARK_THIS_MODULE_WITH_SEMLEX_HANDLER"),
            "{out}"
        );
    }

    #[test]
    fn rejects_non_modules() {
        let out = handler(
            quote!(),
            quote!(
                fn f() {}
            ),
        )
        .to_string();
        assert!(out.contains("goes on an inline module"), "{out}");
    }
}
