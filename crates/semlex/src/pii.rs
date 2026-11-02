//! Values of `sensitivity: pii` attributes.
//!
//! A PII attribute takes only a wrapped value, so a plain value doesn't
//! compile and every use is visible in review:
//!
//! - [`redacted(v)`](redacted) records `[REDACTED]`;
//! - [`hashed(v)`](hashed) records a keyed hash (HMAC-SHA256, hex), so equal
//!   values correlate without exposing them.
//!
//! The hash key is random per process unless set with `set_hash_key`; set
//! it (from a secret) when hashes must correlate across processes. Hashing
//! needs the `pii` feature (on by default).

use crate::kind::Pii;
use crate::{IntoValue, Value};
#[cfg(feature = "pii")]
use hmac::{Hmac, KeyInit, Mac};
#[cfg(feature = "pii")]
use sha2::Sha256;
#[cfg(feature = "pii")]
use std::sync::OnceLock;

/// The value recorded for [`redacted`] values.
pub const REDACTED: &str = "[REDACTED]";

/// A PII value recorded as [`REDACTED`].
#[derive(Debug, Clone, Copy)]
pub struct Redacted<T>(pub T);

/// A PII value recorded as a keyed hash.
#[derive(Debug, Clone, Copy)]
pub struct Hashed<T>(pub T);

/// Records a PII value as `[REDACTED]`.
pub const fn redacted<T>(value: T) -> Redacted<T> {
    Redacted(value)
}

/// Records a PII value as a keyed hash (HMAC-SHA256, hex). Needs the `pii`
/// feature.
pub const fn hashed<T: Hashing>(value: T) -> Hashed<T> {
    Hashed(value)
}

/// Whether [`hashed`] is available: with semlex's `pii` feature.
#[diagnostic::on_unimplemented(
    message = "`hashed(…)` needs semlex's `pii` feature",
    label = "hashing isn't compiled in",
    note = "enable the `pii` feature of `semlex` (it's on by default), or use `redacted(…)`"
)]
pub trait Hashing {}

#[cfg(feature = "pii")]
impl<T> Hashing for T {}

#[cfg(feature = "pii")]
static HASH_KEY: OnceLock<Vec<u8>> = OnceLock::new();

#[cfg(feature = "pii")]
/// Sets the key for [`hashed`] values. Call it once at startup, before any
/// hashed value is recorded; returns `false` if a key was already in use.
pub fn set_hash_key(key: impl Into<Vec<u8>>) -> bool {
    HASH_KEY.set(key.into()).is_ok()
}

#[cfg(feature = "pii")]
fn hash_key() -> &'static [u8] {
    HASH_KEY.get_or_init(|| {
        // Random per process: hashes correlate within a process only.
        use std::hash::{BuildHasher, RandomState};
        let s = RandomState::new();
        (0..4u64)
            .flat_map(|i| s.hash_one(i).to_le_bytes())
            .collect()
    })
}

#[cfg(feature = "pii")]
fn hash(text: &str) -> String {
    let mut mac =
        <Hmac<Sha256> as KeyInit>::new_from_slice(hash_key()).expect("HMAC accepts any key length");
    mac.update(text.as_bytes());
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

impl<K, T: IntoValue<K>> IntoValue<Pii<K>> for Redacted<T> {
    fn into_value(self) -> Value {
        Value::from(REDACTED)
    }
}

#[cfg(feature = "pii")]
impl<K, T: IntoValue<K>> IntoValue<Pii<K>> for Hashed<T> {
    fn into_value(self) -> Value {
        Value::from(hash(&self.0.into_value().as_str()))
    }
}

#[cfg(all(test, feature = "pii"))]
mod tests {
    use super::*;
    use crate::kind::StringValue;

    fn value<V: IntoValue<Pii<StringValue>>>(v: V) -> String {
        v.into_value().as_str().into_owned()
    }

    #[test]
    fn redacted_hides_the_value() {
        assert_eq!(value(redacted("alice@example.com")), REDACTED);
    }

    #[test]
    fn hashed_is_stable_within_a_process_and_hides_the_value() {
        let a = value(hashed("alice@example.com"));
        assert_eq!(a, value(hashed(String::from("alice@example.com"))));
        assert_ne!(a, value(hashed("bob@example.com")));
        assert_eq!(a.len(), 64);
        assert!(!a.contains("alice"));
    }
}
