//! Serde model of Weaver's `resolved/2.0` format, as written by
//! `weaver registry package --v2`.
//!
//! Tolerant of unknown fields everywhere, so a newer Weaver that adds fields
//! still loads. Single-key maps (`{conditionally_required: "…"}`, `{one_of: […]}`)
//! are untagged struct variants, so parsing doesn't depend on how a YAML crate
//! maps enums. Free-form values (examples, annotations) are `serde_json::Value`.

use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;

/// The only `file_format` semlex reads.
pub const FILE_FORMAT: &str = "resolved/2.0";

/// A whole `resolved.yaml`.
#[derive(Debug, Clone, Deserialize)]
pub struct Resolved {
    /// Always [`FILE_FORMAT`]; checked before the rest is parsed.
    pub file_format: String,
    /// The registry's schema URL.
    pub schema_url: String,
    /// Every attribute definition. A key appears more than once when a signal
    /// overrides its brief, note, examples or annotations.
    pub attribute_catalog: Vec<Attribute>,
    /// The registry's own items, referring into the catalog by index.
    pub registry: Registry,
    /// Every signal again, fully resolved, plus refinements.
    pub refinements: Refinements,
    /// Schema URLs of every registry this one was built from;
    /// [`Provenance::source`] indexes into this list.
    #[serde(default)]
    pub dependencies: Vec<String>,
}

/// An attribute definition in the catalog.
#[derive(Debug, Clone, Deserialize)]
pub struct Attribute {
    /// Dotted key, e.g. `http.request.method`.
    pub key: String,
    /// Value type.
    #[serde(rename = "type")]
    pub ty: AttributeType,
    /// One-line description.
    pub brief: String,
    /// Longer description.
    pub note: Option<String>,
    /// Example values.
    pub examples: Option<Value>,
    /// Stability level.
    pub stability: Stability,
    /// Deprecation, if any.
    pub deprecated: Option<Deprecated>,
    /// Annotations. For a catalog entry created by a signal's override, Weaver
    /// writes only the override's map; the effective view re-merges it with
    /// the definition's.
    #[serde(default)]
    pub annotations: BTreeMap<String, Value>,
    /// Set when the attribute comes from a dependency registry.
    pub provenance: Option<Provenance>,
}

/// An attribute's value type, as written in the file.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum AttributeType {
    /// `string`, `int[]`, `any`, `template[string]`, …
    Named(String),
    /// An enum with its members.
    Enum {
        /// The members, in registry order.
        members: Vec<EnumMember>,
    },
}

/// One member of an enum attribute.
#[derive(Debug, Clone, Deserialize)]
pub struct EnumMember {
    /// Identifier, used for the Rust variant name.
    pub id: String,
    /// Wire value. Weaver documents it as defaulting to `id` when omitted.
    pub value: Option<EnumValue>,
    /// One-line description.
    pub brief: Option<String>,
    /// Longer description.
    pub note: Option<String>,
    /// Stability level.
    pub stability: Option<Stability>,
    /// Deprecation, if any.
    pub deprecated: Option<Deprecated>,
    /// Annotations.
    #[serde(default)]
    pub annotations: BTreeMap<String, Value>,
}

/// An enum member's wire value.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum EnumValue {
    /// `true` / `false`.
    Bool(bool),
    /// An integer.
    Int(i64),
    /// A string.
    String(String),
}

/// Stability levels, ordered from least to most stable.
#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Stability {
    /// Formerly "experimental".
    Development,
    /// Alpha.
    Alpha,
    /// Beta.
    Beta,
    /// Release candidate.
    ReleaseCandidate,
    /// Stable.
    Stable,
}

/// Structured deprecation.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum Deprecated {
    /// Renamed to another item.
    Renamed {
        /// The new name.
        renamed_to: String,
        /// Context.
        note: Option<String>,
    },
    /// Removed with no replacement.
    Obsoleted {
        /// Context.
        note: Option<String>,
    },
    /// Anything else.
    Uncategorized {
        /// Context.
        note: Option<String>,
    },
    /// No reason given.
    Unspecified {
        /// Context.
        note: Option<String>,
    },
}

/// Where an attribute or signal was defined.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Provenance {
    /// Index into [`Resolved::dependencies`]; absent for this registry.
    pub source: Option<u32>,
}

/// The registry's own items.
#[derive(Debug, Clone, Deserialize)]
pub struct Registry {
    /// Catalog indexes of each attribute's definition, one per key.
    pub attributes: Vec<usize>,
    /// Public attribute groups. Internal groups don't survive resolution.
    pub attribute_groups: Vec<AttributeGroup>,
    /// Spans.
    pub spans: Vec<Span>,
    /// Metrics.
    pub metrics: Vec<Metric>,
    /// Events.
    pub events: Vec<Event>,
    /// Entities.
    pub entities: Vec<Entity>,
}

/// Every signal fully resolved, including each base signal repeated with
/// `id` equal to its own name.
#[derive(Debug, Clone, Deserialize)]
pub struct Refinements {
    /// Span refinements.
    pub spans: Vec<Span>,
    /// Metric refinements.
    pub metrics: Vec<Metric>,
    /// Event refinements.
    pub events: Vec<Event>,
    /// Entity refinements.
    #[serde(default)]
    pub entities: Vec<Entity>,
}

/// A signal's reference to a catalog entry.
#[derive(Debug, Clone, Deserialize)]
pub struct AttributeRef {
    /// Index into [`Resolved::attribute_catalog`].
    pub base: usize,
    /// Requirement level on this signal.
    pub requirement_level: RequirementLevel,
    /// Spans only: must be set at span start.
    #[serde(default)]
    pub sampling_relevant: Option<bool>,
}

/// An identity attribute: a catalog index (Weaver 0.28) or a reference
/// (Weaver 0.27). Identity attributes are always required.
#[derive(Deserialize)]
#[serde(untagged)]
enum IdentityRef {
    Index(usize),
    Ref(AttributeRef),
}

fn identity_refs<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<AttributeRef>, D::Error> {
    let refs = Vec::<IdentityRef>::deserialize(d)?;
    Ok(refs
        .into_iter()
        .map(|r| match r {
            IdentityRef::Index(base) => AttributeRef {
                base,
                requirement_level: RequirementLevel::Basic(BasicLevel::Required),
                sampling_relevant: None,
            },
            IdentityRef::Ref(r) => r,
        })
        .collect())
}

/// An attribute's requirement level on a signal.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum RequirementLevel {
    /// `required`, `recommended` or `opt_in`.
    Basic(BasicLevel),
    /// `{conditionally_required: "<condition>"}`.
    ConditionallyRequired {
        /// The condition, in prose.
        conditionally_required: String,
    },
    /// `{recommended: "<reason>"}`.
    Recommended {
        /// The reason, in prose.
        recommended: String,
    },
    /// `{opt_in: "<reason>"}`.
    OptIn {
        /// The reason, in prose.
        opt_in: String,
    },
}

/// The plain-string requirement levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BasicLevel {
    /// Must be set.
    Required,
    /// Should be set.
    Recommended,
    /// Set only when the user opts in.
    OptIn,
}

/// A signal's own requirement level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalLevel {
    /// Emitted by default.
    #[default]
    Recommended,
    /// Disabled by default.
    OptIn,
}

/// An `entity_associations` expression.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum EntityAssociation {
    /// A single entity, by type or refinement id.
    Entity {
        /// The entity type.
        #[serde(rename = "type")]
        ty: String,
        /// Set when the entity is defined in a dependency.
        provenance: Option<Provenance>,
    },
    /// At least one of these.
    OneOf {
        /// The alternatives.
        one_of: Vec<EntityAssociation>,
    },
    /// All of these.
    AllOf {
        /// The requirements.
        all_of: Vec<EntityAssociation>,
    },
}

/// A public attribute group.
#[derive(Debug, Clone, Deserialize)]
pub struct AttributeGroup {
    /// Group id.
    pub id: String,
    /// Member attributes.
    pub attributes: Vec<AttributeRef>,
    /// One-line description.
    pub brief: String,
    /// Longer description.
    pub note: Option<String>,
    /// Stability level.
    pub stability: Stability,
    /// Deprecation, if any.
    pub deprecated: Option<Deprecated>,
    /// Annotations.
    #[serde(default)]
    pub annotations: BTreeMap<String, Value>,
    /// Set when defined in a dependency.
    pub provenance: Option<Provenance>,
}

/// A span, or a span refinement when `id` is set.
#[derive(Debug, Clone, Deserialize)]
pub struct Span {
    /// Refinement id; equals `ty` for a base span repeated in `refinements`.
    pub id: Option<String>,
    /// Span type (the base span's type for a refinement).
    #[serde(rename = "type")]
    pub ty: String,
    /// Span kind.
    pub kind: SpanKind,
    /// Naming rules.
    pub name: SpanName,
    /// Member attributes, inheritance and `ref_group`s already applied.
    #[serde(default)]
    pub attributes: Vec<AttributeRef>,
    /// Entity associations.
    #[serde(default)]
    pub entity_associations: Vec<EntityAssociation>,
    /// Signal requirement level.
    #[serde(default)]
    pub requirement_level: SignalLevel,
    /// One-line description.
    pub brief: String,
    /// Longer description.
    pub note: Option<String>,
    /// Stability level.
    pub stability: Stability,
    /// Deprecation, if any.
    pub deprecated: Option<Deprecated>,
    /// Annotations.
    #[serde(default)]
    pub annotations: BTreeMap<String, Value>,
    /// Set when defined in a dependency.
    pub provenance: Option<Provenance>,
}

/// Span kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanKind {
    /// Outgoing request.
    Client,
    /// Incoming request.
    Server,
    /// Enqueue.
    Producer,
    /// Dequeue.
    Consumer,
    /// Internal operation.
    Internal,
}

/// How a span is named.
#[derive(Debug, Clone, Deserialize)]
pub struct SpanName {
    /// Ordered templates; the first whose attributes are all usable wins.
    #[serde(default)]
    pub templates: Vec<NameTemplate>,
    /// Prose naming rule, when there are no templates.
    pub note: Option<String>,
}

/// One span-name template, pre-parsed by Weaver.
#[derive(Debug, Clone, Deserialize)]
pub struct NameTemplate {
    /// The template as written, e.g. `{http.request.method} {http.route}`.
    pub pattern: String,
    /// Attributes the template references.
    #[serde(default)]
    pub attributes: Vec<String>,
    /// The pattern split into literals and attribute references.
    #[serde(default)]
    pub parts: Vec<NamePart>,
}

/// A piece of a span-name template.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum NamePart {
    /// Literal text.
    Literal {
        /// The text.
        value: String,
    },
    /// An attribute's value.
    Attribute {
        /// The attribute key.
        attribute: String,
    },
}

/// A metric, or a metric refinement when `id` is set.
#[derive(Debug, Clone, Deserialize)]
pub struct Metric {
    /// Refinement id; equals `name` for a base metric repeated in `refinements`.
    pub id: Option<String>,
    /// Metric name (the base metric's name for a refinement).
    pub name: String,
    /// Instrument kind.
    pub instrument: Instrument,
    /// UCUM unit.
    pub unit: String,
    /// Member attributes.
    #[serde(default)]
    pub attributes: Vec<AttributeRef>,
    /// Entity associations.
    #[serde(default)]
    pub entity_associations: Vec<EntityAssociation>,
    /// Signal requirement level.
    #[serde(default)]
    pub requirement_level: SignalLevel,
    /// One-line description.
    pub brief: String,
    /// Longer description.
    pub note: Option<String>,
    /// Stability level.
    pub stability: Stability,
    /// Deprecation, if any.
    pub deprecated: Option<Deprecated>,
    /// Annotations.
    #[serde(default)]
    pub annotations: BTreeMap<String, Value>,
    /// Set when defined in a dependency.
    pub provenance: Option<Provenance>,
}

/// Metric instrument kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Instrument {
    /// Monotonic sum.
    #[serde(rename = "counter")]
    Counter,
    /// Non-monotonic sum.
    #[serde(rename = "updowncounter")]
    UpDownCounter,
    /// Last value.
    #[serde(rename = "gauge")]
    Gauge,
    /// Distribution.
    #[serde(rename = "histogram")]
    Histogram,
}

/// An event, or an event refinement when `id` is set.
#[derive(Debug, Clone, Deserialize)]
pub struct Event {
    /// Refinement id; equals `name` for a base event repeated in `refinements`.
    pub id: Option<String>,
    /// Event name. For refinements, Weaver (0.27, 0.28) writes the refinement's
    /// definition id here rather than the base event's name (see
    /// `fixtures/README.md`), so it can't identify the base.
    pub name: String,
    /// Member attributes.
    #[serde(default)]
    pub attributes: Vec<AttributeRef>,
    /// Entity associations.
    #[serde(default)]
    pub entity_associations: Vec<EntityAssociation>,
    /// Signal requirement level.
    #[serde(default)]
    pub requirement_level: SignalLevel,
    /// One-line description.
    pub brief: String,
    /// Longer description.
    pub note: Option<String>,
    /// Stability level.
    pub stability: Stability,
    /// Deprecation, if any.
    pub deprecated: Option<Deprecated>,
    /// Annotations.
    #[serde(default)]
    pub annotations: BTreeMap<String, Value>,
    /// Set when defined in a dependency.
    pub provenance: Option<Provenance>,
}

/// An entity, or an entity refinement when `id` is set.
#[derive(Debug, Clone, Deserialize)]
pub struct Entity {
    /// Refinement id; equals `ty` for a base entity repeated in `refinements`.
    pub id: Option<String>,
    /// Entity type (the base entity's type for a refinement).
    #[serde(rename = "type")]
    pub ty: String,
    /// Attributes that identify an instance; always required. Weaver 0.28
    /// writes catalog indices; 0.27 wrote references (both are accepted).
    #[serde(default, deserialize_with = "identity_refs")]
    pub identity: Vec<AttributeRef>,
    /// Descriptive attributes, with their own requirement levels.
    #[serde(default)]
    pub description: Vec<AttributeRef>,
    /// Signal requirement level.
    #[serde(default)]
    pub requirement_level: SignalLevel,
    /// One-line description.
    pub brief: String,
    /// Longer description.
    pub note: Option<String>,
    /// Stability level.
    pub stability: Stability,
    /// Deprecation, if any.
    pub deprecated: Option<Deprecated>,
    /// Annotations.
    #[serde(default)]
    pub annotations: BTreeMap<String, Value>,
    /// Set when defined in a dependency.
    pub provenance: Option<Provenance>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_accepts_indices_and_references() {
        // Weaver 0.28 writes indices; 0.27 wrote references.
        for yaml in [
            "type: host\nidentity: [3]\nbrief: A host.\nstability: stable\n",
            "type: host\nidentity:\n- base: 3\n  requirement_level: recommended\nbrief: A host.\nstability: stable\n",
        ] {
            let e: Entity = serde_saphyr::from_str(yaml).expect(yaml);
            assert_eq!(e.identity.len(), 1, "{yaml}");
            assert_eq!(e.identity[0].base, 3, "{yaml}");
        }
    }
}
