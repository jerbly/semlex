//! Call-site macros. They never read the registry: they turn dotted names
//! into paths under the registry crate and route values through its hooks,
//! keeping the user's spans on every token that can carry an error.

use proc_macro2::{Delimiter, Ident, Span, TokenStream, TokenTree};
use quote::{quote, quote_spanned};

type Tokens = std::iter::Peekable<std::vec::IntoIter<TokenTree>>;

fn tokens(ts: TokenStream) -> Tokens {
    ts.into_iter().collect::<Vec<_>>().into_iter().peekable()
}

fn is_punct(t: Option<&TokenTree>, c: char) -> bool {
    matches!(t, Some(TokenTree::Punct(p)) if p.as_char() == c)
}

/// A compile error usable where the macro expands to an expression.
fn error(span: Span, msg: &str) -> TokenStream {
    quote_spanned!(span=> { ::core::compile_error!(#msg) })
}

/// A dotted name, each segment with the user's span.
struct Dotted(Vec<Ident>);

impl Dotted {
    /// Each segment as the path segment `registry!()` emitted for it
    /// (keywords become raw identifiers: `type` → `r#type`), with the user's
    /// span, using the same mangling as `semlex_schema::names::ident`.
    fn path_segments(&self) -> Vec<Ident> {
        self.0
            .iter()
            .map(|seg| {
                let name = semlex_schema::names::ident(&seg.to_string());
                match name.strip_prefix("r#") {
                    Some(raw) => Ident::new_raw(raw, seg.span()),
                    None => Ident::new(&name, seg.span()),
                }
            })
            .collect()
    }

    fn span(&self) -> Span {
        self.0[0].span()
    }
    fn text(&self) -> String {
        self.0
            .iter()
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(".")
    }
    /// `__semlex_attr::a::b::c` under the user's spans.
    fn attr_path(&self) -> TokenStream {
        let alias = Ident::new("__semlex_attr", self.span());
        let segs = self.path_segments();
        quote_spanned! {self.span()=> #alias #(::#segs)* }
    }
}

fn parse_dotted(it: &mut Tokens) -> Result<Dotted, (Span, String)> {
    let mut segs = Vec::new();
    loop {
        match it.next() {
            Some(TokenTree::Ident(i)) => segs.push(i),
            Some(t) => {
                return Err((
                    t.span(),
                    "expected a dotted attribute name, e.g. `http.request.method`".into(),
                ));
            }
            None => {
                return Err((
                    Span::call_site(),
                    "expected a dotted attribute name, e.g. `http.request.method`".into(),
                ));
            }
        }
        if is_punct(it.peek(), '.') {
            it.next();
        } else {
            return Ok(Dotted(segs));
        }
    }
}

/// Tokens up to the next top-level `,` (consumed) or the end.
fn take_until_comma(it: &mut Tokens) -> Vec<TokenTree> {
    let mut out = Vec::new();
    while let Some(t) = it.peek() {
        if is_punct(Some(t), ',') {
            it.next();
            break;
        }
        out.push(it.next().expect("peeked"));
    }
    out
}

/// A value: a literal (checked at compile time), `custom(…)`, or any expression.
#[derive(Clone)]
enum ValueExpr {
    Literal(TokenStream),
    Custom {
        inner: TokenStream,
        literal: bool,
        span: Span,
    },

    /// `redacted(…)` / `hashed(…)` for `sensitivity: pii` attributes.
    Pii {
        func: Ident,
        inner: TokenStream,
    },
    Expr(TokenStream),
}

fn is_literal(v: &[TokenTree]) -> bool {
    match v {
        [TokenTree::Literal(_)] => true,
        [TokenTree::Ident(i)] => i == "true" || i == "false",
        [TokenTree::Punct(p), TokenTree::Literal(l)] => {
            p.as_char() == '-' && !l.to_string().starts_with('"')
        }
        _ => false,
    }
}

/// A literal with the type OpenTelemetry stores: integers become `i64` and
/// floats `f64`, suffixed or not. Unsuffixed, an integer would default to
/// `i32` in the generic value check and overflow above `i32::MAX`; a suffix
/// (`1usize`) would skip the literal hooks (enum members, handlers).
fn normalize_literal(v: Vec<TokenTree>) -> TokenStream {
    v.into_iter()
        .map(|t| match &t {
            TokenTree::Literal(l) => {
                let typed = match syn::parse_str::<syn::Lit>(&l.to_string()) {
                    Ok(syn::Lit::Int(i)) => Some(format!("{}i64", i.base10_digits())),
                    Ok(syn::Lit::Float(f)) => Some(format!("{}f64", f.base10_digits())),
                    _ => None,
                };
                match typed.and_then(|t| t.parse::<proc_macro2::Literal>().ok()) {
                    Some(mut lit) => {
                        lit.set_span(l.span());
                        TokenTree::Literal(lit)
                    }
                    None => t,
                }
            }
            _ => t,
        })
        .collect()
}

fn classify(v: Vec<TokenTree>) -> ValueExpr {
    if is_literal(&v) {
        return ValueExpr::Literal(normalize_literal(v));
    }
    if let [TokenTree::Ident(f), TokenTree::Group(g)] = v.as_slice()
        && f == "custom"
        && g.delimiter() == Delimiter::Parenthesis
    {
        let inner: Vec<TokenTree> = g.stream().into_iter().collect();
        let literal = is_literal(&inner);
        return ValueExpr::Custom {
            literal,
            inner: if literal {
                normalize_literal(inner)
            } else {
                inner.into_iter().collect()
            },
            span: f.span(),
        };
    }
    if let [TokenTree::Ident(f), TokenTree::Group(g)] = v.as_slice()
        && (f == "redacted" || f == "hashed")
        && g.delimiter() == Delimiter::Parenthesis
    {
        return ValueExpr::Pii {
            func: f.clone(),
            inner: g.stream(),
        };
    }
    ValueExpr::Expr(v.into_iter().collect())
}

/// One `key: value` (or `key(suffix): value`, or `key?: option`) entry.
struct Entry {
    key: Dotted,
    suffix: Option<TokenStream>,
    /// `key?: option`: where the `?` is.
    optional: Option<Span>,
    value: ValueExpr,
    /// `extra(key, reviewed: "why")`: approved in the macro, so no warning
    /// and no clippy marker.
    reviewed: bool,
}

fn parse_entries(it: &mut Tokens) -> Result<Vec<Entry>, (Span, String)> {
    let mut out = Vec::new();
    while it.peek().is_some() {
        let key = parse_dotted(it)?;
        let suffix = match it.peek() {
            Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis => {
                let s = g.stream();
                it.next();
                Some(s)
            }
            _ => None,
        };
        let optional = match it.peek() {
            Some(TokenTree::Punct(p)) if p.as_char() == '?' => {
                let span = p.span();
                it.next();
                Some(span)
            }
            _ => None,
        };
        if !is_punct(it.peek(), ':') {
            return Err((
                key.span(),
                format!("expected `:` and a value after `{}`", key.text()),
            ));
        }
        it.next();
        let value = take_until_comma(it);
        if value.is_empty() {
            return Err((key.span(), format!("expected a value for `{}`", key.text())));
        }
        out.push(Entry {
            reviewed: false,
            key,
            suffix,
            optional,
            value: classify(value),
        });
    }
    Ok(out)
}

/// Splits off the leading `krate;` passed by the registry crate's forwarder.
fn split_krate(input: TokenStream) -> (TokenStream, Tokens) {
    let mut it = tokens(input);
    let mut krate = TokenStream::new();
    for t in it.by_ref() {
        if is_punct(Some(&t), ';') {
            break;
        }
        krate.extend([t]);
    }
    (krate, it)
}

/// `use $crate::attr as __semlex_attr; use $crate::__semlex::__private as __semlex_rt;`
/// under a user span, so every path built from them shares the user's
/// syntax context.
fn aliases(krate: &TokenStream, at: Span) -> TokenStream {
    let attr = Ident::new("__semlex_attr", at);
    let rt = Ident::new("__semlex_rt", at);
    let reg = Ident::new("__semlex_reg", at);
    quote_spanned! {at=>
        #[allow(unused_imports)]
        use #krate::attr as #attr;
        #[allow(unused_imports)]
        use #krate::__semlex::__private as #rt;
        #[allow(unused_imports)]
        use #krate::__semlex_registry as #reg;
    }
}

/// `key, reviewed: "why"` inside `extra(…)` or `unregistered(…)`: the key's
/// tokens, and whether the use is approved. The reason must be a non-empty
/// string literal; it's for review (`scripts/escape-report.sh` lists it).
fn split_reviewed(suffix: &TokenStream) -> Result<(TokenStream, bool), TokenStream> {
    let toks: Vec<TokenTree> = suffix.clone().into_iter().collect();
    let Some(comma) = toks.iter().position(|t| is_punct(Some(t), ',')) else {
        return Ok((suffix.clone(), false));
    };
    let usage = "expected `reviewed: \"why\"` after the key, to approve this use";
    let rest = &toks[comma + 1..];
    let rest = match rest {
        [rest @ .., TokenTree::Punct(p)] if p.as_char() == ',' => rest,
        rest => rest,
    };
    match rest {
        [] => Ok((toks[..comma].iter().cloned().collect(), false)),
        [
            TokenTree::Ident(i),
            TokenTree::Punct(p),
            TokenTree::Literal(l),
        ] if i == "reviewed" && p.as_char() == ':' => {
            match syn::parse_str::<syn::LitStr>(&l.to_string()) {
                Ok(why) if !why.value().trim().is_empty() => {
                    Ok((toks[..comma].iter().cloned().collect(), true))
                }
                _ => Err(error(
                    l.span(),
                    "`reviewed:` takes the reason as a non-empty string literal",
                )),
            }
        }
        [t, ..] => Err(error(t.span(), usage)),
    }
}

/// `unregistered("key"): value`: the key literal, the markers to
/// put before the value, and the value.
struct Unregistered {
    at: Span,
    key: proc_macro2::Literal,
    markers: TokenStream,
    value: TokenStream,
}

/// `Some` for an `unregistered("…")` entry (or the error for a malformed one).
fn as_unregistered(e: &Entry) -> Option<Result<Unregistered, TokenStream>> {
    if e.key.0.len() != 1 || e.key.0[0] != "unregistered" {
        return None;
    }
    let suffix = e.suffix.as_ref()?;
    let (suffix, reviewed) = match split_reviewed(suffix) {
        Ok(v) => v,
        Err(err) => return Some(Err(err)),
    };
    let toks: Vec<TokenTree> = suffix.into_iter().collect();
    let key = match toks.as_slice() {
        [TokenTree::Literal(l)] if l.to_string().starts_with('"') => l.clone(),
        _ => {
            return Some(Err(error(
                e.key.span(),
                "expected the key as a string literal: `unregistered(\"retry.attempt\"): value`",
            )));
        }
    };
    let value = match &e.value {
        ValueExpr::Literal(v) | ValueExpr::Expr(v) => v.clone(),
        ValueExpr::Custom { span, .. } => {
            return Some(Err(error(
                *span,
                "`custom(…)` is for registry enums; an unregistered attribute takes a plain value",
            )));
        }
        ValueExpr::Pii { func, .. } => {
            return Some(Err(error(
                func.span(),
                "an unregistered attribute takes a plain value; PII attributes belong in the registry",
            )));
        }
    };
    let text = key.to_string();
    let text = text.trim_matches('"');
    let at = key.span();
    let (rt, reg) = (
        Ident::new("__semlex_rt", e.key.span()),
        Ident::new("__semlex_reg", e.key.span()),
    );
    let note =
        format!("`{text}` is not in the semantic-convention registry; add it to the registry");
    let marker = Ident::new("__semlex_unregistered", at);
    let check = quote_spanned! {at=>
        const _: () = #rt::check_unregistered(#key, #reg::ATTRIBUTE_KEYS, #reg::TEMPLATE_PREFIXES);
    };
    let markers = if reviewed {
        check
    } else {
        quote_spanned! {at=>
            #check
            #[deprecated(note = #note)]
            fn #marker() {}
            #marker();
            #rt::unregistered_attribute();
        }
    };
    Some(Ok(Unregistered {
        at,
        key,
        markers,
        value,
    }))
}

/// `extra(key): value`: a registry attribute the signal doesn't define. The
/// inner entry, typed and checked like any registry attribute.
fn as_extra(e: &Entry) -> Option<Result<Entry, TokenStream>> {
    if e.key.0.len() != 1 || e.key.0[0] != "extra" {
        return None;
    }
    let suffix = e.suffix.as_ref()?;
    let usage = "expected a registry attribute: `extra(error.type): value`";
    let (suffix, reviewed) = match split_reviewed(suffix) {
        Ok(v) => v,
        Err(err) => return Some(Err(err)),
    };
    let mut it = tokens(suffix);
    let key = match parse_dotted(&mut it) {
        Ok(k) => k,
        Err(_) => return Some(Err(error(e.key.span(), usage))),
    };
    let inner_suffix = match it.next() {
        Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis => Some(g.stream()),
        None => None,
        Some(t) => return Some(Err(error(t.span(), usage))),
    };
    if let Some(t) = it.next() {
        return Some(Err(error(t.span(), usage)));
    }
    Some(Ok(Entry {
        key,
        suffix: inner_suffix,
        optional: e.optional,
        value: e.value.clone(),
        reviewed,
    }))
}

/// The error for `extra(…)` where every registry attribute is already allowed.
fn extra_not_needed(e: &Entry, inner: &Entry) -> TokenStream {
    error(
        e.key.span(),
        &format!(
            "`extra(…)` is for signals; here any registry attribute is allowed: write `{}: …`",
            inner.key.text()
        ),
    )
}

/// One entry, translated: its `const _` checks and its value expression.
struct Translated {
    at: Span,
    checks: TokenStream,
    /// `Some(key expr)` for template attributes (`P.key(suffix)`).
    template: Option<TokenStream>,
    path: TokenStream,
    value: TokenStream,
}

fn translate(e: &Entry) -> Translated {
    let at = e.key.span();
    let path = e.key.attr_path();
    let rt = Ident::new("__semlex_rt", at);
    let template = e
        .suffix
        .as_ref()
        .map(|suffix| quote_spanned!(at=> #path.key(#suffix)));
    let (checks, value) = match &e.value {
        // Template attributes have no literal hooks.
        ValueExpr::Literal(v) if template.is_some() => {
            (quote_spanned!(at=> #path.check_usable();), v.clone())
        }
        ValueExpr::Literal(lit) => (
            quote_spanned!(at=> #path.check_usable(); #path.check_literal(#lit);),
            quote_spanned!(at=> #path.literal(#lit)),
        ),
        ValueExpr::Custom {
            inner,
            literal: true,
            span,
        } if template.is_none() => (
            quote_spanned!(at=> #path.check_usable(); #path.check_custom(#inner);),
            quote_spanned!(*span=> #rt::custom(#inner)),
        ),
        ValueExpr::Custom { inner, span, .. } => (
            quote_spanned!(at=> #path.check_usable();),
            quote_spanned!(*span=> #rt::custom(#inner)),
        ),
        ValueExpr::Pii { func, inner } => (
            quote_spanned!(at=> #path.check_usable();),
            quote_spanned!(func.span()=> #rt::#func(#inner)),
        ),
        ValueExpr::Expr(expr) => (quote_spanned!(at=> #path.check_usable();), expr.clone()),
    };
    Translated {
        at,
        checks: quote_spanned!(at=> const _: () = { #checks };),
        template,
        path,
        value,
    }
}

/// The `KeyValue` expression for one `attrs!` entry, with its `const _` checks.
fn entry_kv(e: &Entry) -> TokenStream {
    if let Some(at) = e.optional {
        return optional_not_here(at);
    }
    match as_extra(e) {
        Some(Ok(inner)) => return extra_not_needed(e, &inner),
        Some(Err(err)) => return err,
        None => {}
    }
    match as_unregistered(e) {
        Some(Ok(Unregistered {
            at,
            key,
            markers,
            value,
        })) => {
            let rt = Ident::new("__semlex_rt", e.key.span());
            return quote_spanned! {at=> { #markers #rt::unregistered_kv(#key, #value) }};
        }
        Some(Err(err)) => return err,
        None => {}
    }
    let Translated {
        at,
        checks,
        template,
        path,
        value,
    } = translate(e);
    let rt = Ident::new("__semlex_rt", at);
    match template {
        Some(key) => quote_spanned! {at=> { #checks #rt::kv_template(#key, #value) }},
        None => quote_spanned! {at=> { #checks #rt::kv(#path, #value) }},
    }
}

/// `let __semlex_b = __semlex_b.with(…);` for one signal entry, spanned at the
/// key so membership errors point there.
///
/// `group` is the signal's module when the macro names it (`span!`,
/// `metric!`, …), and `None` for `record!(span, …)`.
fn entry_with(e: &Entry, var: &str, metric: bool, group: Option<&TokenStream>) -> TokenStream {
    if let Some(at) = e.optional {
        return optional_with(e, at, var);
    }
    match as_extra(e) {
        Some(Ok(inner)) => return extra_with(&inner, var, metric, group),
        Some(Err(err)) => return quote!(let _ = #err;),
        None => {}
    }
    match as_unregistered(e) {
        Some(Ok(Unregistered {
            at,
            key,
            markers,
            value,
        })) => {
            let rt = Ident::new("__semlex_rt", e.key.span());
            let b = Ident::new(var, e.key.span());
            let with = Ident::new("with_unregistered", at);
            // Its own block: each use defines its own marker function.
            return quote_spanned! {at=> let #b = { #markers #b.#with(#rt::unregistered_kv(#key, #value)) }; };
        }
        Some(Err(err)) => return quote!(let _ = #err;),
        None => {}
    }
    let Translated {
        at,
        checks,
        template,
        path,
        value,
    } = translate(e);
    let b = Ident::new(var, at);
    match template {
        Some(key) => {
            let with = Ident::new("with_template", at);
            quote_spanned! {at=> #checks let #b = #b.#with(#key, #value); }
        }
        None => {
            let with = Ident::new("with", at);
            quote_spanned! {at=> #checks let #b = #b.#with(#path, #value); }
        }
    }
}

/// The error for `key?:` where it isn't supported.
fn optional_not_here(at: Span) -> TokenStream {
    error(
        at,
        "`key?: option` is for signal macros (`span!`, `record!(span, …)`, `metric!`, `event!`, `entity!`); here, unwrap the option first",
    )
}

/// The option expression for a `key?: option` entry (wrappers apply to the
/// value inside), or the error statement.
fn optional_value(e: &Entry, at: Span) -> Result<TokenStream, TokenStream> {
    let fail = |msg: &str| {
        let err = error(at, msg);
        Err(quote!(let _ = #err;))
    };
    if e.key.0.len() == 1 && (e.key.0[0] == "unregistered" || e.key.0[0] == "extra") {
        return fail(
            "`?` can't be combined with `unregistered(…)` or `extra(…)`; unwrap the option first",
        );
    }
    if e.suffix.is_some() {
        return fail("`?` isn't supported for template attributes yet; unwrap the option first");
    }
    let rt = Ident::new("__semlex_rt", e.key.span());
    match &e.value {
        ValueExpr::Literal(_) => fail("a literal is never `None`: drop the `?`"),
        ValueExpr::Expr(v) => Ok(v.clone()),
        ValueExpr::Custom { inner, span, .. } => {
            Ok(quote_spanned!(*span=> (#inner).map(#rt::custom)))
        }
        ValueExpr::Pii { func, inner } => {
            Ok(quote_spanned!(func.span()=> (#inner).map(#rt::#func)))
        }
    }
}

/// `let b = b.with_opt(path, option);` for a `key?: option` entry: the
/// attribute is set if the value is `Some`, and never satisfies a requirement.
fn optional_with(e: &Entry, at: Span, var: &str) -> TokenStream {
    let value = match optional_value(e, at) {
        Ok(v) => v,
        Err(err) => return err,
    };
    let kat = e.key.span();
    let path = e.key.attr_path();
    let b = Ident::new(var, kat);
    let rt = Ident::new("__semlex_rt", kat);
    let with = Ident::new("with", kat);
    quote_spanned! {kat=>
        const _: () = { #path.check_usable(); };
        let #b = #rt::with_opt(#b, #value, |__semlex_b, __semlex_v| __semlex_b.#with(#path, __semlex_v));
    }
}

/// One `record!(&span, …)` entry: recorded through the shared reference
/// `var`, keeping the membership check. Only attributes whose recording
/// doesn't change the span's state are allowed (`InPlace`).
fn entry_in_place(e: &Entry, var: &str) -> TokenStream {
    let kat = e.key.span();
    let b = Ident::new(var, kat);
    let rt = Ident::new("__semlex_rt", kat);
    if let Some(at) = e.optional {
        let value = match optional_value(e, at) {
            Ok(v) => v,
            Err(err) => return err,
        };
        let path = e.key.attr_path();
        let set = in_place(&b, &rt, &path, quote_spanned!(kat=> __semlex_v), kat);
        return quote_spanned! {kat=>
            const _: () = { #path.check_usable(); };
            if let ::std::option::Option::Some(__semlex_v) = #value {
                #set
            }
        };
    }
    // Escape hatches: no membership to check, so set on the span directly.
    let raw = |markers: TokenStream, kv: TokenStream| {
        quote_spanned! {kat=> {
            #markers
            #b.__semlex_core().set(#kv, #rt::INTERNAL);
        } }
    };
    match as_unregistered(e) {
        Some(Ok(Unregistered {
            key,
            markers,
            value,
            ..
        })) => {
            return raw(
                markers,
                quote_spanned!(kat=> #rt::unregistered_kv(#key, #value)),
            );
        }
        Some(Err(err)) => return quote!(let _ = #err;),
        None => {}
    }
    match as_extra(e) {
        Some(Ok(inner)) => {
            let Translated {
                checks,
                template,
                path,
                value,
                at,
            } = translate(&inner);
            if template.is_some() {
                return error(
                    at,
                    "templates aren't supported in `record!(&span, …)` yet; use `let span = record!(span, …)`",
                );
            }
            let warn = extra_markers(&inner, at, &rt);
            let markers = quote_spanned! {at=>
                #checks
                #rt::extra_on(&#b, &#path);
                #warn
            };
            return raw(markers, quote_spanned!(at=> #rt::kv(#path, #value)));
        }
        Some(Err(err)) => return quote!(let _ = #err;),
        None => {}
    }
    let Translated {
        at,
        checks,
        template,
        path,
        value,
    } = translate(e);
    if template.is_some() {
        return error(
            at,
            "templates aren't supported in `record!(&span, …)` yet; use `let span = record!(span, …)`",
        );
    }
    let rt = Ident::new("__semlex_rt", at);
    let set = in_place(&b, &rt, &path, value, at);
    quote_spanned! {at=> #checks #set }
}

/// `set_in_place(span, value, |v| kv(path, v), |b, v| b.with(path, v));`:
/// the second closure is never called; it type-checks the span's own `with`
/// (membership, sampling-relevance) and that the state doesn't change.
fn in_place(
    b: &Ident,
    rt: &Ident,
    path: &TokenStream,
    value: TokenStream,
    at: Span,
) -> TokenStream {
    let with = Ident::new("with", at);
    let set = Ident::new("__semlex_set_in_place", at);
    quote_spanned! {at=>
        #b.#set(
            #value,
            |__semlex_v| #rt::kv(#path, __semlex_v),
            |__semlex_s, __semlex_v| __semlex_s.#with(#path, __semlex_v),
        );
    }
}

/// `let b = { markers; b.with_unregistered(kv) };` for an `extra(…)` entry:
/// typed and checked as a registry attribute, without the membership check,
/// and flagged like `unregistered(…)`.
fn extra_with(inner: &Entry, var: &str, metric: bool, group: Option<&TokenStream>) -> TokenStream {
    let Translated {
        at,
        checks,
        template,
        path,
        value,
    } = translate(inner);
    let rt = Ident::new("__semlex_rt", at);
    let b = Ident::new(var, at);
    let with = Ident::new("with_unregistered", at);
    let key = inner.key.text();
    let bounded = if metric {
        quote_spanned!(at=> const _: () = #path.check_bounded();)
    } else {
        quote!()
    };
    let (kv, member) = match template {
        Some(k) => (
            quote_spanned!(at=> #rt::kv_template(#k, #value)),
            quote_spanned!(at=> #rt::extra_template_on),
        ),
        None => (
            quote_spanned!(at=> #rt::kv(#path, #value)),
            quote_spanned!(at=> #rt::extra_on),
        ),
    };
    // Not a member: a plain `const` where the signal is known (reported by
    // `cargo check`), else the builder's own check (reported when built).
    let not_member = match group {
        Some(group) => {
            let key = key.as_str();
            quote_spanned!(at=> const _: () = #rt::check_extra(<#group::Builder as #rt::Members>::MEMBERS, #key);)
        }
        None => quote_spanned!(at=> #member(&#b, &#path);),
    };
    let markers = extra_markers(inner, at, &rt);
    // Its own block: each use defines its own marker function.
    quote_spanned! {at=> let #b = {
        #checks
        #bounded
        #not_member
        #markers
        #b.#with(#kv)
    }; }
}

/// An `extra(…)` use's warning and clippy marker, or nothing when
/// it's approved with `reviewed: "…"`.
fn extra_markers(inner: &Entry, at: Span, rt: &Ident) -> TokenStream {
    if inner.reviewed {
        return quote!();
    }
    let key = inner.key.text();
    let note = format!(
        "`{key}` is a registry attribute this signal doesn't define; add it to the signal in the registry"
    );
    let marker = Ident::new("__semlex_extra", at);
    quote_spanned! {at=>
        #[deprecated(note = #note)]
        fn #marker() {}
        #marker();
        #rt::extra_attribute();
    }
}

/// Entities never take `unregistered("…")` or `extra(…)` attributes: their
/// attributes identify them.
fn reject_unregistered(entries: &[Entry]) -> Option<TokenStream> {
    let e = entries.iter().find(|e| {
        e.key.0.len() == 1
            && (e.key.0[0] == "unregistered" || e.key.0[0] == "extra")
            && e.suffix.is_some()
    })?;
    Some(error(
        e.key.span(),
        &format!(
            "`{}(…)` attributes can't be used on entities: an entity's attributes identify it, so they must be the ones the registry defines for it",
            e.key.0[0]
        ),
    ))
}

/// `attrs![key: value, …]` → `[KeyValue; N]`.
pub fn attrs(input: TokenStream) -> TokenStream {
    let (krate, mut it) = split_krate(input);
    let entries = match parse_entries(&mut it) {
        Ok(e) => e,
        Err((span, msg)) => return error(span, &msg),
    };
    let at = entries.first().map_or(Span::call_site(), |e| e.key.span());
    let aliases = aliases(&krate, at);
    let kvs = entries.iter().map(entry_kv);
    quote! {{
        #aliases
        [#(#kvs),*]
    }}
}

/// The entries in an optional `{ … }` group.
fn braced_entries(t: Option<TokenTree>) -> Result<Vec<Entry>, (Span, String)> {
    match t {
        None => Ok(Vec::new()),
        Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Brace => {
            parse_entries(&mut tokens(g.stream()))
        }
        Some(t) => Err((
            t.span(),
            "expected `{ key: value, … }` with the attributes".into(),
        )),
    }
}

/// The group's path under its root alias, `__semlex_root::a::b`, under the
/// user's spans: an unknown name then reports the missing segment (an
/// aliased `use` of the full path would report `$crate` instead).
fn group_path(name: &Dotted) -> TokenStream {
    let root = Ident::new("__semlex_root", name.span());
    let segs = name.path_segments();
    quote_spanned! {name.span()=> #root #(::#segs)* }
}

/// The shared front of a signal expansion: aliases, the call site's scope,
/// and the builder chain.
fn signal_front(krate: &TokenStream, root: &str, name: &Dotted, entries: &[Entry]) -> TokenStream {
    let at = name.span();
    let aliases = aliases(krate, at);
    let root = Ident::new(root, at);
    let root_alias = Ident::new("__semlex_root", at);
    let group = group_path(name);
    let reg = Ident::new("__semlex_reg", at);
    let rt = Ident::new("__semlex_rt", at);
    let b = Ident::new("__semlex_b", at);
    let withs: TokenStream = entries
        .iter()
        .map(|e| entry_with(e, "__semlex_b", root == "metrics", Some(&group)))
        .collect();
    quote_spanned! {at=>
        #aliases
        #[allow(unused_imports)]
        use #krate::#root as #root_alias;
        #[allow(dead_code)]
        const __SEMLEX_SCOPE: #rt::Scope = #rt::Scope {
            prefix: #reg::SCOPE_PREFIX,
            name: ::core::env!("CARGO_PKG_NAME"),
            version: ::core::env!("CARGO_PKG_VERSION"),
            schema_url: #reg::SCHEMA_URL,
        };
        let #b = #group::builder();
        #withs
    }
}

/// `metric!(name: value, { key: value, … })`.
pub fn metric(input: TokenStream) -> TokenStream {
    let (krate, mut it) = split_krate(input);
    let name = match parse_dotted(&mut it) {
        Ok(n) => n,
        Err((span, msg)) => return error(span, &msg),
    };
    if !is_punct(it.peek(), ':') {
        return error(
            name.span(),
            &format!("expected `:` and the measurement after `{}`", name.text()),
        );
    }
    it.next();
    let mut rest: Vec<TokenTree> = it.collect();
    // `value, { … }` or `value`.
    let braces = match rest.as_slice() {
        [.., TokenTree::Punct(p), TokenTree::Group(g)]
            if p.as_char() == ',' && g.delimiter() == Delimiter::Brace =>
        {
            let g = rest.pop();
            rest.pop();
            g
        }
        _ => None,
    };
    if is_punct(rest.last(), ',') {
        rest.pop();
    }
    if rest.is_empty() {
        return error(
            name.span(),
            &format!("expected the measurement for `{}`", name.text()),
        );
    }
    // A non-negative, unsuffixed integer literal: any `int` instrument.
    let value: TokenStream = match rest.as_slice() {
        [TokenTree::Literal(l)] => match syn::parse_str::<syn::Lit>(&l.to_string()) {
            Ok(syn::Lit::Int(i)) if i.suffix().is_empty() => {
                let rt = Ident::new("__semlex_rt", l.span());
                let digits = i.base10_digits();
                let mut lit: proc_macro2::Literal =
                    format!("{digits}u64").parse().expect("integer literal");
                lit.set_span(l.span());
                quote_spanned!(l.span()=> #rt::IntLiteral(#lit))
            }
            _ => rest.into_iter().collect(),
        },
        _ => rest.into_iter().collect(),
    };
    let entries = match braced_entries(braces) {
        Ok(e) => e,
        Err((span, msg)) => return error(span, &msg),
    };
    let at = name.span();
    let front = signal_front(&krate, "metrics", &name, &entries);
    let group = group_path(&name);
    let (rt, b) = (Ident::new("__semlex_rt", at), Ident::new("__semlex_b", at));
    let measure = Ident::new("measure", at);
    quote_spanned! {at=> {
        #front
        static __SEMLEX_SITE: #rt::MetricSite<#group::Instrument> = #rt::MetricSite::new();
        #b.#measure(#value, &__SEMLEX_SITE, &__SEMLEX_SCOPE)
    }}
}

/// `event!(name { key: value, … })`.
pub fn event(input: TokenStream) -> TokenStream {
    let (krate, mut it) = split_krate(input);
    let name = match parse_dotted(&mut it) {
        Ok(n) => n,
        Err((span, msg)) => return error(span, &msg),
    };
    let entries = match braced_entries(it.next()) {
        Ok(e) => e,
        Err((span, msg)) => return error(span, &msg),
    };
    if let Some(t) = it.next() {
        return error(t.span(), "unexpected input after the event's attributes");
    }
    let at = name.span();
    let front = signal_front(&krate, "events", &name, &entries);
    let (rt, b) = (Ident::new("__semlex_rt", at), Ident::new("__semlex_b", at));
    let emit = Ident::new("emit", at);
    let group = group_path(&name);
    // The `tracing` fallback (no logger provider): one field per registry key
    // known here, named after it, with its value read back from the event's
    // attributes. Template keys are only known at run time, and
    // `unregistered(…)` values are untyped, so neither is in the fallback; nor
    // are kinds `tracing` can't carry (`FieldNone` gives `None`).
    let fields = entries.iter().filter_map(|e| {
        let key = match as_extra(e) {
            Some(Ok(inner)) if inner.suffix.is_none() => inner.key,
            Some(Ok(_)) => return None,
            Some(Err(_)) => return None,
            None if as_unregistered(e).is_some() || e.suffix.is_some() => return None,
            None => Dotted(e.key.0.clone()),
        };
        let field = key.text();
        let path = key.attr_path();
        Some(quote_spanned!(at=> #field = (&&#rt::FieldProbe(&#path, __semlex_attrs)).__semlex_field()))
    });
    quote_spanned! {at=> {
        #front
        static __SEMLEX_SITE: #rt::EventSite = #rt::EventSite::new();
        #b.#emit(&__SEMLEX_SITE, &__SEMLEX_SCOPE, |__semlex_attrs| {
            #[allow(unused_imports)]
            use #rt::{FieldKnown as _, FieldNone as _};
            #rt::tracing::event!(
                name: #group::EVENT_NAME,
                #rt::tracing::Level::INFO,
                { #(#fields),* }
            );
        })
    }}
}

/// `span!(type { key: value, … } [, parent: cx] [, links: l] [, start_time: t] [, name: n])`.
pub fn span(input: TokenStream) -> TokenStream {
    let (krate, mut it) = split_krate(input);
    let name = match parse_dotted(&mut it) {
        Ok(n) => n,
        Err((span, msg)) => return error(span, &msg),
    };
    let braces = match it.peek() {
        Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Brace => it.next(),
        _ => None,
    };
    let entries = match braced_entries(braces) {
        Ok(e) => e,
        Err((span, msg)) => return error(span, &msg),
    };
    let at = name.span();
    let rt = Ident::new("__semlex_rt", at);
    let mut options = quote_spanned!(at=> #rt::StartOptions::new());
    let mut span_name = quote_spanned!(at=> #rt::NoName);
    while it.peek().is_some() {
        if !is_punct(it.peek(), ',') {
            return error(
                it.next().expect("peeked").span(),
                "expected `,` and an option: `parent`, `links`, `start_time` or `name`",
            );
        }
        it.next();
        if it.peek().is_none() {
            break;
        }
        let key = match it.next() {
            Some(TokenTree::Ident(i)) => i,
            Some(t) => {
                return error(
                    t.span(),
                    "expected an option: `parent`, `links`, `start_time` or `name`",
                );
            }
            None => break,
        };
        if !is_punct(it.peek(), ':') {
            return error(key.span(), &format!("expected `:` after `{key}`"));
        }
        it.next();
        let value: TokenStream = {
            let mut v = Vec::new();
            while let Some(t) = it.peek() {
                if is_punct(Some(t), ',') {
                    break;
                }
                v.push(it.next().expect("peeked"));
            }
            v.into_iter().collect()
        };
        match key.to_string().as_str() {
            "parent" => options = quote_spanned!(key.span()=> #options.parent(&(#value))),
            "links" => options = quote_spanned!(key.span()=> #options.links(#value)),
            "start_time" => options = quote_spanned!(key.span()=> #options.start_time(#value)),
            "name" => span_name = quote_spanned!(key.span()=> #rt::named(#value)),
            "kind" => {
                return error(
                    key.span(),
                    "a span's kind comes from the registry; fix it there",
                );
            }
            other => {
                return error(
                    key.span(),
                    &format!(
                        "unknown `span!` option `{other}`; expected `parent`, `links`, `start_time` or `name`"
                    ),
                );
            }
        }
    }
    let front = signal_front(&krate, "spans", &name, &entries);
    let b = Ident::new("__semlex_b", at);
    let start = Ident::new("start", at);
    quote_spanned! {at=> {
        #front
        static __SEMLEX_SITE: #rt::SpanSite = #rt::SpanSite::new();
        #b.#start(&__SEMLEX_SITE, &__SEMLEX_SCOPE, #options, #span_name)
    }}
}

/// `record!(span, key: value, …)` → the span in its new state, or
/// `record!(current, key: value, …)` on the current span (registry-wide checks).
pub fn record(input: TokenStream) -> TokenStream {
    let (krate, mut it) = split_krate(input);
    let target: Vec<TokenTree> = take_until_comma(&mut it);
    if target.is_empty() {
        return error(Span::call_site(), "expected `record!(span, key: value, …)`");
    }
    let entries = match parse_entries(&mut it) {
        Ok(e) => e,
        Err((span, msg)) => return error(span, &msg),
    };
    if entries.is_empty() {
        return error(
            target[0].span(),
            "expected at least one `key: value` after the span",
        );
    }
    let at = entries[0].key.span();
    let aliases = aliases(&krate, at);
    if let [TokenTree::Ident(i)] = target.as_slice()
        && i == "current"
    {
        let rt = Ident::new("__semlex_rt", at);
        let kvs = entries.iter().map(entry_kv);
        return quote! {{
            #aliases
            #rt::record_current([#(#kvs),*])
        }};
    }
    // `record!(&span, …)`: in place, through a shared reference.
    let in_place = matches!(target.first(), Some(TokenTree::Punct(p)) if p.as_char() == '&');
    let target: TokenStream = target.into_iter().collect();
    let b = Ident::new("__semlex_b", at);
    if in_place {
        let sets: TokenStream = entries
            .iter()
            .map(|e| entry_in_place(e, "__semlex_b"))
            .collect();
        let rt = Ident::new("__semlex_rt", at);
        return quote! {{
            #aliases
            #[allow(unused_imports)]
            use #rt::SpanInPlace as _;
            let #b = #target;
            #sets
        }};
    }
    let withs: TokenStream = entries
        .iter()
        .map(|e| entry_with(e, "__semlex_b", false, None))
        .collect();
    quote! {{
        #aliases
        let #b = #target;
        #withs
        #b
    }}
}

/// `end!(span)`, `end!(span, error: &e, error.type: class)` or
/// `end!(span, result: &r, error.type: |e| class)`: ends a span following the
/// error conventions, with the checked `end()` on every path.
///
/// On an error: `error.type` is recorded on the span (membership-checked),
/// the `exception` event gets the error and its source chain, the status is
/// `Error` with the error's message, then `end()`, which needs every
/// required attribute.
pub fn end(input: TokenStream) -> TokenStream {
    let (krate, mut it) = split_krate(input);
    let target: Vec<TokenTree> = take_until_comma(&mut it);
    let Some(first) = target.first() else {
        return error(
            Span::call_site(),
            "expected `end!(span)` or `end!(span, error: &e, error.type: class)`",
        );
    };
    let at = first.span();
    let target: TokenStream = target.into_iter().collect();
    let s = Ident::new("__semlex_s", at);
    let end = Ident::new("end", at);
    let entries = match parse_entries(&mut it) {
        Ok(e) => e,
        Err((span, msg)) => return error(span, &msg),
    };
    if entries.is_empty() {
        return quote_spanned! {at=> { let #s = #target; #s.#end() } };
    }
    let usage = "expected `error: &error, error.type: class` or `result: &result, error.type: |error| class`";
    let named = |k: &str| entries.iter().find(|e| e.key.text() == k);
    let (Some(class), others) = (
        named("error.type"),
        entries
            .iter()
            .filter(|e| !["error", "result", "error.type"].contains(&e.key.text().as_str()))
            .collect::<Vec<_>>(),
    ) else {
        return error(entries[0].key.span(), usage);
    };
    if let Some(e) = others.first() {
        return error(e.key.span(), usage);
    }
    if let Some(at) = class.optional {
        return error(at, "`error.type` needs a value here: drop the `?`");
    }
    let aliases = aliases(&krate, at);
    let rt = Ident::new("__semlex_rt", at);
    let err = Ident::new("__semlex_err", at);
    // `error.type` on the span, with its own `with` (membership-checked),
    // then the exception record and status, then the checked `end()`.
    let fail = |value: ValueExpr| {
        let entry = Entry {
            reviewed: false,
            key: Dotted(class.key.0.clone()),
            suffix: None,
            optional: None,
            value,
        };
        let Translated {
            at: kat,
            checks,
            template,
            path,
            value,
        } = translate(&entry);
        if template.is_some() {
            return error(kat, usage);
        }
        let with = Ident::new("with", kat);
        quote_spanned! {at=>
            #checks
            let #s = #s.#with(#path, #value);
            #rt::fail(&#s, #err, __semlex_ty);
            #s.#end()
        }
    };
    // The error's value, through any handle that derefs to `dyn Error`.
    // Bound by its own `let` first, so a temporary (`error: &Failed(e)`)
    // lives to the end of the block, not just the probe's statement.
    let probe = |e: TokenStream| {
        quote_spanned! {at=>
            #[allow(unused_imports)]
            use #rt::{ErrConcrete as _, ErrDyn as _, ErrSendSync as _};
            let __semlex_e0 = #e;
            let (#err, __semlex_ty) = (&&&#rt::ErrProbe(__semlex_e0)).__semlex_error();
        }
    };
    match (named("error"), named("result")) {
        (Some(e), None) => {
            let ValueExpr::Expr(error_expr) = &e.value else {
                return error(e.key.span(), "`error:` takes an expression: `error: &e`");
            };
            let probe = probe(error_expr.clone());
            let fail = fail(class.value.clone());
            quote_spanned! {at=> {
                #aliases
                let #s = #target;
                #probe
                #fail
            }}
        }
        (None, Some(r)) => {
            let ValueExpr::Expr(result_expr) = &r.value else {
                return error(
                    r.key.span(),
                    "`result:` takes an expression: `result: &result`",
                );
            };
            let ValueExpr::Expr(classify) = &class.value else {
                return error(
                    class.key.span(),
                    "with `result:`, `error.type` takes a function of the error: `|e| e.class()`",
                );
            };
            let classified =
                ValueExpr::Expr(quote_spanned!(at=> #rt::classify(__semlex_e, #classify)));
            let probe = probe(quote_spanned!(at=> __semlex_e));
            let fail = fail(classified);
            // Matched through a reference: `result:` takes `&r` or `r`.
            quote_spanned! {at=> {
                #aliases
                let #s = #target;
                match &(#result_expr) {
                    ::std::result::Result::Ok(_) => #s.#end(),
                    ::std::result::Result::Err(__semlex_e) => {
                        #probe
                        #fail
                    }
                }
            }}
        }
        _ => error(at, usage),
    }
}

/// `entity!(type { key: value, … })` → `semlex::Entity`.
pub fn entity(input: TokenStream) -> TokenStream {
    let (krate, mut it) = split_krate(input);
    let name = match parse_dotted(&mut it) {
        Ok(n) => n,
        Err((span, msg)) => return error(span, &msg),
    };
    let entries = match braced_entries(it.next()) {
        Ok(e) => e,
        Err((span, msg)) => return error(span, &msg),
    };
    if let Some(t) = it.next() {
        return error(t.span(), "unexpected input after the entity's attributes");
    }
    let at = name.span();
    if let Some(err) = reject_unregistered(&entries) {
        return err;
    }
    let front = signal_front(&krate, "entities", &name, &entries);
    let b = Ident::new("__semlex_b", at);
    let build = Ident::new("build", at);
    quote_spanned! {at=> {
        #front
        #b.#build()
    }}
}

/// `info!` / `warn!` / … : `log!(krate; level; entries…, "message", args…)`.
///
/// Entries are `key: value` or `group id { key: value, … }`; the message and
/// its arguments follow, as in `tracing`. Each value is checked like any
/// attribute value and becomes a `tracing` field named by the attribute's key.
pub fn log(input: TokenStream) -> TokenStream {
    let (krate, mut it) = split_krate(input);
    let level = match it.next() {
        Some(TokenTree::Ident(l))
            if ["info", "warn", "error", "debug", "trace"].contains(&l.to_string().as_str()) =>
        {
            l
        }
        Some(t) => return error(t.span(), "expected a log level"),
        None => return error(Span::call_site(), "expected a log level"),
    };
    if !is_punct(it.next().as_ref(), ';') {
        return error(level.span(), "expected `;` after the log level");
    }

    // Entries up to the message (the first string literal at an entry's start).
    let mut plain: Vec<Entry> = Vec::new();
    let mut groups: Vec<(Dotted, Vec<Entry>)> = Vec::new();
    let mut message = TokenStream::new();
    while let Some(t) = it.peek() {
        match t {
            TokenTree::Literal(l) if l.to_string().starts_with('"') => {
                message = it.by_ref().collect();
                break;
            }
            TokenTree::Ident(i) if i == "group" => {
                it.next();
                let id = match parse_dotted(&mut it) {
                    Ok(d) => d,
                    Err((span, msg)) => return error(span, &msg),
                };
                let entries = match braced_entries(it.next()) {
                    Ok(e) => e,
                    Err((span, msg)) => return error(span, &msg),
                };
                groups.push((id, entries));
                if is_punct(it.peek(), ',') {
                    it.next();
                }
            }
            _ => {
                // One `key: value` entry.
                let mut one: Vec<TokenTree> = Vec::new();
                while let Some(t) = it.peek() {
                    if is_punct(Some(t), ',') {
                        it.next();
                        break;
                    }
                    one.push(it.next().expect("peeked"));
                }
                match parse_entries(&mut tokens(one.into_iter().collect())) {
                    Ok(mut e) => plain.append(&mut e),
                    Err((span, msg)) => return error(span, &msg),
                }
            }
        }
    }
    let all: Vec<&Entry> = plain
        .iter()
        .chain(groups.iter().flat_map(|(_, e)| e.iter()))
        .collect();
    for e in &all {
        if let Some(at) = e.optional {
            return optional_not_here(at);
        }
        match as_extra(e) {
            Some(Ok(inner)) => return extra_not_needed(e, &inner),
            Some(Err(err)) => return err,
            None => {}
        }
    }
    if let Some(e) = all
        .iter()
        .find(|e| e.suffix.is_some() && as_unregistered(e).is_none())
    {
        return error(
            e.key.span(),
            "template attributes can't be logged through `tracing` (field names are fixed at compile time); emit an event with `event!` instead",
        );
    }

    let at = all.first().map_or(level.span(), |e| e.key.span());
    let aliases = aliases(&krate, at);
    let rt = Ident::new("__semlex_rt", at);

    // `group id { … }`: membership and requirements, without the values.
    let group_checks = groups.iter().map(|(id, entries)| {
        let gat = id.span();
        let root = Ident::new("__semlex_groups", gat);
        let segs = id.path_segments();
        let b = Ident::new("__semlex_g", gat);
        let check = Ident::new("check", gat);
        let withs = entries
            .iter()
            .filter(|e| as_unregistered(e).is_none())
            .map(|e| {
                let eat = e.key.span();
                let path = e.key.attr_path();
                let with = Ident::new("check_with", eat);
                let b = Ident::new("__semlex_g", eat);
                quote_spanned!(eat=> let #b = #b.#with(#path);)
            });
        quote_spanned! {gat=>
            #[allow(unused_imports)]
            use #krate::groups as #root;
            let #b = #root #(::#segs)* ::builder();
            #(#withs)*
            #b.#check();
        }
    });

    let mut lets = TokenStream::new();
    let mut fields = Vec::new();
    for (i, e) in all.iter().enumerate() {
        match as_unregistered(e) {
            Some(Ok(Unregistered {
                at,
                key,
                markers,
                value,
            })) => {
                let v = Ident::new(&format!("__semlex_v{i}"), at);
                let rt = Ident::new("__semlex_rt", e.key.span());
                // Its own block: each use defines its own marker function.
                lets.extend(
                    quote_spanned! {at=> let #v = { #markers #rt::unregistered_field(#value) }; },
                );
                fields.push(quote_spanned! {at=> { #key } = #v });
                continue;
            }
            Some(Err(err)) => return err,
            None => {}
        }
        let Translated {
            at,
            checks,
            path,
            value,
            ..
        } = translate(e);
        let v = Ident::new(&format!("__semlex_v{i}"), at);
        let rt = Ident::new("__semlex_rt", at);
        lets.extend(quote_spanned! {at=> #checks let #v = #rt::log_field(#path, #value); });
        fields.push(quote_spanned! {at=> { #rt::key_of(&#path) } = #v });
    }
    // `tracing::event!` with an empty name, the level, the fields, then the
    // message and its arguments. Without `name`, `tracing` names the
    // callsite `event <file>:<line>`, and `opentelemetry-appender-tracing`
    // copies that into the log record's `event_name`, so every log would look
    // like an event the registry doesn't define. Events come from `event!`.
    let lvl = Ident::new(&level.to_string().to_uppercase(), level.span());
    let message = if message.is_empty() {
        quote!()
    } else {
        quote!(, #message)
    };
    quote! {{
        #aliases
        #(#group_checks)*
        #lets
        #rt::tracing::event!(name: "", #rt::tracing::Level::#lvl, { #(#fields),* } #message)
    }}
}

/// `assert_span!` / `assert_metric!` / `assert_event!`:
/// `assert_signal!(krate; kind; telemetry, name { key: value, … })`.
///
/// The expected attributes are checked like `attrs!` entries; the assertion
/// itself is a method on `semlex::testing::Telemetry`.
pub fn assert_signal(input: TokenStream) -> TokenStream {
    let (krate, mut it) = split_krate(input);
    let kind = match it.next() {
        Some(TokenTree::Ident(k))
            if ["span", "metric", "event"].contains(&k.to_string().as_str()) =>
        {
            k
        }
        Some(t) => return error(t.span(), "expected a signal kind"),
        None => return error(Span::call_site(), "expected a signal kind"),
    };
    if !is_punct(it.next().as_ref(), ';') {
        return error(kind.span(), "expected `;` after the signal kind");
    }
    let telemetry: TokenStream = take_until_comma(&mut it).into_iter().collect();
    if telemetry.is_empty() {
        return error(
            kind.span(),
            "expected the `Telemetry` first: `assert_span!(t, http.server.request { … })`",
        );
    }
    let name = match parse_dotted(&mut it) {
        Ok(n) => n,
        Err((span, msg)) => return error(span, &msg),
    };
    let entries = match braced_entries(it.next()) {
        Ok(e) => e,
        Err((span, msg)) => return error(span, &msg),
    };
    let at = name.span();
    let aliases = aliases(&krate, at);
    let root = Ident::new(
        match kind.to_string().as_str() {
            "span" => "spans",
            "metric" => "metrics",
            _ => "events",
        },
        at,
    );
    let root_alias = Ident::new("__semlex_root", at);
    let group = group_path(&name);
    let kvs = entries.iter().map(entry_kv);
    let expected = Ident::new("__semlex_expected", at);
    let call = match kind.to_string().as_str() {
        "span" => {
            let m = Ident::new("assert_span", at);
            quote_spanned!(at=> (#telemetry).#m(&#group::TYPE, &#expected))
        }
        "metric" => {
            let m = Ident::new("assert_metric", at);
            quote_spanned!(at=> (#telemetry).#m(#group::NAME, #group::UNIT, &#expected))
        }
        _ => {
            let m = Ident::new("assert_event", at);
            quote_spanned!(at=> (#telemetry).#m(#group::EVENT_NAME, &#expected))
        }
    };
    quote_spanned! {at=> {
        #aliases
        #[allow(unused_imports)]
        use #krate::#root as #root_alias;
        let #expected = [#(#kvs),*];
        #call
    }}
}
