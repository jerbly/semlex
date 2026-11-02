//! Loading the committed fixtures, and the effective view of the fixture
//! registry as a reviewed snapshot.

use semlex_schema::{
    Error, GroupKind, GroupKindTag, Refines, Registry, Requirement, Role, ValueType,
    annotations::Cardinality, annotations::Sensitivity, names,
};
use std::fmt::Write;
use std::path::PathBuf;

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(rel)
}

fn fixture_registry() -> Registry {
    semlex_schema::load(fixture("resolved/resolved.yaml")).expect("fixture loads")
}

/// One line per group and per member attribute; the snapshot is reviewed
/// against `fixtures/registry/signals.yaml`.
fn render(r: &Registry) -> String {
    let mut out = String::new();
    for g in &r.groups {
        let refines = match &g.refines {
            Refines::Nothing => String::new(),
            Refines::Base(b) => format!(" refines {b}"),
            Refines::Unknown => " refines <unknown>".into(),
        };
        let extra = match &g.kind {
            GroupKind::Span { kind, name } => format!(
                " kind={kind:?} name={}",
                if name.templates.is_empty() {
                    "<note>".to_string()
                } else {
                    name.templates
                        .iter()
                        .map(|t| t.pattern.as_str())
                        .collect::<Vec<_>>()
                        .join(" | ")
                }
            ),
            GroupKind::Metric {
                name,
                instrument,
                unit,
            } => format!(" name={name} {instrument:?} unit={unit}"),
            GroupKind::Event { event_name } => format!(" event_name={event_name:?}"),
            GroupKind::Entity | GroupKind::AttributeGroup => String::new(),
        };
        writeln!(
            out,
            "{} {}{refines}{extra} level={:?} stability={:?}{}",
            g.kind.tag().noun(),
            g.id,
            g.requirement_level,
            g.stability,
            if g.deprecated.is_some() {
                " deprecated"
            } else {
                ""
            }
        )
        .unwrap();
        for a in &g.attributes {
            let req = match &a.requirement {
                Requirement::Required => "required".to_string(),
                Requirement::ConditionallyRequired(c) => format!("conditionally_required({c})"),
                Requirement::Recommended(None) => "recommended".into(),
                Requirement::Recommended(Some(r)) => format!("recommended({r})"),
                Requirement::OptIn(None) => "opt_in".into(),
                Requirement::OptIn(Some(r)) => format!("opt_in({r})"),
            };
            let role = match a.role {
                Role::Member => "",
                Role::Identity => " identity",
                Role::Description => " description",
            };
            let ann = if a.annotations.is_empty() {
                String::new()
            } else {
                format!(" annotations={:?}", a.annotations.as_map())
            };
            writeln!(
                out,
                "    {} {req}{}{role}{ann} brief={:?}",
                a.key,
                if a.sampling_relevant {
                    " sampling_relevant"
                } else {
                    ""
                },
                a.brief
            )
            .unwrap();
        }
    }
    out
}

#[test]
fn fixture_effective_view() {
    insta::assert_snapshot!(render(&fixture_registry()));
}

#[test]
fn fixture_specifics() {
    let r = fixture_registry();

    // Base signals repeated in `refinements` aren't duplicated.
    assert_eq!(
        r.groups_of(GroupKindTag::Span)
            .filter(|g| g.id == "db.client")
            .count(),
        1
    );

    // Weaver drops the definition's annotations on a reference override; the
    // view restores them per key.
    let span = r.group(GroupKindTag::Span, "http.server.request").unwrap();
    let user = span.attribute("user.id").unwrap();
    assert_eq!(
        user.annotations.cardinality(),
        Ok(Some(Cardinality::Bounded))
    );
    assert_eq!(user.annotations.sensitivity(), Ok(Some(Sensitivity::Pii)));

    // Inline `ref` beats the `ref_group` entry for the same key.
    let method = span.attribute("http.request.method").unwrap();
    assert!(method.sampling_relevant);
    assert_eq!(method.requirement, Requirement::Required);

    // Refinement override of an inherited attribute.
    let hbase = r.group(GroupKindTag::Span, "db.hbase.client").unwrap();
    assert_eq!(hbase.refines, Refines::Base("db.client".into()));
    assert_eq!(
        hbase.attribute("db.namespace").unwrap().brief,
        "The HBase namespace."
    );
    assert_eq!(
        r.group(GroupKindTag::Span, "db.client")
            .unwrap()
            .attribute("db.namespace")
            .unwrap()
            .brief,
        "The name of the database."
    );

    // Entity identity attributes are required whatever the file says.
    let service = r.group(GroupKindTag::Entity, "service").unwrap();
    assert!(
        service
            .attributes
            .iter()
            .filter(|a| a.role == Role::Identity)
            .all(|a| a.requirement == Requirement::Required)
    );

    // Event refinements: Weaver (as of 0.28) doesn't record the base event.
    let web = r.group(GroupKindTag::Event, "session.start.web").unwrap();
    assert_eq!(web.refines, Refines::Unknown);

    // Enum value types, including defaults.
    let ValueType::Enum(hit) = &r.attribute("demo.cache.hit").unwrap().ty else {
        panic!("enum")
    };
    assert_eq!(hit.value_type, semlex_schema::Primitive::Boolean);
    assert!(matches!(
        r.attribute("demo.label").unwrap().ty,
        ValueType::Template(_)
    ));
}

#[test]
fn fixture_names() {
    let r = fixture_registry();
    let n = names::names(&r).expect("fixture names resolve");
    let a = |k: &str| &n.attributes[k];
    assert_eq!(a("http.request.method").module, ["http", "request"]);
    assert_eq!(a("http.request.method").leaf, "method");
    assert_eq!(a("http.request.method").marker, "http_request_method");
    assert_eq!(a("demo.type").leaf, "r#type");
    assert_eq!(a("demo.match").leaf, "matched"); // code_generation.rust.ident
    // `db.operation` is a const; `db.operation.name` lives in module `db::operation`.
    assert_eq!(a("db.operation").module, ["db"]);
    assert_eq!(a("db.operation.name").module, ["db", "operation"]);
    let tier = a("demo.tier").enumeration.as_ref().unwrap();
    assert_eq!(tier.name, "ServiceTier"); // code_generation.rust.enum_name
    assert_eq!(tier.variants, ["Free", "Pro", "Legacy", "Internal"]);
    assert_eq!(
        a("http.request.method").enumeration.as_ref().unwrap().name,
        "HttpRequestMethod"
    );
}

#[test]
fn otel_semconv_loads_and_names_resolve() {
    let r = semlex_schema::load(fixture("otel-semconv/resolved.yaml")).expect("OTel semconv loads");
    assert_eq!(r.attributes.len(), 940);
    let n = names::names(&r).unwrap_or_else(|errs| {
        panic!(
            "{}",
            errs.iter()
                .map(|e| e.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        )
    });
    // The three rename pairs whose simple marker names clash.
    assert_eq!(
        n.attributes["messaging.client_id"].marker,
        "messaging__client_id"
    );
    assert_eq!(
        n.attributes["messaging.client.id"].marker,
        "messaging__client__id"
    );
    // Old and new member ids of one enum (`azure.aks` / `azure_aks`).
    let platform = n.attributes["cloud.platform"].enumeration.as_ref().unwrap();
    assert!(
        platform.variants.contains(&"Azure_Aks".to_string())
            && platform.variants.contains(&"AzureAks".to_string())
    );
    // Every built-in annotation in OTel semconv has a valid shape.
    for a in &r.attributes {
        a.annotations.code_generation_exclude().unwrap();
        a.annotations.rust_ident().unwrap();
    }
    for g in &r.groups {
        g.annotations.metric_value_type().unwrap();
    }
}

#[test]
fn wrong_file_format_is_rejected_by_name() {
    let text = std::fs::read_to_string(fixture("resolved/resolved.yaml"))
        .unwrap()
        .replacen("resolved/2.0", "resolved/3.0", 1);
    let e = semlex_schema::parse(&text).unwrap_err();
    assert!(
        matches!(e, Error::UnsupportedFormat { found: Some(ref f) } if f == "resolved/3.0"),
        "{e}"
    );
    assert!(e.to_string().contains("weaver registry package --v2"));
}

#[test]
fn unknown_fields_are_tolerated() {
    let text = std::fs::read_to_string(fixture("resolved/resolved.yaml"))
        .unwrap()
        .replacen("schema_url:", "future_top_level: 1\nschema_url:", 1)
        .replacen(
            "- key: container.id\n",
            "- key: container.id\n  future_field: x\n",
            1,
        );
    semlex_schema::parse(&text).expect("unknown fields are ignored");
}

/// Weaver writes YAML 1.2, leaving `on`, `off`, `yes`, `no` unquoted: they
/// are strings, not YAML 1.1 booleans (an enum like `channel.required_state`).
#[test]
fn yaml_1_1_boolean_words_are_strings() {
    let entry = "- key: acme.device.power\n  type:\n    members:\n    - id: on\n      value: ON\n      stability: stable\n    - id: off\n      value: OFF\n      stability: stable\n    - id: yes\n      value: Yes\n      stability: stable\n  brief: The power state the operator asked for.\n  stability: stable\n  examples:\n  - ON\nregistry:\n";
    let text = std::fs::read_to_string(fixture("resolved/resolved.yaml"))
        .unwrap()
        .replacen("registry:\n", entry, 1);
    let model = semlex_schema::parse_model(&text).expect("parses");
    let power = model
        .attribute_catalog
        .iter()
        .find(|a| a.key == "acme.device.power")
        .expect("in the catalog");
    let semlex_schema::model::AttributeType::Enum { members } = &power.ty else {
        panic!("an enum: {:?}", power.ty);
    };
    let pairs: Vec<_> = members
        .iter()
        .map(|m| (m.id.as_str(), m.value.clone()))
        .collect();
    use semlex_schema::model::EnumValue::String as S;
    assert_eq!(
        pairs,
        [
            ("on", Some(S("ON".into()))),
            ("off", Some(S("OFF".into()))),
            ("yes", Some(S("Yes".into())))
        ]
    );
    semlex_schema::parse(&text).expect("the view builds");
}

#[test]
fn dangling_index_is_invalid() {
    let text = std::fs::read_to_string(fixture("resolved/resolved.yaml"))
        .unwrap()
        .replacen("    - base: 1\n", "    - base: 9999\n", 1);
    let e = semlex_schema::parse(&text).unwrap_err();
    assert!(
        e.to_string()
            .contains("attribute index 9999 is out of range"),
        "{e}"
    );
}

#[test]
fn missing_file_names_the_path() {
    let e = semlex_schema::load("does/not/exist.yaml").unwrap_err();
    assert!(
        e.to_string()
            .starts_with("can't read registry `does/not/exist.yaml`"),
        "{e}"
    );
}

/// A registry that `ref:`s attributes defined in a dependency: Weaver lists
/// only the registry's own attributes in `registry.attributes`, so the
/// dependency's definitions come from their catalog entries.
#[test]
fn dependency_attributes_come_from_the_catalog() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/with-dependency/resolved.yaml");
    let reg = semlex_schema::load(&path).expect("loads");
    let error_type = reg
        .attribute("error.type")
        .expect("defined from the catalog");
    assert_eq!(error_type.provenance, Some(0), "from the dependency");
    assert!(matches!(error_type.ty, semlex_schema::ValueType::Enum(_)));
    // Only an overriding reference exists: its entry still carries the
    // definition's annotations.
    let address = reg
        .attribute("server.address")
        .expect("defined from the catalog");
    assert_eq!(
        address.annotations.cardinality().unwrap(),
        Some(semlex_schema::annotations::Cardinality::Unbounded)
    );
    let span = reg
        .group(semlex_schema::GroupKindTag::Span, "app.job.run")
        .expect("span");
    assert!(span.attribute("error.type").is_some());
    assert_eq!(
        span.attribute("server.address").unwrap().brief,
        "The job server."
    );
}

#[test]
fn dependency_attributes_keep_pii_when_a_reference_drops_it() {
    // Two catalog entries for one dependency attribute, used equally: the
    // first (lower index) wins as the definition, but the other's
    // `sensitivity: pii` must not be lost.
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/with-dependency/resolved.yaml");
    let text = std::fs::read_to_string(path).expect("fixture");
    let entries = "- key: user.email\n  type: string\n  brief: Overridden.\n  stability: stable\n  provenance:\n    source: 0\n- key: user.email\n  type: string\n  brief: The user's email.\n  stability: stable\n  annotations:\n    sensitivity: pii\n  provenance:\n    source: 0\nregistry:\n";
    let text = text.replacen("registry:\n", entries, 1);
    let mut uses = ["    - base: 3\n", "    - base: 4\n"].into_iter();
    let text = text
        .split("    requirement_level: recommended\n    brief: Running a job.\n")
        .enumerate()
        .map(|(i, part)| {
            if i == 0 {
                part.to_string()
            } else {
                let base = uses.next().unwrap_or_default();
                format!("{base}      requirement_level: recommended\n    requirement_level: recommended\n    brief: Running a job.\n{part}")
            }
        })
        .collect::<String>();
    let reg = semlex_schema::parse(&text).expect("parses");
    let email = reg
        .attribute("user.email")
        .expect("defined from the catalog");
    assert_eq!(email.brief, "Overridden.", "the lower index wins the tie");
    assert_eq!(
        email.annotations.sensitivity().unwrap(),
        Some(semlex_schema::annotations::Sensitivity::Pii)
    );
}
