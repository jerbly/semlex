//! Built-in annotation handlers, written with the same [`handler`](crate::handler)
//! API as user handlers.

/// `sensitivity: pii`: the attribute takes only `redacted(…)` or `hashed(…)`
/// values (see [`crate::pii`]).
#[crate::handler]
pub mod sensitivity {
    /// Wraps the attribute's kind so only PII wrappers convert into it.
    pub type Kind<K> = crate::kind::Pii<K>;

    /// `pii` is the only level.
    pub const fn check_literal<V>(value: V, _level: &str) {
        // Literals of PII attributes are rejected by the value check, which
        // asks for `redacted(…)` or `hashed(…)`.
        ::core::mem::forget(value)
    }
}
