#!/usr/bin/env sh
# Lists semlex escape hatches for review: `unregistered(…)`
# and `extra(…)` uses, raw OpenTelemetry access (`otel_mut`, `into_otel`), crates enabling
# non-stable registry items, and approvals (`#[allow(…disallowed_methods)]`
# or `reviewed: "…"` inside `extra(…)` / `unregistered(…)`). Prints a
# Markdown report; always exits 0 (it informs review; clippy enforces).
#
# usage: scripts/escape-report.sh [pathspec ...]   (default: the whole repository)
set -eu

# semlex's own sources, docs and test fixtures aren't application code.
excludes=':!crates/semlex* :!docs :!issues :!fixtures :!scripts :!policies/tests :!**/tests/ui/** :!*.md'

report() {
    # $1: label, $2: file name suffix (`.rs`, `Cargo.toml`), $3: extended regex, then pathspecs
    label=$1
    suffix=$2
    pattern=$3
    shift 3
    # shellcheck disable=SC2086
    git grep -n -I -E "$pattern" -- "${@:-.}" $excludes 2>/dev/null | while IFS=: read -r file line text; do
        case "$file" in *"$suffix") ;; *) continue ;; esac
        text=$(printf '%s' "$text" | sed -e 's/^[[:space:]]*//' -e 's/|/\\|/g')
        printf '| %s | `%s:%s` | `%s` |\n' "$label" "$file" "$line" "$text"
    done
}

paths=${*:-.}
# shellcheck disable=SC2086
rows=$(
    report 'unregistered attribute' .rs 'unregistered\(' $paths
    report 'attribute outside its signal (`extra`)' .rs '(^|[^a-z_])extra\(' $paths
    report 'raw OTel span (`otel_mut`)' .rs '\.otel_mut\(' $paths
    report 'left semlex (`into_otel`)' .rs '\.into_otel\(' $paths
    report 'non-stable items enabled' Cargo.toml '(^|[^-])features *= *\[[^]]*semlex-unstable' $paths
    report 'approval' .rs 'allow\([^)]*disallowed_methods' $paths
    report 'approval (in the macro)' .rs '(unregistered|extra)\([^:]*, *reviewed *:' $paths
)

echo '## semlex escape hatches'
echo
if [ -z "$rows" ]; then
    echo 'None.'
else
    echo "$(printf '%s\n' "$rows" | wc -l | tr -d ' ') found:"
    echo
    echo '| Kind | Where | Code |'
    echo '|---|---|---|'
    printf '%s\n' "$rows"
fi
