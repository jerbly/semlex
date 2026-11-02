# Fixtures

| Path | What | Regenerate with |
|---|---|---|
| `registry/` | Hand-written V2 registry (`definition/2`) covering every feature semlex consumes | edit by hand |
| `resolved/` | `registry/` packaged by the pinned Weaver (`resolved/2.0` + publication manifest) | `just registry` |
| `otel-semconv/` | Upstream OpenTelemetry semconv, packaged at the pinned version, for scale tests | `just otel-semconv` |
| `schemas/` | Weaver's JSON schema for `resolved/2.0` at the pinned version (conformance tests) | `just weaver-schema` |

Packaged output is committed. CI re-packages with the pinned Weaver and fails on
any diff (`just fixtures-check`), so a Weaver bump shows up as a reviewable
fixture diff.

The fixture registry uses OTel-style names (`http.*`, `db.*`, `session.*`, …) so
tests read like real instrumentation, but it defines every attribute
itself: packaging needs no dependency or network.

## Coverage

`registry/` deliberately includes:

- attribute types: every primitive and array type, `any`, `template[string]`, `template[string[]]`;
- string, int and boolean enums, with member-level stability, deprecation and `code_generation.exclude`;
- all five stability levels and the `renamed`, `obsoleted` and `uncategorized` deprecation reasons, including a stable attribute that was renamed (`http.method`);
- name-mangling cases: a Rust keyword segment (`demo.type`), a key that is a prefix of another (`db.operation` / `db.operation.name`), `code_generation.rust.ident` and `.enum_name`; a `code_generation.rust.type` override (`demo.timeout`: `Duration`);
- spans with `sampling_relevant`, `name.templates` (with literal fallback) and `name.note` only;
- span, metric, event and entity refinements, including attribute overrides that produce several catalog entries for one key;
- `ref_group` with an inline `ref` overriding the group's entry;
- public and internal attribute groups (internal ones don't survive resolution);
- metrics of all four instrument kinds, with and without `code_generation.metric_value_type`;
- events, including one with an `any` attribute; entities with identity and description;
- signal `requirement_level: opt_in`, a deprecated span and a deprecated public group;
- requirement-level objects (`conditionally_required`, `recommended` with a reason);
- `entity_associations` with nested `all_of` / `one_of`;
- annotations at definition, reference and enum-member level: `code_generation.*`, `cardinality`, `sensitivity`, and a user namespace (`demo.case`), including a reference that overrides one of two definition annotations (`user.id` on `http.server.request`).

## Weaver behaviour semlex handles

How Weaver's resolved output differs from what a reader might expect, and
what `semlex-schema` does about it:

- Refinement ids lose their signal prefix: `span.db.hbase.client` resolves to
  id `db.hbase.client`.
- `refinements.spans` (and the other refinement lists) also contain every base
  signal as a refinement of itself, with `id` equal to its type.
- A reference that overrides `brief`, `note` or `annotations` produces a second
  catalog entry for the same key (`url.path`, `db.namespace`).
- Span-name templates arrive pre-parsed into `parts` (literal or attribute),
  with their attribute list.
- Entity `identity` entries are plain catalog indices (`- 25`) in 0.28, and
  `{base, requirement_level}` references in 0.27; both parse.
- In upstream OTel semconv 1.44, 9 keys are a prefix of another key
  (`db.operation` / `db.operation.name`), 3 pairs clash once dots become `_`
  (`messaging.client_id` / `messaging.client.id`), and `cloud.platform` has
  member pairs like `azure.aks` / `azure_aks`. All resolve under the naming
  rules in `semlex_schema::names`.

Open upstream issues, each with a workaround in semlex. Remove a workaround
once the fix ships in the pinned Weaver. The fixture registry reproduces the
first three (`just registry`, then read `fixtures/resolved/resolved.yaml`).

| Upstream | Problem | semlex |
|---|---|---|
| [open-telemetry/weaver#1705](https://github.com/open-telemetry/weaver/issues/1705) | A reference's `annotations` replace the definition's whole map instead of merging (`user.id`'s `sensitivity: pii` would be lost) | Deep-merges definition then reference (`Annotations::merged`) |
| [open-telemetry/weaver#1836](https://github.com/open-telemetry/weaver/issues/1836) | Event refinements resolve with their own id as `name`, not the base event's (`session.start.web`) | Event refinements are a compile error that names the issue |
| [open-telemetry/weaver#1837](https://github.com/open-telemetry/weaver/issues/1837) | Refinements don't inherit `entity_associations` | A refinement with none gets its base's |
| [open-telemetry/weaver#1838](https://github.com/open-telemetry/weaver/issues/1838) | `--registry .` loads no definition files, without an error | Recipes name the folder (`-r ../app`) |
