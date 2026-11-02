//! `registry!("path", { options })` input.

use proc_macro2::Span;
use semlex_schema::model::Stability;
use syn::ext::IdentExt;
use syn::parse::{Parse, ParseStream};
use syn::{Ident, LitStr, Path, Token, Type, braced, bracketed};

/// Parsed `registry!()` input.
pub struct RegistryInput {
    /// The registry path, relative to the crate's `Cargo.toml` directory.
    pub path: LitStr,
    /// Options.
    pub options: Options,
}

/// `registry!()` options.
#[derive(Clone, Default)]
pub struct Options {
    /// Lowest stability level to emit; `None` emits everything.
    pub min_stability: Option<Stability>,
    /// `groups: ["http.*", …]`: emit only signals and groups whose id matches
    /// one of these patterns (`*` matches any run of characters). Empty: all.
    pub groups: Vec<String>,
    /// `conversions: { key: Type, … }`: `From` impls for external types into
    /// an enum attribute's type.
    pub conversions: Vec<Conversion>,
    /// `annotations: { namespace.key: handler::path | ignore, … }`.
    pub annotations: Vec<Declaration>,
    /// `scope_prefix: "acme."`: prefix for call sites' instrumentation
    /// scope names (the calling crate's name follows it).
    pub scope_prefix: String,
    /// `allow_unstable: true`: non-stable items are usable without the
    /// `semlex-unstable` feature (a crate that owns its registry, e.g. a
    /// library instrumenting itself).
    pub allow_unstable: bool,
    /// `visibility: crate`: the generated modules and macros are visible in
    /// this crate only, instead of exported (a library instrumenting itself).
    pub crate_only: bool,
}

/// One `conversions` entry.
#[derive(Clone)]
pub struct Conversion {
    /// The enum attribute's dotted key.
    pub key: String,
    /// Where the key was written, for errors.
    pub span: Span,
    /// The external type.
    pub ty: Type,
}

/// One `annotations` entry: how an annotation namespace (or a key within it)
/// is handled.
#[derive(Clone)]
pub struct Declaration {
    /// Dotted path into the annotation map, e.g. `demo.case`.
    pub path: Vec<String>,
    /// Where the path was written, for errors.
    pub span: Span,
    /// What to do with it.
    pub handling: Handling,
}

/// How a declared annotation is handled.
#[derive(Clone)]
pub enum Handling {
    /// A `#[semlex::handler]` module.
    Handler(Path),
    /// Used by other tools only; `registry!()` ignores it.
    Ignore,
}

/// `a.b.c` (keywords allowed), returning the segments and the first one's span.
fn dotted(input: ParseStream<'_>) -> syn::Result<(Vec<String>, Span)> {
    let first: Ident = input.call(Ident::parse_any)?;
    let span = first.span();
    let mut segs = vec![first.to_string()];
    while input.peek(Token![.]) {
        input.parse::<Token![.]>()?;
        segs.push(input.call(Ident::parse_any)?.to_string());
    }
    Ok((segs, span))
}

/// `{ entry, entry, … }`, each entry parsed by `f`.
fn braced_list(
    input: ParseStream<'_>,
    mut f: impl FnMut(ParseStream<'_>) -> syn::Result<()>,
) -> syn::Result<()> {
    let body;
    braced!(body in input);
    while !body.is_empty() {
        f(&body)?;
        if body.parse::<Option<Token![,]>>()?.is_none() {
            break;
        }
    }
    if !body.is_empty() {
        return Err(body.error("expected `,` between entries"));
    }
    Ok(())
}

fn stability(v: &Ident) -> syn::Result<Option<Stability>> {
    Ok(match v.to_string().as_str() {
        "all" => None,
        "stable_only" | "stable" => Some(Stability::Stable),
        "release_candidate" => Some(Stability::ReleaseCandidate),
        "beta" => Some(Stability::Beta),
        "alpha" => Some(Stability::Alpha),
        "development" => Some(Stability::Development),
        _ => {
            return Err(syn::Error::new(
                v.span(),
                "expected `all`, `stable_only`, or a minimum level: `development`, `alpha`, `beta`, `release_candidate`",
            ));
        }
    })
}

impl Parse for RegistryInput {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        let path: LitStr = input.parse().map_err(|e| {
            syn::Error::new(
                e.span(),
                "expected the path of a resolved registry, e.g. `registry!(\"resolved.yaml\")`",
            )
        })?;
        let mut options = Options::default();
        if input.parse::<Option<Token![,]>>()?.is_some() && !input.is_empty() {
            braced_list(input, |body| {
                let key: Ident = body.parse()?;
                body.parse::<Token![:]>()?;
                match key.to_string().as_str() {
                    "stability" => options.min_stability = stability(&body.parse()?)?,
                    "scope_prefix" => options.scope_prefix = body.parse::<LitStr>()?.value(),
                    "allow_unstable" => {
                        options.allow_unstable = body.parse::<syn::LitBool>()?.value
                    }
                    "visibility" => {
                        let v: Ident = body.call(Ident::parse_any)?;
                        options.crate_only = match v.to_string().as_str() {
                            "crate" => true,
                            "public" => false,
                            _ => {
                                return Err(syn::Error::new(
                                    v.span(),
                                    "expected `public` (the default: modules and macros are exported) or `crate` (visible in this crate only)",
                                ));
                            }
                        };
                    }
                    "groups" => {
                        let list;
                        bracketed!(list in body);
                        let pats = list.parse_terminated(<LitStr as Parse>::parse, Token![,])?;
                        options.groups = pats.iter().map(LitStr::value).collect();
                    }
                    "conversions" => braced_list(body, |b| {
                        let (key, span) = dotted(b)?;
                        b.parse::<Token![:]>()?;
                        let ty: Type = b.parse()?;
                        options.conversions.push(Conversion {
                            key: key.join("."),
                            span,
                            ty,
                        });
                        Ok(())
                    })?,
                    "annotations" => braced_list(body, |b| {
                        let (path, span) = dotted(b)?;
                        b.parse::<Token![:]>()?;
                        let target: Path = b.parse()?;
                        let handling = if target.is_ident("ignore") {
                            Handling::Ignore
                        } else {
                            Handling::Handler(target)
                        };
                        options.annotations.push(Declaration {
                            path,
                            span,
                            handling,
                        });
                        Ok(())
                    })?,
                    other => {
                        return Err(syn::Error::new(
                            key.span(),
                            format!(
                                "unknown `registry!` option `{other}`; expected `stability`, `scope_prefix`, `allow_unstable`, `visibility`, `groups`, `conversions` or `annotations`"
                            ),
                        ));
                    }
                }
                Ok(())
            })?;
            input.parse::<Option<Token![,]>>()?;
        }
        if !input.is_empty() {
            return Err(input.error("unexpected input after the registry options"));
        }
        Ok(RegistryInput { path, options })
    }
}

/// Whether `id` matches a `groups` pattern (`*` matches any run of characters).
pub fn matches(pattern: &str, id: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == id;
    }
    let (first, last) = (parts[0], parts[parts.len() - 1]);
    if !id.starts_with(first)
        || !id[first.len()..].ends_with(last)
        || id.len() < first.len() + last.len()
    {
        return false;
    }
    let mut rest = &id[first.len()..id.len() - last.len()];
    for p in &parts[1..parts.len() - 1] {
        match rest.find(p) {
            Some(i) => rest = &rest[i + p.len()..],
            None => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn group_patterns() {
        assert!(matches("http.*", "http.server.request"));
        assert!(!matches("http.*", "db.client"));
        assert!(matches("*.client", "db.client"));
        assert!(matches("db.client", "db.client"));
        assert!(!matches("db.client", "db.client.extra"));
        assert!(matches("*", "anything"));
        assert!(matches("a*b*c", "a-x-b-y-c"));
        assert!(!matches("a*b*c", "a-x-c"));
    }
}
