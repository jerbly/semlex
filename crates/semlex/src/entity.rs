//! Entities (`entity!`) and entity associations.
//!
//! `entity!` builds an [`Entity`]; its attributes go into the SDK `Resource`.
//! Signals declare which entities their telemetry should be associated with.
//! The associations can't be checked at a signal's call site (the Resource is
//! configured elsewhere), so the runtime records each signal type the first
//! time one of its call sites is used, and [`check_associations_with`] checks
//! them against a Resource's attributes.

use opentelemetry::KeyValue;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Mutex;

/// An entity's attributes, from `entity!`, for the SDK `Resource`.
#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    ty: &'static str,
    attributes: Vec<KeyValue>,
}

impl Entity {
    #[doc(hidden)]
    pub fn new(ty: &'static str, attributes: Vec<KeyValue>) -> Self {
        Self { ty, attributes }
    }

    /// The entity type (or refinement id), e.g. `service`.
    pub fn entity_type(&self) -> &'static str {
        self.ty
    }

    /// The attributes.
    pub fn attributes(&self) -> &[KeyValue] {
        &self.attributes
    }
}

impl IntoIterator for Entity {
    type Item = KeyValue;
    type IntoIter = std::vec::IntoIter<KeyValue>;
    fn into_iter(self) -> Self::IntoIter {
        self.attributes.into_iter()
    }
}

/// An `entity_associations` expression.
#[derive(Debug)]
pub enum Association {
    /// An entity, present when every identity attribute is in the Resource.
    Entity {
        /// Entity type.
        ty: &'static str,
        /// Its identity attributes (empty if the registry doesn't define it).
        identity: &'static [&'static str],
    },
    /// At least one of these.
    OneOf(&'static [Association]),
    /// All of these.
    AllOf(&'static [Association]),
}

impl Association {
    fn satisfied(&self, has: &dyn Fn(&str) -> bool) -> bool {
        match self {
            Association::Entity { identity, .. } => {
                !identity.is_empty() && identity.iter().all(|k| has(k))
            }
            Association::OneOf(v) => v.iter().any(|a| a.satisfied(has)),
            Association::AllOf(v) => v.iter().all(|a| a.satisfied(has)),
        }
    }
}

impl fmt::Display for Association {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let list = |f: &mut fmt::Formatter<'_>, name: &str, v: &[Association]| {
            write!(f, "{name}(")?;
            for (i, a) in v.iter().enumerate() {
                if i > 0 {
                    f.write_str(", ")?;
                }
                write!(f, "{a}")?;
            }
            f.write_str(")")
        };
        match self {
            Association::Entity { ty, .. } => f.write_str(ty),
            Association::OneOf(v) => list(f, "one_of", v),
            Association::AllOf(v) => list(f, "all_of", v),
        }
    }
}

/// A signal type's entity associations (a top-level list is an implicit
/// `one_of`, as in the registry).
#[derive(Debug)]
pub struct Associations {
    /// `span`, `metric` or `event`.
    pub kind: &'static str,
    /// The signal's id.
    pub signal: &'static str,
    /// The expressions.
    pub expressions: &'static [Association],
}

impl Associations {
    /// Whether a Resource with attributes for which `has(key)` holds
    /// satisfies these associations. No associations: always satisfied.
    pub fn satisfied_by(&self, has: &dyn Fn(&str) -> bool) -> bool {
        self.expressions.is_empty() || self.expressions.iter().any(|a| a.satisfied(has))
    }
}

/// A signal used by this process whose associations the Resource doesn't
/// satisfy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssociationViolation {
    /// `span`, `metric` or `event`.
    pub kind: &'static str,
    /// The signal's id.
    pub signal: &'static str,
    /// The required associations, e.g. `all_of(service, one_of(host, container))`.
    pub expected: String,
}

impl fmt::Display for AssociationViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} `{}` must be associated with {}, which the Resource doesn't provide (an entity counts when all its identity attributes are in the Resource)",
            self.kind, self.signal, self.expected
        )
    }
}

impl std::error::Error for AssociationViolation {}

/// Signal types used so far, by `kind` and id.
static USED: Mutex<BTreeMap<(&'static str, &'static str), &'static Associations>> =
    Mutex::new(BTreeMap::new());

/// Records that a signal type was used; warns once (debug builds) if the
/// Resource given to `semlex::init` doesn't satisfy its associations.
pub(crate) fn observe(a: &'static Associations) {
    let first = USED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert((a.kind, a.signal), a)
        .is_none();
    if first
        && cfg!(debug_assertions)
        && let Some(resource) = crate::init::resource()
        && let Some(v) = violation(a, resource)
    {
        eprintln!("semlex: {v}");
    }
}

fn violation(a: &'static Associations, resource: &[KeyValue]) -> Option<AssociationViolation> {
    let has = |k: &str| resource.iter().any(|kv| kv.key.as_str() == k);
    (!a.satisfied_by(&has)).then(|| AssociationViolation {
        kind: a.kind,
        signal: a.signal,
        expected: a
            .expressions
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join(" | "),
    })
}

/// Checks every signal type used so far against a Resource's attributes.
pub fn check_associations_with(resource: &[KeyValue]) -> Result<(), Vec<AssociationViolation>> {
    let used = USED.lock().unwrap_or_else(|e| e.into_inner());
    let v: Vec<_> = used
        .values()
        .filter_map(|a| violation(a, resource))
        .collect();
    if v.is_empty() { Ok(()) } else { Err(v) }
}

/// Checks every signal type used so far against the Resource given to
/// `semlex::init` (`Config::with_resource`); `Ok` if none was given.
pub fn check_associations() -> Result<(), Vec<AssociationViolation>> {
    match crate::init::resource() {
        Some(r) => check_associations_with(r),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::Association::{AllOf, Entity, OneOf};
    use super::*;

    static SERVICE: Association = Entity {
        ty: "service",
        identity: &["service.name", "service.instance.id"],
    };
    static HOST: Association = Entity {
        ty: "host",
        identity: &["host.name"],
    };
    static CONTAINER: Association = Entity {
        ty: "container",
        identity: &["container.id"],
    };
    static HTTP: Associations = Associations {
        kind: "span",
        signal: "http.server.request",
        expressions: &[AllOf(&[
            Entity {
                ty: "service",
                identity: &["service.name", "service.instance.id"],
            },
            OneOf(&[
                Entity {
                    ty: "host",
                    identity: &["host.name"],
                },
                Entity {
                    ty: "container",
                    identity: &["container.id"],
                },
            ]),
        ])],
    };

    fn has(keys: &'static [&'static str]) -> impl Fn(&str) -> bool {
        move |k| keys.contains(&k)
    }

    #[test]
    fn entity_needs_every_identity_attribute() {
        assert!(SERVICE.satisfied(&has(&["service.name", "service.instance.id"])));
        assert!(!SERVICE.satisfied(&has(&["service.name"])));
        let _ = (&HOST, &CONTAINER);
    }

    #[test]
    fn nested_expressions() {
        assert!(HTTP.satisfied_by(&has(&[
            "service.name",
            "service.instance.id",
            "container.id"
        ])));
        assert!(!HTTP.satisfied_by(&has(&["service.name", "service.instance.id"])));
        assert_eq!(
            HTTP.expressions[0].to_string(),
            "all_of(service, one_of(host, container))"
        );
    }
}
