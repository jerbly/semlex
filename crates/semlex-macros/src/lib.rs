//! Proc-macro shim for semlex: `registry!()` and the call-site macros.
//!
//! Users depend on the `semlex` facade crate, not on this crate directly. The
//! logic lives in `semlex-expand`.

use proc_macro::TokenStream;

/// Reads a Weaver `resolved/2.0` registry and expands it into Rust items.
/// See the `semlex` crate docs.
#[proc_macro]
pub fn registry(input: TokenStream) -> TokenStream {
    semlex_expand::registry_from_env(input.into(), &[]).into()
}

/// Marks an annotation handler module and fills in the hooks it leaves out.
/// See the `semlex` crate docs.
#[proc_macro_attribute]
pub fn handler(attr: TokenStream, item: TokenStream) -> TokenStream {
    semlex_expand::handler(attr.into(), item.into()).into()
}

/// `metric!(name: value, { key: value, … })`, reached through the registry
/// crate's forwarder, which passes `$crate` first.
#[doc(hidden)]
#[proc_macro]
pub fn metric(input: TokenStream) -> TokenStream {
    semlex_expand::metric(input.into()).into()
}

/// `event!(name { key: value, … })`, reached through the registry crate's
/// forwarder, which passes `$crate` first.
#[doc(hidden)]
#[proc_macro]
pub fn event(input: TokenStream) -> TokenStream {
    semlex_expand::event(input.into()).into()
}

/// `span!(type { key: value, … }, options…)`, reached through the registry
/// crate's forwarder, which passes `$crate` first.
#[doc(hidden)]
#[proc_macro]
pub fn span(input: TokenStream) -> TokenStream {
    semlex_expand::span(input.into()).into()
}

/// `record!(span, key: value, …)`, reached through the registry crate's
/// forwarder, which passes `$crate` first.
#[doc(hidden)]
#[proc_macro]
pub fn record(input: TokenStream) -> TokenStream {
    semlex_expand::record(input.into()).into()
}

/// `entity!(type { key: value, … })`, reached through the registry crate's
/// forwarder, which passes `$crate` first.
#[doc(hidden)]
#[proc_macro]
pub fn entity(input: TokenStream) -> TokenStream {
    semlex_expand::entity(input.into()).into()
}

/// `info!`/`warn!`/`error!`/`debug!`/`trace!`, reached through the registry
/// crate's forwarders, which pass `$crate` and the level first.
#[doc(hidden)]
#[proc_macro]
pub fn log(input: TokenStream) -> TokenStream {
    semlex_expand::log(input.into()).into()
}

/// `assert_span!`/`assert_metric!`/`assert_event!`, reached through the registry
/// crate's forwarders, which pass `$crate` and the signal kind first.
#[doc(hidden)]
#[proc_macro]
pub fn assert_signal(input: TokenStream) -> TokenStream {
    semlex_expand::assert_signal(input.into()).into()
}

/// `attrs![key: value, …]`, reached through the registry crate's forwarder, which
/// passes `$crate` first. See the `semlex` crate docs.
#[doc(hidden)]
#[proc_macro]
pub fn attrs(input: TokenStream) -> TokenStream {
    semlex_expand::attrs(input.into()).into()
}

/// `end!(span, error: &e, error.type: class)`, reached through the registry
/// crate's forwarder, which passes `$crate` first.
#[doc(hidden)]
#[proc_macro]
pub fn end(input: TokenStream) -> TokenStream {
    semlex_expand::end(input.into()).into()
}
