#!/usr/bin/env bash
# Rebuild this example's gitignored .data/: the City geometry for every place in
# places.tsv. Places already fetched are skipped; delete a file to fetch it
# again. Fetches run one at a time, a few seconds apart — both OSM services ask
# for that. d3 itself is not here — tauler downloads and pins it.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
grep -v '^#' "$here/places.tsv" | while IFS=$'\t' read -r name lat lon radius; do
  [[ -s $here/.data/$name.js ]] && continue
  "$here/fetch-city.sh" "$name" "$lat" "$lon" "$radius"
  sleep 3
done
