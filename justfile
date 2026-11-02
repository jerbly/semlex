# semlex development recipes. Run `just` to list them.

# Pinned tool versions. CI installs exactly these; local recipes check them.
weaver_version := "0.28.0"
otel_semconv_version := "1.44.0"

# Placeholder publication URI for the fixture. `weaver registry package` requires
# one, but it is only recorded in the publication manifest.
fixture_uri := "https://semlex.example/schemas/0.1.0/resolved.yaml"

_default:
    @just --list

# Fail unless the installed Weaver matches the pinned version.
weaver-version:
    #!/usr/bin/env sh
    set -eu
    actual="$(weaver --version | awk '{print $2}')"
    if [ "$actual" != "{{weaver_version}}" ]; then
        echo "weaver $actual is installed; semlex pins {{weaver_version}}" >&2
        exit 1
    fi

# Check the fixture registry, then package it into fixtures/resolved/.
registry: weaver-version
    weaver registry check --v2 -r fixtures/registry -p policies/registry
    weaver registry package --v2 -r fixtures/registry -o fixtures/resolved \
        --resolved-registry-uri {{fixture_uri}}
    # A registry referring to a dependency's attributes (relative paths
    # resolve from the app folder, which must be named, not `.`).
    cd crates/semlex-schema/tests/data/with-dependency/app && weaver registry package --v2 -r ../app -o .. \
        --resolved-registry-uri https://semlex.example/schemas/app/1.0.0/resolved.yaml

# Check each example's local registry (OTel semconv upstream), then package it into its crate.
example-registries: weaver-version
    #!/usr/bin/env sh
    set -eu
    for ex in simple showcase advanced; do
        weaver registry check --v2 -r examples/$ex/registry -p policies/registry --quiet
        weaver registry package --v2 -r examples/$ex/registry -o examples/$ex \
            --resolved-registry-uri https://semlex.example/schemas/$ex/1.0.0/resolved.yaml
    done

# Package upstream OTel semconv (pinned) into fixtures/otel-semconv/ for scale tests.
otel-semconv: weaver-version
    weaver registry package --v2 \
        -r 'https://github.com/open-telemetry/semantic-conventions.git@v{{otel_semconv_version}}[model]' \
        -o fixtures/otel-semconv \
        --resolved-registry-uri https://opentelemetry.io/schemas/{{otel_semconv_version}}/resolved.yaml

# Fetch Weaver's JSON schema for resolved/2.0 at the pinned version.
weaver-schema:
    curl -fsSL -o fixtures/schemas/semconv.resolved.v2.json \
        https://raw.githubusercontent.com/open-telemetry/weaver/v{{weaver_version}}/schemas/semconv.resolved.v2.json

# Re-package every fixture and re-fetch the schema; fail if anything committed differs.
fixtures-check: registry example-registries otel-semconv weaver-schema
    git diff --exit-code -- fixtures/resolved fixtures/otel-semconv fixtures/schemas crates/semlex-schema/tests/data/with-dependency/resolved.yaml crates/semlex-schema/tests/data/with-dependency/manifest.yaml examples/*/resolved.yaml examples/*/manifest.yaml

fmt:
    cargo fmt --all

lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo clippy -p semlex --no-default-features -- -D warnings

# fixture-semconv's UI tests check that non-stable items need `semlex-unstable`,
# so they run in their own cargo invocation: Cargo unifies features across a
# workspace build, and fixture-service enables the feature.
test:
    cargo test --workspace --exclude fixture-semconv
    cargo test -p fixture-semconv
    cargo test -p fixture-semconv --features semlex-unstable --test unstable

# Fail unless touching resolved.yaml rebuilds the crate that calls registry!().
rebuild-check:
    #!/usr/bin/env sh
    set -eu
    cargo build -q -p fixture-semconv
    touch fixtures/resolved/resolved.yaml
    if cargo build -v -p fixture-semconv 2>&1 | grep -q -- '--crate-name fixture_semconv'; then
        echo "ok: registry change rebuilt fixture-semconv"
    else
        echo "registry change did not rebuild fixture-semconv" >&2
        exit 1
    fi

# The registry policies: the fixture passes, and policies/tests/bad-annotations
# triggers every finding.
policy-check: weaver-version
    #!/usr/bin/env sh
    set -eu
    weaver registry check --v2 -r fixtures/registry -p policies/registry --quiet
    out="$(weaver registry check --v2 -r policies/tests/bad-annotations -p policies/registry --diagnostic-format json 2>&1 || true)"
    for id in invalid_cardinality invalid_sensitivity invalid_code_generation unbounded_metric_attribute; do
        echo "$out" | grep -q "\"id\": \"$id\"" || { echo "policy finding $id not reported" >&2; exit 1; }
    done
    n="$(echo "$out" | grep -o '"type": "PolicyFinding"' | wc -l | tr -d ' ')"
    [ "$n" -eq 7 ] || { echo "expected 7 policy findings, got $n" >&2; exit 1; }
    echo "ok: 7 findings on bad-annotations, none on the fixture"

# Markdown report of escape-hatch uses (`unregistered(…)`, `otel_mut`,
# `into_otel`, `semlex-unstable`, approvals) for review.
escape-report *paths:
    sh scripts/escape-report.sh {{paths}}

# The report finds fixture-service's deliberate escape hatch, and clippy rejects
# an unapproved `unregistered(…)` (policies/tests/unregistered-denied).
unregistered-policy-check:
    #!/usr/bin/env sh
    set -eu
    sh scripts/escape-report.sh | grep -q 'tests/fixture-service/src/lib.rs.*unregistered("demo.retry.attempt")' \
        || { echo "escape report missed fixture-service's unregistered(…) use" >&2; exit 1; }
    if out="$(CARGO_TARGET_DIR=target cargo clippy -q --manifest-path policies/tests/unregistered-denied/Cargo.toml -- -D warnings 2>&1)"; then
        echo "clippy accepted an unapproved unregistered(…)" >&2; exit 1
    fi
    echo "$out" | grep -q 'use of a disallowed method `semlex::__private::unregistered_attribute`' \
        || { echo "unexpected clippy output:" >&2; echo "$out" >&2; exit 1; }
    echo "$out" | grep -q 'use of a disallowed method `semlex::__private::extra_attribute`' \
        || { echo "clippy accepted an unapproved extra(…):" >&2; echo "$out" >&2; exit 1; }
    echo "ok: report lists the approved use; clippy rejects unapproved unregistered(…) and extra(…)"

# Re-check cost of a registry crate after a registry change. The default is
# the full-OTel stress test (a regression marker at <= 2 s and <= 700 MB, not
# a budget real local registries need). Median of three runs.
# usage: just measure [package] [resolved.yaml]
measure package="otel-stress" registry="fixtures/otel-semconv/resolved.yaml":
    #!/usr/bin/env sh
    set -eu
    cargo check -q -p {{package}}
    for i in 1 2 3; do
        touch {{registry}}
        /usr/bin/time -l cargo check -q -p {{package}} 2>&1 \
            | awk '/ real /{t=$1} /maximum resident set size/{m=$1/1048576} END{printf "%.2f s  %.0f MB\n", t, m}'
    done | sort -n | sed -n 2p | sed 's/^/median: /'

# Package and build the four publishable crates as a registry would; uploads nothing.
publish-check:
    cargo publish --workspace --dry-run

# Build on the minimum supported Rust version.
msrv:
    cargo +1.89 check --workspace --all-targets

# Everything CI runs.
ci: lint test msrv rebuild-check policy-check unregistered-policy-check fixtures-check
