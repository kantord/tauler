#!/usr/bin/env bash
# Rebuild this example's gitignored .data/: crates.js, the dependency graph the
# network chart draws. d3 itself is not here — tauler downloads and pins it.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
"$here/gen-crates.sh"
