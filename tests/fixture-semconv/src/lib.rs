//! Example registry crate: the one place the registry is read.

semlex::registry!("../../fixtures/resolved/resolved.yaml", {
    // Call sites' instrumentation scopes are `demo.<crate name>`.
    scope_prefix: "demo.",
    annotations: {
        // `demo: { case: upper }` on attributes: upper-case values only.
        demo.case: crate::annotations::case,
        // Structured SLOs, a shape for annotation hooks (examples/advanced).
        demo.slo: ignore,
    },
    conversions: {
        http.request.method: ::http::Method,
    },
});

/// Annotation handlers for this registry's own annotations.
pub mod annotations {
    /// `demo.case: upper`: values must be upper case.
    #[semlex::handler]
    pub mod case {
        use semlex::Value;

        /// Literals must already be upper case.
        pub const fn check_literal(value: &str, arg: &str) {
            if semlex::const_fns::str_eq(arg, "upper") {
                let bytes = value.as_bytes();
                let mut i = 0;
                while i < bytes.len() {
                    if bytes[i].is_ascii_lowercase() {
                        panic!("value must be upper case (annotation `demo.case: upper`)");
                    }
                    i += 1;
                }
            }
        }

        /// Runtime values are upper-cased.
        pub fn apply(value: Value, arg: &str) -> Value {
            match value {
                Value::String(s) if arg == "upper" => Value::from(s.as_str().to_uppercase()),
                other => other,
            }
        }
    }
}
