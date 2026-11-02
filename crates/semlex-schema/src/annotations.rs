//! Annotations: merging and typed access to the built-in ones.

use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;

/// An item's annotations, keyed by top-level namespace (`code_generation`,
/// `cardinality`, `demo`, …).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Annotations(BTreeMap<String, Value>);

/// `code_generation.metric_value_type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricValueType {
    /// Integer instrument.
    Int,
    /// Floating-point instrument (the default when unset).
    Double,
}

/// `cardinality`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cardinality {
    /// A small, known set of values.
    Bounded,
    /// Unbounded; not allowed on metrics.
    Unbounded,
}

/// `sensitivity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sensitivity {
    /// Personally identifiable information; values must be wrapped.
    Pii,
}

/// A built-in annotation whose value has the wrong shape.
#[derive(Debug, Clone, PartialEq)]
pub struct AnnotationError {
    /// Dotted path of the annotation, e.g. `code_generation.exclude`.
    pub path: String,
    /// What was expected, e.g. "`true` or `false`".
    pub expected: &'static str,
    /// The value found, as YAML-ish text.
    pub found: String,
}

impl fmt::Display for AnnotationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "annotation `{}` must be {}, found `{}`",
            self.path, self.expected, self.found
        )
    }
}

impl std::error::Error for AnnotationError {}

impl Annotations {
    /// Wraps a map as read from the file.
    pub fn new(map: BTreeMap<String, Value>) -> Self {
        Self(map)
    }

    /// The definition's annotations with a reference's on top: per key, and
    /// recursively into maps, the reference wins.
    ///
    /// Weaver (0.27, 0.28) writes only the reference's map for an override, dropping
    /// the definition's other keys, so this is needed to keep, for example,
    /// `sensitivity: pii` when a signal sets only `cardinality`.
    pub fn merged(
        definition: &BTreeMap<String, Value>,
        reference: &BTreeMap<String, Value>,
    ) -> Self {
        let mut out = definition.clone();
        for (k, v) in reference {
            match out.get_mut(k) {
                Some(existing) => deep_merge(existing, v),
                None => {
                    out.insert(k.clone(), v.clone());
                }
            }
        }
        Self(out)
    }

    /// True when there are no annotations.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Top-level namespaces present.
    pub fn namespaces(&self) -> impl Iterator<Item = &str> {
        self.0.keys().map(String::as_str)
    }

    /// The raw value of a top-level namespace (user-defined annotations).
    pub fn get(&self, namespace: &str) -> Option<&Value> {
        self.0.get(namespace)
    }

    /// The raw value at a path of map keys, e.g. `["code_generation", "rust", "ident"]`.
    pub fn at(&self, path: &[&str]) -> Option<&Value> {
        let (first, rest) = path.split_first()?;
        let mut v = self.0.get(*first)?;
        for k in rest {
            v = v.as_object()?.get(*k)?;
        }
        Some(v)
    }

    /// The underlying map.
    pub fn as_map(&self) -> &BTreeMap<String, Value> {
        &self.0
    }

    /// `code_generation.exclude`; `false` when unset.
    pub fn code_generation_exclude(&self) -> Result<bool, AnnotationError> {
        match self.at(&["code_generation", "exclude"]) {
            None => Ok(false),
            Some(Value::Bool(b)) => Ok(*b),
            Some(v) => Err(err("code_generation.exclude", "`true` or `false`", v)),
        }
    }

    /// `code_generation.metric_value_type`.
    pub fn metric_value_type(&self) -> Result<Option<MetricValueType>, AnnotationError> {
        self.word(
            &["code_generation", "metric_value_type"],
            "`int` or `double`",
            |s| match s {
                "int" => Some(MetricValueType::Int),
                "double" => Some(MetricValueType::Double),
                _ => None,
            },
        )
    }

    /// `cardinality`.
    pub fn cardinality(&self) -> Result<Option<Cardinality>, AnnotationError> {
        self.word(&["cardinality"], "`bounded` or `unbounded`", |s| match s {
            "bounded" => Some(Cardinality::Bounded),
            "unbounded" => Some(Cardinality::Unbounded),
            _ => None,
        })
    }

    /// `sensitivity`.
    pub fn sensitivity(&self) -> Result<Option<Sensitivity>, AnnotationError> {
        self.word(&["sensitivity"], "`pii`", |s| {
            (s == "pii").then_some(Sensitivity::Pii)
        })
    }

    /// `code_generation.rust.type`: a Rust type path for the value.
    pub fn rust_type(&self) -> Result<Option<&str>, AnnotationError> {
        self.string(&["code_generation", "rust", "type"])
    }

    /// `code_generation.rust.ident`: replaces the attribute's last path segment.
    pub fn rust_ident(&self) -> Result<Option<&str>, AnnotationError> {
        self.string(&["code_generation", "rust", "ident"])
    }

    /// `code_generation.rust.enum_name`: the generated enum's type name.
    pub fn rust_enum_name(&self) -> Result<Option<&str>, AnnotationError> {
        self.string(&["code_generation", "rust", "enum_name"])
    }

    fn string(&self, path: &[&str]) -> Result<Option<&str>, AnnotationError> {
        match self.at(path) {
            None => Ok(None),
            Some(Value::String(s)) => Ok(Some(s)),
            Some(v) => Err(err(&path.join("."), "a string", v)),
        }
    }

    fn word<T>(
        &self,
        path: &[&str],
        expected: &'static str,
        parse: impl Fn(&str) -> Option<T>,
    ) -> Result<Option<T>, AnnotationError> {
        match self.at(path) {
            None => Ok(None),
            Some(v) => v
                .as_str()
                .and_then(&parse)
                .map(Some)
                .ok_or_else(|| err(&path.join("."), expected, v)),
        }
    }
}

fn deep_merge(base: &mut Value, over: &Value) {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                match b.get_mut(k) {
                    Some(existing) => deep_merge(existing, v),
                    None => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (base, over) => *base = over.clone(),
    }
}

fn err(path: &str, expected: &'static str, found: &Value) -> AnnotationError {
    AnnotationError {
        path: path.to_string(),
        expected,
        found: found.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map(v: Value) -> BTreeMap<String, Value> {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn reference_wins_per_key_and_keeps_other_keys() {
        let def = map(json!({"sensitivity": "pii", "cardinality": "unbounded"}));
        let reference = map(json!({"cardinality": "bounded"}));
        let a = Annotations::merged(&def, &reference);
        assert_eq!(a.cardinality(), Ok(Some(Cardinality::Bounded)));
        assert_eq!(a.sensitivity(), Ok(Some(Sensitivity::Pii)));
    }

    #[test]
    fn merge_is_deep() {
        let def = map(json!({"code_generation": {"exclude": false, "rust": {"ident": "a"}}}));
        let reference = map(json!({"code_generation": {"rust": {"type": "Duration"}}}));
        let a = Annotations::merged(&def, &reference);
        assert_eq!(a.code_generation_exclude(), Ok(false));
        assert_eq!(a.rust_ident(), Ok(Some("a")));
        assert_eq!(a.rust_type(), Ok(Some("Duration")));
    }

    #[test]
    fn wrong_shapes_are_errors_naming_the_path() {
        let a = Annotations::new(map(
            json!({"cardinality": "huge", "code_generation": {"exclude": "yes"}}),
        ));
        let e = a.cardinality().unwrap_err();
        assert_eq!(
            e.to_string(),
            "annotation `cardinality` must be `bounded` or `unbounded`, found `\"huge\"`"
        );
        assert_eq!(
            a.code_generation_exclude().unwrap_err().path,
            "code_generation.exclude"
        );
    }

    #[test]
    fn unset_built_ins_have_defaults() {
        let a = Annotations::default();
        assert_eq!(a.code_generation_exclude(), Ok(false));
        assert_eq!(a.metric_value_type(), Ok(None));
        assert_eq!(a.rust_ident(), Ok(None));
    }
}
