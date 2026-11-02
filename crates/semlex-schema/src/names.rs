//! Rust names for registry items.
//!
//! - An attribute `a.b.c` becomes `const c` in module `attr::a::b`, typed by a
//!   marker struct `attr_types::a_b_c`. A `const` and a module may share a
//!   name, so `db.operation` and `db.operation.name` coexist.
//! - When two keys give the same marker name (`messaging.client_id` and
//!   `messaging.client.id`), every key in the clash spells its dots `__`.
//! - Enums are named in CamelCase (`HttpRequestMethod`), and members become
//!   CamelCase variants. Clashing enum or variant names spell their dots `_`
//!   (`Process_ContextSwitchType`; `Azure_Aks` next to `AzureAks`). `Custom`
//!   is reserved for values outside the registry.
//! - `code_generation.rust.ident` (attribute or enum member) and
//!   `code_generation.rust.enum_name` override the generated names.
//! - Collisions that remain are errors naming every key involved.

use crate::view::{GroupKindTag, Registry, ValueType};
use std::collections::BTreeMap;
use std::fmt;

/// Reserved words that can be written as raw identifiers.
const RAW_KEYWORDS: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "dyn", "else", "enum", "extern", "false",
    "fn", "for", "gen", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "static", "struct", "trait", "true", "type", "unsafe", "use", "where",
    "while", "abstract", "become", "box", "do", "final", "macro", "override", "priv", "try",
    "typeof", "unsized", "virtual", "yield",
];

/// Reserved words that can't be raw identifiers.
const NON_RAW_KEYWORDS: &[&str] = &["self", "Self", "super", "crate", "_"];

/// The name of the reserved enum variant for values outside the registry.
pub const CUSTOM_VARIANT: &str = "Custom";

/// A valid Rust identifier for one dotted-name segment.
///
/// Characters other than ASCII letters, digits and `_` become `_`; a leading
/// digit gets a `_` prefix; keywords become raw identifiers (`r#type`), or get
/// a `_` suffix when they can't be raw (`self_`).
pub fn ident(segment: &str) -> String {
    let mut s: String = segment
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        s.push('_');
    }
    if s.starts_with(|c: char| c.is_ascii_digit()) {
        s.insert(0, '_');
    }
    if NON_RAW_KEYWORDS.contains(&s.as_str()) {
        s.push('_');
    } else if RAW_KEYWORDS.contains(&s.as_str()) {
        s.insert_str(0, "r#");
    }
    s
}

/// `snake.or-kebab` → `SnakeOrKebab`, valid as a type name.
pub fn camel(s: &str) -> String {
    let mut out = String::new();
    for part in s
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|p| !p.is_empty())
    {
        let mut cs = part.chars();
        out.extend(cs.next().map(|c| c.to_ascii_uppercase()));
        out.extend(cs);
    }
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, 'V');
    }
    if NON_RAW_KEYWORDS.contains(&out.as_str()) {
        out.push('_');
    }
    out
}

/// Names for one attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttributeNames {
    /// Module path under `attr`, e.g. `["http", "request"]`.
    pub module: Vec<String>,
    /// The `const` name, e.g. `method`.
    pub leaf: String,
    /// The marker struct in `attr_types`, e.g. `http_request_method`.
    pub marker: String,
    /// For enums: the enum type name and one variant name per member.
    pub enumeration: Option<EnumNames>,
}

/// Names for an enum attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumNames {
    /// The enum type, e.g. `HttpRequestMethod`.
    pub name: String,
    /// One variant name per member, in member order.
    pub variants: Vec<String>,
}

/// Names for every item in a registry.
#[derive(Debug, Clone, Default)]
pub struct Names {
    /// By attribute key.
    pub attributes: BTreeMap<String, AttributeNames>,
    /// Module path of each group under its kind's root, by kind and id.
    pub groups: BTreeMap<(GroupKindTag, String), Vec<String>>,
}

/// A naming problem in the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameError {
    /// Two attributes map to the same Rust path.
    AttributePath {
        /// The colliding keys.
        keys: Vec<String>,
        /// The shared path.
        path: String,
    },
    /// Two names still clash after disambiguation.
    Marker {
        /// The colliding keys.
        keys: Vec<String>,
        /// The shared name.
        name: String,
    },
    /// Two enums get the same type name.
    EnumName {
        /// The colliding keys.
        keys: Vec<String>,
        /// The shared name.
        name: String,
    },
    /// Two members of one enum get the same variant name.
    Variant {
        /// The enum attribute.
        key: String,
        /// The colliding member ids.
        members: Vec<String>,
        /// The shared variant name.
        name: String,
    },
    /// Two groups of one kind map to the same module path.
    GroupPath {
        /// Kind of the groups.
        kind: GroupKindTag,
        /// The colliding ids.
        ids: Vec<String>,
        /// The shared path.
        path: String,
    },
    /// A `code_generation.rust.*` override is unusable.
    Override {
        /// Where the override is.
        item: String,
        /// What is wrong.
        problem: String,
    },
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let list = |v: &[String]| {
            v.iter()
                .map(|k| format!("`{k}`"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        match self {
            NameError::AttributePath { keys, path } => write!(
                f,
                "attributes {} all map to the Rust path `attr::{path}`; rename one, or set `code_generation.rust.ident` on it",
                list(keys)
            ),
            NameError::Marker { keys, name } => {
                write!(
                    f,
                    "attributes {} all map to the Rust type `{name}`; rename one",
                    list(keys)
                )
            }
            NameError::EnumName { keys, name } => write!(
                f,
                "enum attributes {} all map to the Rust enum `{name}`; set `code_generation.rust.enum_name` on one",
                list(keys)
            ),
            NameError::Variant { key, members, name } => write!(
                f,
                "members {} of enum `{key}` all map to the variant `{name}`; set `code_generation.rust.ident` on one",
                list(members)
            ),
            NameError::GroupPath { kind, ids, path } => {
                write!(
                    f,
                    "{}s {} all map to the Rust path `{path}`; rename one",
                    kind.noun(),
                    list(ids)
                )
            }
            NameError::Override { item, problem } => write!(f, "{item}: {problem}"),
        }
    }
}

impl std::error::Error for NameError {}

fn is_ident(s: &str) -> bool {
    let s = s.strip_prefix("r#").unwrap_or(s);
    !s.is_empty()
        && !s.starts_with(|c: char| c.is_ascii_digit())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !NON_RAW_KEYWORDS.contains(&s)
}

/// Marker name for a key: dots become `sep`, then [`ident`] rules apply.
fn marker_for(key: &str, sep: &str) -> String {
    ident(&key.replace('.', sep))
}

/// Enum name for a key: CamelCase, segments joined with `sep`.
fn enum_for(key: &str, sep: &str) -> String {
    key.split('.').map(camel).collect::<Vec<_>>().join(sep)
}

/// Keys that share a name, and the name.
type Clash = (Vec<String>, String);

/// Assigns `simple(key)` to each key; every key in a clash gets `spelled(key)`
/// instead. Clashes that remain are returned.
fn disambiguate(
    keys: &[&str],
    simple: impl Fn(&str) -> String,
    spelled: impl Fn(&str) -> String,
) -> (BTreeMap<String, String>, Vec<Clash>) {
    let mut by_name: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for k in keys {
        by_name.entry(simple(k)).or_default().push(k);
    }
    let mut out = BTreeMap::new();
    for (name, ks) in &by_name {
        for k in ks {
            out.insert(
                k.to_string(),
                if ks.len() > 1 {
                    spelled(k)
                } else {
                    name.clone()
                },
            );
        }
    }
    let mut again: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (k, n) in &out {
        again.entry(n.clone()).or_default().push(k.clone());
    }
    let clashes = again
        .into_iter()
        .filter(|(_, ks)| ks.len() > 1)
        .map(|(n, ks)| (ks, n))
        .collect();
    (out, clashes)
}

/// Computes every name, or every naming problem.
pub fn names(reg: &Registry) -> Result<Names, Vec<NameError>> {
    let mut errors = Vec::new();
    let mut names = Names::default();
    let keys: Vec<&str> = reg.attributes.iter().map(|a| a.key.as_str()).collect();

    // Marker types.
    let (markers, clashes) = disambiguate(&keys, |k| marker_for(k, "_"), |k| marker_for(k, "__"));
    errors.extend(
        clashes
            .into_iter()
            .map(|(keys, name)| NameError::Marker { keys, name }),
    );

    // Enum type names (overrides first, then CamelCase with clash handling).
    let enum_keys: Vec<&str> = reg
        .attributes
        .iter()
        .filter(|a| matches!(a.ty, ValueType::Enum(_)))
        .map(|a| a.key.as_str())
        .collect();
    let mut overridden = BTreeMap::new();
    for a in reg
        .attributes
        .iter()
        .filter(|a| matches!(a.ty, ValueType::Enum(_)))
    {
        match a.annotations.rust_enum_name() {
            Ok(Some(n)) if is_ident(n) => {
                overridden.insert(a.key.clone(), n.to_string());
            }
            Ok(Some(n)) => errors.push(NameError::Override {
                item: format!("attribute `{}`", a.key),
                problem: format!(
                    "`code_generation.rust.enum_name` `{n}` is not a valid Rust identifier"
                ),
            }),
            Ok(None) => {}
            Err(e) => errors.push(NameError::Override {
                item: format!("attribute `{}`", a.key),
                problem: e.to_string(),
            }),
        }
    }
    let auto: Vec<&str> = enum_keys
        .iter()
        .copied()
        .filter(|k| !overridden.contains_key(*k))
        .collect();
    let (mut enum_names, _) = disambiguate(&auto, |k| enum_for(k, ""), |k| enum_for(k, "_"));
    enum_names.extend(overridden);
    let mut by_enum: BTreeMap<&str, Vec<String>> = BTreeMap::new();
    for (k, n) in &enum_names {
        by_enum.entry(n).or_default().push(k.clone());
    }
    for (n, ks) in by_enum.into_iter().filter(|(_, ks)| ks.len() > 1) {
        errors.push(NameError::EnumName {
            keys: ks,
            name: n.to_string(),
        });
    }

    // Paths and leaves.
    let mut by_path: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for a in &reg.attributes {
        let segs: Vec<&str> = a.key.split('.').collect();
        let (last, module) = segs.split_last().expect("split always yields one segment");
        let leaf = match a.annotations.rust_ident() {
            Ok(Some(n)) if is_ident(n) => n.to_string(),
            Ok(Some(n)) => {
                errors.push(NameError::Override {
                    item: format!("attribute `{}`", a.key),
                    problem: format!(
                        "`code_generation.rust.ident` `{n}` is not a valid Rust identifier"
                    ),
                });
                ident(last)
            }
            Ok(None) => ident(last),
            Err(e) => {
                errors.push(NameError::Override {
                    item: format!("attribute `{}`", a.key),
                    problem: e.to_string(),
                });
                ident(last)
            }
        };
        let module: Vec<String> = module.iter().map(|s| ident(s)).collect();
        let path = module
            .iter()
            .chain([&leaf])
            .cloned()
            .collect::<Vec<_>>()
            .join("::");
        by_path.entry(path).or_default().push(a.key.clone());

        let enumeration = match &a.ty {
            ValueType::Enum(def) => {
                // Overrides first; the rest are CamelCase, and members whose
                // names clash spell their dots `_` (`azure.aks` → `Azure_Aks`
                // next to `azure_aks` → `AzureAks`).
                let mut overrides: BTreeMap<String, String> = BTreeMap::new();
                for m in &def.members {
                    match m.annotations.rust_ident() {
                        Ok(Some(n)) if is_ident(n) => {
                            overrides.insert(m.id.clone(), n.to_string());
                        }
                        Ok(Some(n)) => errors.push(NameError::Override {
                            item: format!("member `{}` of enum `{}`", m.id, a.key),
                            problem: format!(
                                "`code_generation.rust.ident` `{n}` is not a valid Rust identifier"
                            ),
                        }),
                        Ok(None) => {}
                        Err(e) => errors.push(NameError::Override {
                            item: format!("member `{}` of enum `{}`", m.id, a.key),
                            problem: e.to_string(),
                        }),
                    }
                }
                let auto: Vec<&str> = def
                    .members
                    .iter()
                    .map(|m| m.id.as_str())
                    .filter(|id| !overrides.contains_key(*id))
                    .collect();
                let (mut by_member, _) = disambiguate(&auto, camel, |id| enum_for(id, "_"));
                by_member.extend(overrides);
                let variants: Vec<String> = def
                    .members
                    .iter()
                    .map(|m| by_member[&m.id].clone())
                    .collect();
                let mut seen: BTreeMap<&str, Vec<String>> = BTreeMap::new();
                for (m, v) in def.members.iter().zip(&variants) {
                    seen.entry(v).or_default().push(m.id.clone());
                }
                for (v, ids) in seen {
                    if v == CUSTOM_VARIANT {
                        errors.push(NameError::Variant {
                            key: a.key.clone(),
                            members: ids,
                            name: format!(
                                "{CUSTOM_VARIANT} (reserved for values outside the registry)"
                            ),
                        });
                    } else if ids.len() > 1 {
                        errors.push(NameError::Variant {
                            key: a.key.clone(),
                            members: ids,
                            name: v.to_string(),
                        });
                    }
                }
                Some(EnumNames {
                    name: enum_names[&a.key].clone(),
                    variants,
                })
            }
            _ => None,
        };
        names.attributes.insert(
            a.key.clone(),
            AttributeNames {
                module,
                leaf,
                marker: markers[&a.key].clone(),
                enumeration,
            },
        );
    }
    for (path, ks) in by_path.into_iter().filter(|(_, ks)| ks.len() > 1) {
        errors.push(NameError::AttributePath { keys: ks, path });
    }

    // Groups: module path under the kind's root.
    let mut by_group_path: BTreeMap<(GroupKindTag, String), Vec<String>> = BTreeMap::new();
    for g in &reg.groups {
        let path: Vec<String> = g.id.split('.').map(ident).collect();
        by_group_path
            .entry((g.kind.tag(), path.join("::")))
            .or_default()
            .push(g.id.clone());
        names.groups.insert((g.kind.tag(), g.id.clone()), path);
    }
    for ((kind, path), ids) in by_group_path.into_iter().filter(|(_, ids)| ids.len() > 1) {
        errors.push(NameError::GroupPath { kind, ids, path });
    }

    if errors.is_empty() {
        Ok(names)
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idents() {
        assert_eq!(ident("method"), "method");
        assert_eq!(ident("type"), "r#type");
        assert_eq!(ident("self"), "self_");
        assert_eq!(ident("1xx"), "_1xx");
        assert_eq!(ident("x-forwarded-for"), "x_forwarded_for");
        assert_eq!(ident(""), "__");
    }

    #[test]
    fn camels() {
        assert_eq!(camel("http.request.method"), "HttpRequestMethod");
        assert_eq!(camel("_OTHER"), "OTHER");
        assert_eq!(camel("1xx"), "V1xx");
        assert_eq!(camel("self"), "Self_");
    }

    #[test]
    fn clashing_markers_spell_out_dots() {
        let keys = ["messaging.client_id", "messaging.client.id", "http.route"];
        let (m, clashes) = disambiguate(&keys, |k| marker_for(k, "_"), |k| marker_for(k, "__"));
        assert!(clashes.is_empty());
        assert_eq!(m["messaging.client_id"], "messaging__client_id");
        assert_eq!(m["messaging.client.id"], "messaging__client__id");
        assert_eq!(m["http.route"], "http_route");
    }
}
