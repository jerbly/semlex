//! The effective view: what the rest of semlex consumes.
//!
//! Built from the raw [`model`](crate::model) by joining catalog indexes,
//! re-merging annotations (definition, then reference), dropping the base
//! signals Weaver repeats in `refinements`, and normalizing types and
//! requirement levels.

use crate::Error;
use crate::annotations::{Annotations, Cardinality, Sensitivity};
use crate::model::{
    self, BasicLevel, Deprecated, EntityAssociation, EnumValue, Instrument, Provenance,
    RequirementLevel, SignalLevel, SpanKind, SpanName, Stability,
};
use serde_json::Value;
use std::collections::BTreeMap;

/// A loaded registry.
#[derive(Debug, Clone)]
pub struct Registry {
    /// The registry's schema URL.
    pub schema_url: String,
    /// Schema URLs of dependency registries; [`AttributeDef::provenance`] and
    /// [`Group::provenance`] index into this.
    pub dependencies: Vec<String>,
    /// One definition per attribute key, sorted by key.
    pub attributes: Vec<AttributeDef>,
    /// Every signal, refinement and public attribute group.
    pub groups: Vec<Group>,
    by_key: BTreeMap<String, usize>,
}

impl Registry {
    /// The definition of `key`.
    pub fn attribute(&self, key: &str) -> Option<&AttributeDef> {
        self.by_key.get(key).map(|&i| &self.attributes[i])
    }

    /// Groups of one kind.
    pub fn groups_of(&self, kind: GroupKindTag) -> impl Iterator<Item = &Group> {
        self.groups.iter().filter(move |g| g.kind.tag() == kind)
    }

    /// A group by kind and id.
    pub fn group(&self, kind: GroupKindTag, id: &str) -> Option<&Group> {
        self.groups_of(kind).find(|g| g.id == id)
    }
}

/// An attribute's definition.
#[derive(Debug, Clone)]
pub struct AttributeDef {
    /// Dotted key.
    pub key: String,
    /// Value type.
    pub ty: ValueType,
    /// One-line description.
    pub brief: String,
    /// Longer description.
    pub note: Option<String>,
    /// Example values.
    pub examples: Option<Value>,
    /// Stability (not refinable per signal).
    pub stability: Stability,
    /// Deprecation (not refinable per signal).
    pub deprecated: Option<Deprecated>,
    /// The definition's annotations.
    pub annotations: Annotations,
    /// Index into [`Registry::dependencies`] when imported.
    pub provenance: Option<u32>,
}

/// Primitive value types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Primitive {
    /// `string`.
    String,
    /// `int`.
    Int,
    /// `double`.
    Double,
    /// `boolean`.
    Boolean,
}

/// An attribute's value type.
#[derive(Debug, Clone, PartialEq)]
pub enum ValueType {
    /// A primitive.
    Primitive(Primitive),
    /// An array of a primitive.
    Array(Primitive),
    /// `any`: complex values; only valid where the OTel API can carry them.
    Any,
    /// `template[T]`: a family of keys `<key>.<suffix>` with value type `T`
    /// (never an enum or another template).
    Template(Box<ValueType>),
    /// An enum.
    Enum(EnumDef),
}

/// An enum attribute's members.
#[derive(Debug, Clone, PartialEq)]
pub struct EnumDef {
    /// The wire type shared by every member.
    pub value_type: Primitive,
    /// Members in registry order.
    pub members: Vec<EnumMember>,
}

/// An enum member.
#[derive(Debug, Clone, PartialEq)]
pub struct EnumMember {
    /// Identifier (used for the Rust variant name).
    pub id: String,
    /// Wire value; defaults to `id` when the file omits it.
    pub value: EnumValue,
    /// One-line description.
    pub brief: Option<String>,
    /// Longer description.
    pub note: Option<String>,
    /// Stability; inherits the attribute's when omitted.
    pub stability: Stability,
    /// Deprecation, if any.
    pub deprecated: Option<Deprecated>,
    /// Annotations.
    pub annotations: Annotations,
}

/// The kind of a group, with kind-specific data.
#[derive(Debug, Clone)]
pub enum GroupKind {
    /// A span.
    Span {
        /// Span kind.
        kind: SpanKind,
        /// Naming rules.
        name: SpanName,
    },
    /// A metric.
    Metric {
        /// The metric name on the wire (the base metric's, for a refinement).
        name: String,
        /// Instrument kind.
        instrument: Instrument,
        /// UCUM unit.
        unit: String,
    },
    /// An event.
    Event {
        /// The `event_name` on the wire. `None` for an event refinement:
        /// Weaver (as of 0.28) doesn't record its base event (see `fixtures/README.md`).
        event_name: Option<String>,
    },
    /// An entity.
    Entity,
    /// A public attribute group.
    AttributeGroup,
}

/// [`GroupKind`] without data, for lookups and naming roots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GroupKindTag {
    /// Spans.
    Span,
    /// Metrics.
    Metric,
    /// Events.
    Event,
    /// Entities.
    Entity,
    /// Public attribute groups.
    AttributeGroup,
}

impl GroupKind {
    /// The kind without its data.
    pub fn tag(&self) -> GroupKindTag {
        match self {
            GroupKind::Span { .. } => GroupKindTag::Span,
            GroupKind::Metric { .. } => GroupKindTag::Metric,
            GroupKind::Event { .. } => GroupKindTag::Event,
            GroupKind::Entity => GroupKindTag::Entity,
            GroupKind::AttributeGroup => GroupKindTag::AttributeGroup,
        }
    }
}

impl GroupKindTag {
    /// Lower-case noun used in messages: `span`, `metric`, …
    pub fn noun(self) -> &'static str {
        match self {
            GroupKindTag::Span => "span",
            GroupKindTag::Metric => "metric",
            GroupKindTag::Event => "event",
            GroupKindTag::Entity => "entity",
            GroupKindTag::AttributeGroup => "attribute group",
        }
    }
}

/// What a group refines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refines {
    /// A base signal or attribute group.
    Nothing,
    /// A refinement of the base signal with this id.
    Base(String),
    /// A refinement whose base the resolved file doesn't record (event
    /// refinements, as of Weaver 0.28).
    Unknown,
}

/// A signal, refinement or public attribute group, fully resolved.
#[derive(Debug, Clone)]
pub struct Group {
    /// Kind and kind-specific data.
    pub kind: GroupKind,
    /// The name used in the DSL: the span type, metric or event name, entity
    /// type, refinement id or attribute group id.
    pub id: String,
    /// What this group refines.
    pub refines: Refines,
    /// Member attributes, in the order Weaver writes them (sorted by key;
    /// entity identity attributes first).
    pub attributes: Vec<EffectiveAttribute>,
    /// Signal requirement level.
    pub requirement_level: SignalLevel,
    /// One-line description.
    pub brief: String,
    /// Longer description.
    pub note: Option<String>,
    /// Stability.
    pub stability: Stability,
    /// Deprecation, if any.
    pub deprecated: Option<Deprecated>,
    /// The group's own annotations.
    pub annotations: Annotations,
    /// Entity associations.
    pub entity_associations: Vec<EntityAssociation>,
    /// Index into [`Registry::dependencies`] when imported.
    pub provenance: Option<u32>,
}

impl Group {
    /// Attributes that must be set.
    pub fn required(&self) -> impl Iterator<Item = &EffectiveAttribute> {
        self.attributes
            .iter()
            .filter(|a| a.requirement == Requirement::Required)
    }

    /// A member attribute by key.
    pub fn attribute(&self, key: &str) -> Option<&EffectiveAttribute> {
        self.attributes.iter().find(|a| a.key == key)
    }
}

/// An attribute as seen by one group: the definition with that group's
/// overrides applied.
#[derive(Debug, Clone)]
pub struct EffectiveAttribute {
    /// Dotted key.
    pub key: String,
    /// One-line description (the group's override, if any).
    pub brief: String,
    /// Longer description (the group's override, if any).
    pub note: Option<String>,
    /// Examples (the group's override, if any).
    pub examples: Option<Value>,
    /// Definition annotations with the group's on top.
    pub annotations: Annotations,
    /// Requirement level on this group.
    pub requirement: Requirement,
    /// Spans only: must be set at span start.
    pub sampling_relevant: bool,
    /// Entities only: identity or description.
    pub role: Role,
}

/// Requirement level, normalized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Requirement {
    /// Must be set (including every entity identity attribute).
    Required,
    /// Required under a condition stated in prose.
    ConditionallyRequired(String),
    /// Should be set; optionally with a reason.
    Recommended(Option<String>),
    /// Set only on opt-in; optionally with a reason.
    OptIn(Option<String>),
}

/// An attribute's role in an entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Not an entity attribute.
    Member,
    /// Identifies the entity.
    Identity,
    /// Describes the entity.
    Description,
}

impl Requirement {
    fn from_model(r: &RequirementLevel) -> Self {
        match r {
            RequirementLevel::Basic(BasicLevel::Required) => Requirement::Required,
            RequirementLevel::Basic(BasicLevel::Recommended) => Requirement::Recommended(None),
            RequirementLevel::Basic(BasicLevel::OptIn) => Requirement::OptIn(None),
            RequirementLevel::ConditionallyRequired {
                conditionally_required,
            } => Requirement::ConditionallyRequired(conditionally_required.clone()),
            RequirementLevel::Recommended { recommended } => {
                Requirement::Recommended(Some(recommended.clone()))
            }
            RequirementLevel::OptIn { opt_in } => Requirement::OptIn(Some(opt_in.clone())),
        }
    }
}

fn provenance(p: &Option<Provenance>) -> Option<u32> {
    p.as_ref().and_then(|p| p.source)
}

fn primitive(name: &str) -> Option<Primitive> {
    Some(match name {
        "string" => Primitive::String,
        "int" => Primitive::Int,
        "double" => Primitive::Double,
        "boolean" => Primitive::Boolean,
        _ => return None,
    })
}

fn value_type(
    key: &str,
    ty: &model::AttributeType,
    stability: Stability,
) -> Result<ValueType, Error> {
    let invalid = |msg: String| Error::Invalid(format!("attribute `{key}`: {msg}"));
    match ty {
        model::AttributeType::Named(name) => {
            if let Some(inner) = name
                .strip_prefix("template[")
                .and_then(|s| s.strip_suffix(']'))
            {
                let inner = value_type(
                    key,
                    &model::AttributeType::Named(inner.to_string()),
                    stability,
                )?;
                if matches!(inner, ValueType::Template(_) | ValueType::Enum(_)) {
                    return Err(invalid(format!("unsupported template type `{name}`")));
                }
                return Ok(ValueType::Template(Box::new(inner)));
            }
            if name == "any" {
                return Ok(ValueType::Any);
            }
            if let Some(p) = name.strip_suffix("[]").and_then(primitive) {
                return Ok(ValueType::Array(p));
            }
            primitive(name)
                .map(ValueType::Primitive)
                .ok_or_else(|| invalid(format!("unknown type `{name}`")))
        }
        model::AttributeType::Enum { members } => {
            let members: Vec<EnumMember> = members
                .iter()
                .map(|m| EnumMember {
                    id: m.id.clone(),
                    value: m
                        .value
                        .clone()
                        .unwrap_or_else(|| EnumValue::String(m.id.clone())),
                    brief: m.brief.clone(),
                    note: m.note.clone(),
                    stability: m.stability.unwrap_or(stability),
                    deprecated: m.deprecated.clone(),
                    annotations: Annotations::new(m.annotations.clone()),
                })
                .collect();
            let kind = |v: &EnumValue| match v {
                EnumValue::Bool(_) => Primitive::Boolean,
                EnumValue::Int(_) => Primitive::Int,
                EnumValue::String(_) => Primitive::String,
            };
            let first = members
                .first()
                .ok_or_else(|| invalid("enum has no members".into()))?;
            let value_type = kind(&first.value);
            if let Some(m) = members.iter().find(|m| kind(&m.value) != value_type) {
                return Err(invalid(format!(
                    "enum mixes value types (member `{}`)",
                    m.id
                )));
            }
            Ok(ValueType::Enum(EnumDef {
                value_type,
                members,
            }))
        }
    }
}

/// Every catalog index a signal, refinement or attribute group refers to.
fn referenced(r: &model::Resolved) -> Vec<usize> {
    let (g, f) = (&r.registry, &r.refinements);
    let lists = g
        .attribute_groups
        .iter()
        .map(|x| &x.attributes)
        .chain(g.spans.iter().map(|x| &x.attributes))
        .chain(g.metrics.iter().map(|x| &x.attributes))
        .chain(g.events.iter().map(|x| &x.attributes))
        .chain(
            g.entities
                .iter()
                .flat_map(|x| [&x.identity, &x.description]),
        )
        .chain(f.spans.iter().map(|x| &x.attributes))
        .chain(f.metrics.iter().map(|x| &x.attributes))
        .chain(f.events.iter().map(|x| &x.attributes))
        .chain(
            f.entities
                .iter()
                .flat_map(|x| [&x.identity, &x.description]),
        );
    lists.flatten().map(|a| a.base).collect()
}

/// Builds the effective view from the raw model.
pub(crate) fn build(r: model::Resolved) -> Result<Registry, Error> {
    let cat = &r.attribute_catalog;
    let entry = |i: usize| {
        cat.get(i).ok_or_else(|| {
            Error::Invalid(format!(
                "attribute index {i} is out of range (the catalog has {} entries)",
                cat.len()
            ))
        })
    };

    let def_of = |a: &model::Attribute, annotations: Annotations| -> Result<_, Error> {
        Ok(AttributeDef {
            key: a.key.clone(),
            ty: value_type(&a.key, &a.ty, a.stability)?,
            brief: a.brief.clone(),
            note: a.note.clone(),
            examples: a.examples.clone(),
            stability: a.stability,
            deprecated: a.deprecated.clone(),
            annotations,
            provenance: provenance(&a.provenance),
        })
    };

    // Definitions: one per key.
    let mut attributes = Vec::new();
    for &i in &r.registry.attributes {
        let a = entry(i)?;
        attributes.push(def_of(a, Annotations::new(a.annotations.clone()))?);
    }
    // Attributes defined in a dependency registry and only referenced here
    // aren't in `registry.attributes`; their catalog entries hold the full
    // definition (with `provenance`). A reference that overrides `brief`,
    // `note` or `annotations` gets its own entry, so take the entry most
    // references use, which is the definition as written unless every use
    // overrides it. Annotations merge across all the entries (the most-used
    // wins on a conflict), and `sensitivity: pii` and `cardinality: unbounded`
    // stick: a reference that drops them must not drop the protection.
    let mut defined: std::collections::BTreeSet<String> =
        attributes.iter().map(|a| a.key.clone()).collect();
    let mut uses: BTreeMap<String, BTreeMap<usize, usize>> = BTreeMap::new();
    for i in referenced(&r) {
        let e = entry(i)?;
        if !defined.contains(&e.key) {
            *uses.entry(e.key.clone()).or_default().entry(i).or_default() += 1;
        }
    }
    for (key, entries) in uses {
        // Fewest uses first, so the most-used entry (the lowest index on a
        // tie) merges last and wins.
        let mut order: Vec<(usize, usize)> = entries.into_iter().collect();
        order.sort_by(|(ia, na), (ib, nb)| na.cmp(nb).then(ib.cmp(ia)));
        let mut annotations = Annotations::default();
        let (mut pii, mut unbounded) = (false, false);
        for &(i, _) in &order {
            let one = Annotations::new(entry(i)?.annotations.clone());
            pii |= one.sensitivity() == Ok(Some(Sensitivity::Pii));
            unbounded |= one.cardinality() == Ok(Some(Cardinality::Unbounded));
            annotations = Annotations::merged(annotations.as_map(), one.as_map());
        }
        let mut map = annotations.as_map().clone();
        if pii {
            map.insert("sensitivity".into(), "pii".into());
        }
        if unbounded {
            map.insert("cardinality".into(), "unbounded".into());
        }
        let &(i, _) = order.last().expect("at least one use");
        attributes.push(def_of(entry(i)?, Annotations::new(map))?);
        defined.insert(key);
    }
    attributes.sort_by(|a, b| a.key.cmp(&b.key));
    let mut by_key = BTreeMap::new();
    for (i, a) in attributes.iter().enumerate() {
        if by_key.insert(a.key.clone(), i).is_some() {
            return Err(Error::Invalid(format!(
                "attribute `{}` is defined twice",
                a.key
            )));
        }
    }

    let effective =
        |refs: &[model::AttributeRef], role: Role| -> Result<Vec<EffectiveAttribute>, Error> {
            refs.iter()
                .map(|a| {
                    let e = entry(a.base)?;
                    let def_i = *by_key.get(&e.key).ok_or_else(|| {
                        Error::Invalid(format!(
                            "attribute `{}` is referenced but has no definition",
                            e.key
                        ))
                    })?;
                    let def = &attributes[def_i];
                    Ok(EffectiveAttribute {
                        key: e.key.clone(),
                        brief: e.brief.clone(),
                        note: e.note.clone(),
                        examples: e.examples.clone(),
                        annotations: Annotations::merged(def.annotations.as_map(), &e.annotations),
                        requirement: if role == Role::Identity {
                            Requirement::Required
                        } else {
                            Requirement::from_model(&a.requirement_level)
                        },
                        sampling_relevant: a.sampling_relevant.unwrap_or(false),
                        role,
                    })
                })
                .collect()
        };

    let mut groups = Vec::new();
    macro_rules! common {
        ($s:expr, $kind:expr, $id:expr, $refines:expr, $attrs:expr, $assoc:expr) => {
            Group {
                kind: $kind,
                id: $id,
                refines: $refines,
                attributes: $attrs,
                requirement_level: $s.requirement_level,
                brief: $s.brief.clone(),
                note: $s.note.clone(),
                stability: $s.stability,
                deprecated: $s.deprecated.clone(),
                annotations: Annotations::new($s.annotations.clone()),
                entity_associations: $assoc,
                provenance: provenance(&$s.provenance),
            }
        };
    }
    // A refinements entry whose id equals its base's name repeats a base signal.
    let refinement =
        |id: &Option<String>, base: &str| -> Option<String> { id.clone().filter(|id| id != base) };

    for s in &r.registry.spans {
        let kind = GroupKind::Span {
            kind: s.kind,
            name: s.name.clone(),
        };
        groups.push(common!(
            s,
            kind,
            s.ty.clone(),
            Refines::Nothing,
            effective(&s.attributes, Role::Member)?,
            s.entity_associations.clone()
        ));
    }
    for s in &r.refinements.spans {
        if let Some(id) = refinement(&s.id, &s.ty) {
            let kind = GroupKind::Span {
                kind: s.kind,
                name: s.name.clone(),
            };
            groups.push(common!(
                s,
                kind,
                id,
                Refines::Base(s.ty.clone()),
                effective(&s.attributes, Role::Member)?,
                s.entity_associations.clone()
            ));
        }
    }
    for m in &r.registry.metrics {
        let kind = GroupKind::Metric {
            name: m.name.clone(),
            instrument: m.instrument,
            unit: m.unit.clone(),
        };
        groups.push(common!(
            m,
            kind,
            m.name.clone(),
            Refines::Nothing,
            effective(&m.attributes, Role::Member)?,
            m.entity_associations.clone()
        ));
    }
    for m in &r.refinements.metrics {
        if let Some(id) = refinement(&m.id, &m.name) {
            let kind = GroupKind::Metric {
                name: m.name.clone(),
                instrument: m.instrument,
                unit: m.unit.clone(),
            };
            groups.push(common!(
                m,
                kind,
                id,
                Refines::Base(m.name.clone()),
                effective(&m.attributes, Role::Member)?,
                m.entity_associations.clone()
            ));
        }
    }
    for e in &r.registry.events {
        let kind = GroupKind::Event {
            event_name: Some(e.name.clone()),
        };
        groups.push(common!(
            e,
            kind,
            e.name.clone(),
            Refines::Nothing,
            effective(&e.attributes, Role::Member)?,
            e.entity_associations.clone()
        ));
    }
    for e in &r.refinements.events {
        if let Some(id) = refinement(&e.id, &e.name) {
            // Weaver (0.27, 0.28) writes the refinement's definition id into `name`,
            // not the base event's name, so the base is unknown.
            let base_known = r.registry.events.iter().any(|b| b.name == e.name);
            let (event_name, refines) = if base_known {
                (Some(e.name.clone()), Refines::Base(e.name.clone()))
            } else {
                (None, Refines::Unknown)
            };
            let kind = GroupKind::Event { event_name };
            groups.push(common!(
                e,
                kind,
                id,
                refines,
                effective(&e.attributes, Role::Member)?,
                e.entity_associations.clone()
            ));
        }
    }
    let entity_attrs = |e: &model::Entity| -> Result<Vec<EffectiveAttribute>, Error> {
        let mut v = effective(&e.identity, Role::Identity)?;
        v.extend(effective(&e.description, Role::Description)?);
        Ok(v)
    };
    for e in &r.registry.entities {
        groups.push(common!(
            e,
            GroupKind::Entity,
            e.ty.clone(),
            Refines::Nothing,
            entity_attrs(e)?,
            Vec::new()
        ));
    }
    for e in &r.refinements.entities {
        if let Some(id) = refinement(&e.id, &e.ty) {
            groups.push(common!(
                e,
                GroupKind::Entity,
                id,
                Refines::Base(e.ty.clone()),
                entity_attrs(e)?,
                Vec::new()
            ));
        }
    }
    for g in &r.registry.attribute_groups {
        groups.push(Group {
            kind: GroupKind::AttributeGroup,
            id: g.id.clone(),
            refines: Refines::Nothing,
            attributes: effective(&g.attributes, Role::Member)?,
            requirement_level: SignalLevel::Recommended,
            brief: g.brief.clone(),
            note: g.note.clone(),
            stability: g.stability,
            deprecated: g.deprecated.clone(),
            annotations: Annotations::new(g.annotations.clone()),
            entity_associations: Vec::new(),
            provenance: provenance(&g.provenance),
        });
    }

    Ok(Registry {
        schema_url: r.schema_url,
        dependencies: r.dependencies,
        attributes,
        groups,
        by_key,
    })
}
