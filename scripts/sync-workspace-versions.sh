#!/usr/bin/env bash
#
# Rewrite every intra-workspace path dependency to require the current
# `[workspace.package] version` -- except a dependency on a crate that opts out of
# workspace-version lockstep, which must never be forced back onto the shared value.
#
# release-plz rewrites a path dependency's `version = "..."` only for the crates it
# decides to release, but `version.workspace = true` moves every member of the
# workspace regardless. The requirement therefore goes stale in any cycle where a
# sibling crate happens to have no commits of its own, and `cargo publish` then
# verifies against the sibling's *released* copy instead of the local one — see
# `tests/workspace_manifests_test.rs` for the times that broke CI.
#
# `tauler-ui-macro` deliberately does not use `version.workspace = true` (see the
# comment on its `version` field) specifically so it stops being swept into that bump
# on cycles where it has no commits of its own -- release-plz/release-plz#2878. A blind
# rewrite here would put it right back into lockstep by force, undoing that. Extend
# INDEPENDENTLY_VERSIONED_CRATES below if another crate opts out the same way.
#
# Usage: scripts/sync-workspace-versions.sh [repo-root]

set -euo pipefail

root=${1:-.}

# Crate names whose `version` this script must never rewrite to the shared workspace
# value, because their Cargo.toml deliberately versions them independently.
INDEPENDENTLY_VERSIONED_CRATES="tauler-ui-macro"

version=$(
    awk '
        /^\[workspace\.package\]/ { section = 1; next }
        /^\[/                     { section = 0 }
        section && /^version[[:space:]]*=/ {
            match($0, /"[^"]*"/)
            print substr($0, RSTART + 1, RLENGTH - 2)
            exit
        }
    ' "$root/Cargo.toml"
)

if [ -z "$version" ]; then
    echo "no version under [workspace.package] in $root/Cargo.toml" >&2
    exit 1
fi

skip_pattern=$(printf '%s' "$INDEPENDENTLY_VERSIONED_CRATES" | tr ' ' '|')

for manifest in "$root"/Cargo.toml "$root"/*/Cargo.toml; do
    [ -f "$manifest" ] || continue
    # `-i.bak` because BSD sed requires an argument to `-i`; GNU sed accepts it too.
    sed -i.bak -E \
        -e '/^[[:space:]]*#/b' \
        -e "/^($skip_pattern)[[:space:]]*=/b" \
        -e '/path[[:space:]]*=[[:space:]]*"/ s/version[[:space:]]*=[[:space:]]*"[^"]*"/version = "'"$version"'"/' \
        "$manifest"
    rm -f "$manifest.bak"
done
