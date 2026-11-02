//! Expansion snapshots: exactly what the macros emit, pretty-printed, so a
//! reviewer sees the effect of a change on generated code.

use quote::quote;
use std::path::PathBuf;

fn pretty(tokens: proc_macro2::TokenStream) -> String {
    let file: syn::File = syn::parse2(tokens).expect("expansion parses as a file");
    prettyplease::unparse(&file)
}

fn data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data")
}

#[test]
fn registry_expansion() {
    let out = semlex_expand::registry(quote!("small.yaml"), &data(), "fixture-semconv");
    // The tracked path is machine-specific; normalize it.
    let text = pretty(out).replace(&data().display().to_string(), "$DATA");
    insta::assert_snapshot!(text);
}

#[test]
fn registry_expansion_stable_only() {
    let out = semlex_expand::registry(
        quote!("small.yaml", { stability: stable_only }),
        &data(),
        "fixture-semconv",
    );
    let text = pretty(out);
    // Non-stable attributes and enum members aren't emitted at all.
    assert!(!text.contains("rpc_grpc_status_code"), "{text}");
    assert!(!text.contains("Query"), "{text}");
    assert!(text.contains("http_request_method"), "{text}");
}

#[test]
fn attrs_expansion() {
    let out = semlex_expand::attrs(quote!(
        ::org;
        http.request.method: "GET",
        http.route: route,
        http.request.method: custom("BREW"),
        http.request.header("accept"): vec!["*/*"],
    ));
    insta::assert_snapshot!(pretty(quote!(fn f() { let _ = #out; })));
}

#[test]
fn composed_hooks_expansion() {
    let out = semlex_expand::registry(
        quote!("annotated.yaml", { annotations: { org.case: crate::annotations::case } }),
        &data(),
        "org-semconv",
    );
    let text = pretty(out).replace(&data().display().to_string(), "$DATA");
    // Only the parts annotations change: markers and the metric group.
    let start = text.find("pub mod attr_types").expect("attr_types");
    let end = text.find("/// Enum attribute value types.").expect("enums");
    let metric = text.find("pub mod requests").expect("metric");
    let metric_end = text[metric..]
        .find("\n}\n")
        .map_or(text.len(), |i| metric + i + 3);
    insta::assert_snapshot!(format!(
        "{}\n// …\n{}",
        &text[start..end],
        &text[metric..metric_end]
    ));
}

#[test]
fn metric_expansion() {
    let out = semlex_expand::metric(quote!(
        ::org;
        http.server.duration: elapsed, {
            http.request.method: "GET",
            http.route: route,
        }
    ));
    insta::assert_snapshot!(pretty(quote!(fn f() { #out })));
}

#[test]
fn event_expansion() {
    let out = semlex_expand::event(quote!(::org; session.start { session.id: id }));
    insta::assert_snapshot!(pretty(quote!(fn f() { #out })));
}

#[test]
fn span_expansion() {
    let out = semlex_expand::span(quote!(
        ::org;
        http.server { http.request.method: "GET" }, parent: &cx, name: "custom"
    ));
    insta::assert_snapshot!(pretty(quote!(fn f() { let _ = #out; })));
}

#[test]
fn record_expansion() {
    let out = semlex_expand::record(quote!(::org; span, http.response.status_code: 200));
    let current = semlex_expand::record(quote!(::org; current, http.route: route));
    insta::assert_snapshot!(pretty(quote!(fn f() { let span = #out; #current; })));
}

#[test]
fn log_expansion() {
    let out = semlex_expand::log(quote!(
        ::org; warn;
        http.route: route,
        group exception { exception.type: "E", exception.message: msg },
        "upstream timed out after {}ms", 250
    ));
    insta::assert_snapshot!(pretty(quote!(fn f() { #out; })));
}

#[test]
fn unregistered_expansion() {
    let out = semlex_expand::attrs(quote!(::org; unregistered("retry.attempt"): n));
    insta::assert_snapshot!(pretty(quote!(fn f() { let _ = #out; })));
}

/// A custom macro crate's hook: per-metric SLO constants from a structured
/// annotation.
struct Slo;

impl semlex_expand::AnnotationHook for Slo {
    fn key(&self) -> &str {
        "org.slo"
    }

    fn on_metric(
        &self,
        metric: &semlex_expand::schema::Group,
        value: &semlex_expand::Value,
        out: &mut semlex_expand::Emit<'_>,
    ) {
        let Some(objective) = value.get("objective").and_then(|v| v.as_f64()) else {
            return out.error("expected `objective: <0..1>`");
        };
        let id = &metric.id;
        let module = out.signal_path(metric).expect("emitted");
        let route = out.attribute_path("http.route").expect("emitted");
        out.items(
            &["slo", "http"],
            quote! {
                pub const OBJECTIVE: f64 = #objective;
                pub const METRIC: &str = #id;
                pub use #module as metric;
                pub use #route as ROUTE;
            },
        );
    }
}

/// Records `org.owner` values; handles attributes only.
struct Owner;

impl semlex_expand::AnnotationHook for Owner {
    fn key(&self) -> &str {
        "org.owner"
    }

    fn on_attribute(
        &self,
        attribute: &semlex_expand::schema::AttributeDef,
        value: &semlex_expand::Value,
        out: &mut semlex_expand::Emit<'_>,
    ) {
        let (key, owner) = (&attribute.key, value.as_str().unwrap_or_default());
        out.items(
            &["owners"],
            quote! { pub const OWNERS: &[(&str, &str)] = &[(#key, #owner)]; },
        );
    }
}

#[test]
fn hook_expansion() {
    let out = semlex_expand::registry_with_hooks(
        quote!("hooked.yaml"),
        &data(),
        "org-semconv",
        &[&Slo, &Owner],
    );
    let text = pretty(out);
    let start = text
        .find("/// Items from annotation hooks under `owners`.")
        .expect("hook items");
    let end = text.find("/// `attrs![").expect("forwarders");
    insta::assert_snapshot!(&text[start..end]);
}

#[test]
fn hook_errors() {
    let run = |hooks: &[&dyn semlex_expand::AnnotationHook], opts| {
        semlex_expand::registry_with_hooks(
            quote!("hooked.yaml", #opts),
            &data(),
            "org-semconv",
            hooks,
        )
        .to_string()
    };
    // Undeclared without the hook.
    let out = run(&[&Owner], quote!({}));
    assert!(
        out.contains(
            "annotation `org.slo.objective` on metric `http.server.duration` isn't declared"
        ),
        "{out}"
    );
    // A hook that doesn't handle metrics.
    struct Attrs;
    impl semlex_expand::AnnotationHook for Attrs {
        fn key(&self) -> &str {
            "org.slo"
        }
    }
    let out = run(&[&Attrs, &Owner], quote!({}));
    assert!(
        out.contains(
            "annotation `org.slo` on metric `http.server.duration`: its hook doesn't handle metrics"
        ),
        "{out}"
    );
    // Declared twice.
    let out = run(
        &[&Slo, &Owner],
        quote!({ annotations: { org.slo: ignore } }),
    );
    assert!(
        out.contains("annotation `org.slo` is handled by this macro's hook"),
        "{out}"
    );
    let out = run(&[&Slo, &Slo, &Owner], quote!({}));
    assert!(
        out.contains("two annotation hooks handle `org.slo`"),
        "{out}"
    );
}

#[test]
fn crate_visibility_keeps_modules_and_macros_private() {
    let public = semlex_expand::registry(quote!("small.yaml"), &data(), "org-semconv").to_string();
    assert!(public.contains("macro_export"), "exported by default");
    assert!(public.contains("pub mod spans"), "{public}");

    let private = semlex_expand::registry(
        quote!("small.yaml", { visibility: crate, allow_unstable: true }),
        &data(),
        "org-semconv",
    )
    .to_string();
    assert!(!private.contains("macro_export"), "no exported macros");
    assert!(
        private.contains("pub (crate) use super :: __semlex_span as span"),
        "{private}"
    );
    assert!(
        private.contains("pub (crate) use self :: __semlex_macros :: *"),
        "a glob, which the crate's own names shadow: {private}"
    );
    assert!(private.contains("pub (crate) mod spans"), "{private}");
    assert!(!private.contains("pub mod spans"), "{private}");
    assert!(
        private.contains("const ALLOW_UNSTABLE : bool = true"),
        "{private}"
    );
}
