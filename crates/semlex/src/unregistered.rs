//! `unregistered("key"): value`, the escape hatch for attributes outside the
//! registry.
//!
//! Each use warns in every build (a deprecation marker at the key) and calls
//! [`unregistered_attribute`], which the workspace `clippy.toml` bans so CI
//! fails unless the use is approved with
//! `#[allow(deprecated, clippy::disallowed_methods)]` on a `let` or item.

use crate::__private::rt::str_eq;
use opentelemetry::{Key, KeyValue, StringValue, Value};
use std::borrow::Cow;
use tracing::field::DisplayValue;

/// Marker called by every `unregistered(…)` use. Ban it in `clippy.toml`:
///
/// ```toml
/// disallowed-methods = [
///     { path = "semlex::__private::unregistered_attribute", reason = "attribute is not in the semantic-convention registry; add it to the registry instead of using `unregistered(…)`" },
/// ]
/// ```
#[inline(always)]
pub fn unregistered_attribute() {}

/// Marker called by every `extra(…)` use: a registry attribute on a signal
/// that doesn't define it. Ban it in `clippy.toml` like
/// [`unregistered_attribute`].
#[inline(always)]
pub fn extra_attribute() {}

/// A value for an unregistered attribute: untyped, so any OpenTelemetry
/// attribute value type.
#[diagnostic::on_unimplemented(
    message = "`{Self}` can't be the value of an unregistered attribute",
    note = "use an integer, float, `bool`, `&'static str`, `String`, `Cow<'static, str>` or a `Vec` of those"
)]
pub trait UnregisteredValue {
    /// The `tracing` field type, for the log macros.
    type Field: tracing::Value;
    /// The OpenTelemetry value.
    fn into_value(self) -> Value;
    /// The `tracing` field.
    fn into_field(self) -> Self::Field;
}

macro_rules! unregistered_value {
    ($field:ty: $($t:ty => |$v:ident| ($value:expr, $f:expr)),+ $(,)?) => {
        $(impl UnregisteredValue for $t {
            type Field = $field;
            fn into_value(self) -> Value {
                let $v = self;
                $value
            }
            fn into_field(self) -> $field {
                let $v = self;
                $f
            }
        })+
    };
}

unregistered_value!(i64:
    i64 => |v| (Value::I64(v), v),
    i32 => |v| (Value::I64(v.into()), v.into()),
    i16 => |v| (Value::I64(v.into()), v.into()),
    i8 => |v| (Value::I64(v.into()), v.into()),
    u32 => |v| (Value::I64(v.into()), v.into()),
    u16 => |v| (Value::I64(v.into()), v.into()),
    u8 => |v| (Value::I64(v.into()), v.into()),
);
unregistered_value!(f64: f64 => |v| (Value::F64(v), v), f32 => |v| (Value::F64(v.into()), v.into()));
unregistered_value!(bool: bool => |v| (Value::Bool(v), v));
unregistered_value!(DisplayValue<String>:
    &'static str => |v| (Value::from(v), tracing::field::display(v.to_string())),
    String => |v| (Value::from(v.clone()), tracing::field::display(v)),
    Cow<'static, str> => |v| (Value::from(v.clone()), tracing::field::display(v.into_owned())),
    Vec<i64> => |v| (Value::Array(v.clone().into()), tracing::field::display(format!("{v:?}"))),
    Vec<f64> => |v| (Value::Array(v.clone().into()), tracing::field::display(format!("{v:?}"))),
    Vec<bool> => |v| (Value::Array(v.clone().into()), tracing::field::display(format!("{v:?}"))),
    Vec<String> => |v| (Value::Array(v.iter().cloned().map(StringValue::from).collect::<Vec<_>>().into()), tracing::field::display(format!("{v:?}"))),
);

/// The `KeyValue` for an unregistered attribute.
pub fn unregistered_kv<V: UnregisteredValue>(key: &'static str, value: V) -> KeyValue {
    KeyValue::new(Key::from_static_str(key), value.into_value())
}

/// The `tracing` field for an unregistered attribute.
pub fn unregistered_field<V: UnregisteredValue>(value: V) -> V::Field {
    value.into_field()
}

/// Compile-time check that an unregistered key isn't actually in the
/// registry (as an attribute, or under a template attribute's prefix).
pub const fn check_unregistered(key: &str, registry_keys: &[&str], template_prefixes: &[&str]) {
    let mut i = 0;
    while i < registry_keys.len() {
        if str_eq(key, registry_keys[i]) {
            panic!(
                "this key is an attribute in the semantic-convention registry: use it directly instead of `unregistered(…)`, or with `extra(…)` if the signal doesn't define it"
            );
        }
        i += 1;
    }
    let k = key.as_bytes();
    let mut i = 0;
    while i < template_prefixes.len() {
        let p = template_prefixes[i].as_bytes();
        if k.len() > p.len() && k[p.len()] == b'.' {
            let mut j = 0;
            let mut same = true;
            while j < p.len() {
                if k[j] != p[j] {
                    same = false;
                    break;
                }
                j += 1;
            }
            if same {
                panic!(
                    "this key belongs to a template attribute in the registry: use it with a key suffix, e.g. `http.request.header(\"content-type\")`, instead of `unregistered(…)`"
                );
            }
        }
        i += 1;
    }
}
