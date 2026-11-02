//! A small shop's checkout, instrumented with semlex.
//!
//! Every attribute key, signal name and enum value below comes from the
//! shop's local registry (`registry/`, with OpenTelemetry semconv as its
//! upstream), and `rustc` checks each one against it. See `README.md` for a
//! map from features to code.
//!
//! `registry/` is packaged into `resolved.yaml` with `just example-registries`,
//! and expanded here. The default `visibility` (`public`) exports the macros,
//! so the integration tests and the compile-error tour (separate crates) can
//! use them; in this file they're used by their bare names, after the
//! invocation.

use semlex::{Context, KeyValue};
use std::time::{Duration, Instant};

semlex::registry!("resolved.yaml", {
    // Call sites' instrumentation scopes are `shop.<crate name>`.
    scope_prefix: "shop.",
    // Our own annotations: `shop.case` is enforced by the handler below.
    annotations: {
        shop.case: crate::annotations::case,
    },
    // `http::Method` values are accepted for `http.request.method`.
    conversions: {
        http.request.method: ::http::Method,
    },
});

/// Handlers for the shop's own registry annotations.
pub mod annotations {
    /// `shop.case: upper`: literals must be upper case, and runtime values
    /// are upper-cased.
    #[semlex::handler]
    pub mod case {
        use semlex::Value;

        /// Rejects lower-case literals at compile time.
        pub const fn check_literal(value: &str, arg: &str) {
            if semlex::const_fns::str_eq(arg, "upper") {
                let bytes = value.as_bytes();
                let mut i = 0;
                while i < bytes.len() {
                    if bytes[i].is_ascii_lowercase() {
                        panic!("value must be upper case (annotation `shop.case: upper`)");
                    }
                    i += 1;
                }
            }
        }

        /// Upper-cases runtime values.
        pub fn apply(value: Value, arg: &str) -> Value {
            match value {
                Value::String(s) if arg == "upper" => Value::from(s.as_str().to_uppercase()),
                other => other,
            }
        }
    }
}

use enums::PaymentMethod;

/// A checkout request, as the web framework hands it over.
pub struct CheckoutRequest {
    /// The HTTP method.
    pub method: http::Method,
    /// The request path, e.g. `/cart/42/checkout`.
    pub path: String,
    /// The store's tenant code, any case.
    pub tenant: String,
    /// The signed-in customer, if any.
    pub customer: Option<Customer>,
    /// The cart's items, as SKUs.
    pub skus: Vec<String>,
    /// How the customer pays.
    pub payment: PaymentMethod,
}

/// A signed-in customer.
pub struct Customer {
    /// Account id (PII).
    pub id: String,
    /// Email address (PII).
    pub email: String,
}

/// Why a checkout failed, with the provider's reason as its source.
#[derive(Debug)]
pub struct PaymentDeclined {
    provider: ProviderError,
}

impl std::fmt::Display for PaymentDeclined {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("payment declined")
    }
}

impl std::error::Error for PaymentDeclined {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.provider)
    }
}

/// The payment provider's reason.
#[derive(Debug)]
pub struct ProviderError(&'static str);

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ProviderError {}

/// The Resource's attributes: this service instance.
///
/// **Entities**: identity attributes are required; pass the same attributes
/// to `semlex::Config::with_resource` so entity associations are checked.
pub fn resource_attributes(instance_id: &str) -> Vec<KeyValue> {
    entity!(service {
        service.name: "shop-checkout",
        service.instance.id: instance_id,
        service.version: env!("CARGO_PKG_VERSION"),
    })
    .into_iter()
    .collect()
}

/// `POST /cart/{id}/checkout`: authorizes the payment, stores the order and
/// returns the order id, or the HTTP status for the failure.
pub fn checkout(req: &CheckoutRequest) -> Result<String, u16> {
    let started = Instant::now();

    // **Spans**: sampling-relevant attributes (`http.request.method`,
    // `url.scheme`) must be set here, where samplers can see them.
    // **Conversions**: an `http::Method` is accepted for the method.
    // **Opt-in template attributes**: a header, keyed by its name.
    let span = span!(http.server.request {
        http.request.method: req.method.clone(),
        url.scheme: "https",
        http.request.header("content-type"): vec!["application/json".to_string()],
    });

    // **Required attributes** can come later, any time before `end()`.
    // **Annotation handlers**: `shop.tenant.code` is upper-cased at run time
    // (literals must already be upper case: `shop.case: upper`).
    let span = record!(span,
        url.path: &req.path,
        shop.tenant.code: &req.tenant,
    );

    // **PII**: `sensitivity: pii` attributes only take `redacted(…)` or
    // `hashed(…)`; a bare value doesn't compile.
    // **Optional values**: `key?: option` sets the attribute only if `Some`
    // (wrappers apply to the value inside).
    let customer_id = req.customer.as_ref().map(|c| c.id.clone());
    let span = record!(span, shop.customer.id?: redacted(customer_id));

    // **Span names** come from the registry's templates and are re-evaluated
    // at `end()`: `POST` becomes `POST /cart/{id}/checkout`.
    // **`record!(&span, …)`** records without consuming the span (in
    // closures, as values become known); only required attributes that
    // aren't set yet need `let span = record!(span, …)`.
    record!(&span, http.route: "/cart/{id}/checkout");

    // **Metrics**: an up-down counter takes signed values.
    metric!(shop.checkout.active: 1, { shop.tenant.code: &req.tenant });

    // **`record!(current, …)`**: code that only has the ambient context can
    // still record registry attributes (registry-wide checks only).
    // **`in_scope`** runs a closure with the span as the current span.
    note_cart(&span, req.skus.len());

    // **Child spans**: pass `child_context()`, not the span.
    let email = req.customer.as_ref().map(|c| c.email.as_str());
    let outcome = authorize_payment(&span.child_context(), &req.payment, email)
        .and_then(|()| insert_order(&span.child_context()));

    metric!(shop.checkout.active: -1, { shop.tenant.code: &req.tenant });
    let elapsed = started.elapsed();

    match outcome {
        Ok(order_id) => {
            // **Events** are log records with an `event_name`; arrays are
            // `Vec`s of the element type.
            event!(shop.order.placed {
                shop.order.id: &order_id,
                shop.cart.item_count: req.skus.len(),
                shop.cart.skus: req.skus.clone(),
                shop.payment.method: &req.payment,
            });
            // **Counters** take unsigned values (a `usize` length as is);
            // enum attributes take the generated enum.
            metric!(shop.cart.items_sold: req.skus.len(), {
                shop.payment.method: &req.payment,
                shop.tenant.code: &req.tenant,
            });
            // **Histograms** with unit `s` take a `Duration`.
            record_duration(elapsed, 201, None, &req.tenant);
            let span = record!(span, http.response.status_code: 201);
            span.end();
            Ok(order_id)
        }
        Err(e) => {
            // **Logs** are `tracing` events with registry keys; `group …`
            // checks a public attribute group's requirements.
            warn!(
                http.route: "/cart/{id}/checkout",
                group exception { exception.type: "PaymentDeclined", exception.message: e.to_string() },
                "checkout failed"
            );
            record_duration(elapsed, 402, Some("payment_declined"), &req.tenant);
            record!(&span, http.response.status_code: 402);
            // **Error paths**: `end!` records `error.type` (checked against the
            // span), the `exception` event with the error's source chain and
            // the status `Error` with the error's message, then the checked `end()`.
            end!(span, error: &e, error.type: "payment_declined");
            Err(402)
        }
    }
}

/// Records how long a checkout took.
fn record_duration(elapsed: Duration, status: i64, error: Option<&'static str>, tenant: &str) {
    // **`extra(…)`**: `shop.tenant.code` is a registry attribute, but
    // `http.server.request.duration` doesn't define it. It's typed and checked
    // like any registry attribute, and warns / fails CI unless approved with
    // `reviewed: "why"`.
    metric!(http.server.request.duration: elapsed, {
        http.request.method: "POST",
        http.route: "/cart/{id}/checkout",
        http.response.status_code: status,
        error.type?: error,
        extra(shop.tenant.code, reviewed: "per-tenant latency until the metric has it"): tenant,
    });
}

/// Notes the cart size on whatever span is current.
fn note_cart_size(items: usize) {
    record!(current, shop.cart.item_count: items);
}

/// Asks the payment provider to authorize the payment.
fn authorize_payment(
    parent: &Context,
    method: &PaymentMethod,
    email: Option<&str>,
) -> Result<(), PaymentDeclined> {
    // The span's name comes from its template: `authorize card`.
    let span = span!(shop.payment.authorize { shop.payment.method: method }, parent: parent);
    // **`code_generation.rust.type`**: this `double` (seconds) takes a
    // `Duration`.
    let span = record!(span, shop.payment.timeout: Duration::from_secs(5));
    // **PII**: `hashed(…)` records an HMAC-SHA256, so equal emails correlate.
    let span = record!(span, shop.customer.email?: hashed(email.map(str::to_string)));
    // **`unregistered(…)`**: a key the registry doesn't have. Untyped, and
    // warns / fails CI unless approved like this.
    let span = record!(span, unregistered("shop.experiment.fast_auth", reviewed: "experiment flag; registry if it stays"): true);

    if *method == PaymentMethod::Invoice {
        let declined = PaymentDeclined {
            provider: ProviderError("invoices need a credit check"),
        };
        end!(span, error: &declined, error.type: "declined");
        return Err(declined);
    }
    span.end();
    Ok(())
}

/// Notes the cart size on `span`, through code that only sees the current
/// span.
///
/// **`StartedSpan`**: a helper that works with any started span takes
/// `impl StartedSpan` (the span's type spells out its state).
fn note_cart(span: &impl semlex::StartedSpan, items: usize) {
    span.in_scope(|| note_cart_size(items));
}

/// The span for inserting an order, set up in one place.
///
/// **`Ready`**: each span type has a `Ready` alias, its started state with
/// every required attribute set, so a function can return it (through
/// `ready()`, which names any missing attribute) and callers record and
/// `end()` it as usual.
fn order_span(parent: &Context, order_id: &str) -> spans::shop::orders::insert::Ready {
    // **Refinements**: `shop.orders.insert` is `db.client` plus the order id.
    // **Prose names**: `db.client` names its spans in prose, so `name:` is
    // required.
    let span = span!(
        shop.orders.insert { db.system.name: "postgresql" },
        parent: parent,
        name: "INSERT orders"
    );
    let span = record!(span, db.namespace: "shop", shop.order.id: order_id);
    // `ready()` names any required attribute still missing.
    span.ready()
}

/// Stores the order.
fn insert_order(parent: &Context) -> Result<String, PaymentDeclined> {
    let order_id = "ord-1001".to_string();
    let span = order_span(parent, &order_id);
    record!(&span, db.operation.name: "INSERT");
    span.end();
    Ok(order_id)
}

/// Reports warehouse fill. **Opt-in signals** do nothing unless enabled by
/// name in `semlex::init` (`.enable("shop.inventory.fill_ratio")`).
pub fn report_inventory(fast: f64, bulk: f64) {
    metric!(shop.inventory.fill_ratio: fast, { shop.inventory.fill_ratio_tier: "fast" });
    metric!(shop.inventory.fill_ratio: bulk, { shop.inventory.fill_ratio_tier: "bulk" });
}

/// Records a request with a method the registry doesn't list.
///
/// **Open enums**: `custom(…)` records a value outside the registry's
/// members; a mistyped literal (`"PSOT"`) is a compile error instead.
pub fn record_unusual_method(path: &str) {
    let span = span!(http.server.request {
        http.request.method: custom("PURGE"),
        url.scheme: "https",
    });
    let span = record!(span, url.path: path, shop.tenant.code: "ACME");
    let span = record!(span, http.response.status_code: 405);
    span.end();
}

/// Attributes for hand-written OpenTelemetry code.
///
/// **`attrs!`**: registry-checked `KeyValue`s, with no signal.
pub fn link_attributes(order_id: &str) -> [KeyValue; 2] {
    attrs![shop.order.id: order_id, shop.payment.method: "card"]
}
