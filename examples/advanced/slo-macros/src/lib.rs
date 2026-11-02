//! Example custom `registry!` macro: `semlex::registry!`
//! plus a hook for a structured annotation handlers can't read.
//!
//! ```yaml
//! metrics:
//!   - name: http.server.request.duration
//!     instrument: histogram
//!     unit: s
//!     annotations:
//!       acme:
//!         slo: { objective: 0.99, latency: 0.5 }   # 99% within 500 ms
//! ```
//!
//! emits, per annotated metric, `crate::slo::<metric path>` with the
//! objective, the latency threshold and a `within` check.

use proc_macro::TokenStream;
use quote::quote;
use semlex_expand::schema::{Group, GroupKind, model::Instrument};
use semlex_expand::{AnnotationHook, Emit, Value};

/// `semlex::registry!`, with `acme.slo` handled.
#[proc_macro]
pub fn registry(input: TokenStream) -> TokenStream {
    semlex_expand::registry_from_env(input.into(), &[&Slo]).into()
}

/// `acme.slo: { objective, latency }` on latency histograms.
pub(crate) struct Slo;

impl AnnotationHook for Slo {
    fn key(&self) -> &str {
        "acme.slo"
    }

    fn on_metric(&self, metric: &Group, value: &Value, out: &mut Emit<'_>) {
        let GroupKind::Metric {
            name,
            instrument,
            unit,
        } = &metric.kind
        else {
            return;
        };
        if *instrument != Instrument::Histogram || unit != "s" {
            return out.error("latency SLOs need a histogram with unit `s`");
        }
        let number = |field: &str| value.get(field).and_then(Value::as_f64);
        let objective = match number("objective") {
            Some(o) if o > 0.0 && o < 1.0 => o,
            _ => return out.error("`objective` must be a number between 0 and 1, e.g. 0.99"),
        };
        let latency_ms = match number("latency") {
            Some(l) if l > 0.0 => (l * 1000.0).round() as u64,
            _ => return out.error("`latency` must be a positive number of seconds, e.g. 0.5"),
        };
        if let Value::Object(m) = value
            && let Some(other) = m.keys().find(|k| *k != "objective" && *k != "latency")
        {
            return out.error(format!(
                "unknown field `{other}`; expected `objective` and `latency`"
            ));
        }
        let Some(builder) = out.signal_path(metric) else {
            return;
        };
        let doc = format!(
            " SLO for `{}`: {}% within {latency_ms} ms.",
            metric.id,
            objective * 100.0
        );
        let path: Vec<&str> = metric.id.split('.').collect();
        let mut module = vec!["slo"];
        module.extend(path);
        out.items(
            &module,
            quote! {
                #[doc = #doc]
                pub const METRIC: &str = #name;
                /// Fraction of measurements that must be within [`LATENCY`].
                pub const OBJECTIVE: f64 = #objective;
                /// The latency threshold.
                pub const LATENCY: ::std::time::Duration = ::std::time::Duration::from_millis(#latency_ms);
                /// Whether a measurement meets the latency threshold.
                pub fn within(elapsed: ::std::time::Duration) -> bool {
                    elapsed <= LATENCY
                }
                /// The metric's generated module.
                pub use #builder as metric;
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::Slo;
    use quote::quote;
    use std::path::PathBuf;

    /// `registry!` over the example's registry, as `advanced` invokes it.
    fn expand() -> String {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let input = quote!("resolved.yaml", { scope_prefix: "acme.", visibility: crate });
        semlex_expand::registry_with_hooks(input, &dir, "advanced", &[&Slo]).to_string()
    }

    #[test]
    fn emits_slo_items() {
        let out = expand();
        let file: syn::File = syn::parse_str(&out).expect("parses");
        let text = prettyplease::unparse(&file);
        let start = text
            .find("/// Items from annotation hooks under `slo`.")
            .expect("slo module");
        let end = text.find("/// `attrs![").expect("forwarders");
        insta::assert_snapshot!(&text[start..end]);
    }
}
