# Shapes of the annotations semlex interprets, checked by
# `weaver registry check --v2 -p policies/registry` so mistakes surface in the
# registry PR rather than in a later service build.
#
# Also bans `cardinality: unbounded` attributes on metrics: `registry!()`
# rejects their use at compile time; this rejects them at registry review.
package after_resolution

import rego.v1

signal_kinds := ["spans", "metrics", "events"]

signal_name(s) := object.get(s, "id", object.get(s, "type", object.get(s, "name", "?")))

# Every annotation map, with where it is.
annotated contains [where, ann] if {
	a := input.registry.attributes[_]
	ann := a.annotations
	where := sprintf("attribute `%s`", [a.key])
}

annotated contains [where, ann] if {
	a := input.registry.attributes[_]
	m := a.type.members[_]
	ann := m.annotations
	where := sprintf("member `%s` of attribute `%s`", [m.id, a.key])
}

annotated contains [where, ann] if {
	some section in ["registry", "refinements"]
	some kind in signal_kinds
	s := input[section][kind][_]
	ann := s.annotations
	where := sprintf("%s `%s`", [trim_suffix(kind, "s"), signal_name(s)])
}

annotated contains [where, ann] if {
	some section in ["registry", "refinements"]
	some kind in signal_kinds
	s := input[section][kind][_]
	r := s.attributes[_]
	ann := r.annotations
	where := sprintf("attribute `%s` on %s `%s`", [r.key, trim_suffix(kind, "s"), signal_name(s)])
}

annotated contains [where, ann] if {
	some section in ["registry", "refinements"]
	e := input[section].entities[_]
	some part in ["identity", "description"]
	r := e[part][_]
	ann := r.annotations
	where := sprintf("attribute `%s` on entity `%s`", [r.key, signal_name(e)])
}

annotated contains [where, ann] if {
	g := input.registry.attribute_groups[_]
	r := g.attributes[_]
	ann := r.annotations
	where := sprintf("attribute `%s` in group `%s`", [r.key, g.id])
}

finding(id, msg) := {"id": id, "message": msg, "level": "violation"}

deny contains finding("invalid_cardinality", msg) if {
	[where, ann] := annotated[_]
	v := ann.cardinality
	not v in {"bounded", "unbounded"}
	msg := sprintf("%s: `cardinality` must be `bounded` or `unbounded`, found `%v`", [where, v])
}

deny contains finding("invalid_sensitivity", msg) if {
	[where, ann] := annotated[_]
	v := ann.sensitivity
	v != "pii"
	msg := sprintf("%s: `sensitivity` must be `pii`, found `%v`", [where, v])
}

deny contains finding("invalid_code_generation", msg) if {
	[where, ann] := annotated[_]
	v := ann.code_generation.exclude
	not is_boolean(v)
	msg := sprintf("%s: `code_generation.exclude` must be `true` or `false`, found `%v`", [where, v])
}

deny contains finding("invalid_code_generation", msg) if {
	[where, ann] := annotated[_]
	v := ann.code_generation.metric_value_type
	not v in {"int", "double"}
	msg := sprintf("%s: `code_generation.metric_value_type` must be `int` or `double`, found `%v`", [where, v])
}

deny contains finding("invalid_code_generation", msg) if {
	[where, ann] := annotated[_]
	some key in ["type", "ident", "enum_name"]
	v := ann.code_generation.rust[key]
	not is_string(v)
	msg := sprintf("%s: `code_generation.rust.%s` must be a string, found `%v`", [where, key, v])
}

deny contains finding("invalid_code_generation", msg) if {
	[where, ann] := annotated[_]
	some key in ["ident", "enum_name"]
	v := ann.code_generation.rust[key]
	is_string(v)
	not regex.match(`^(r#)?[A-Za-z_][A-Za-z0-9_]*$`, v)
	msg := sprintf("%s: `code_generation.rust.%s` must be a Rust identifier, found `%v`", [where, key, v])
}

deny contains finding("unbounded_metric_attribute", msg) if {
	some section in ["registry", "refinements"]
	m := input[section].metrics[_]
	r := m.attributes[_]
	r.annotations.cardinality == "unbounded"
	msg := sprintf("attribute `%s` on metric `%s` is `cardinality: unbounded`; use a bounded attribute, or mark this reference `cardinality: bounded`", [r.key, signal_name(m)])
}
