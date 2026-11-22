#!/usr/bin/env bash
# Instructions + allocations per pipeline stage; see the module docs of benches/cost.rs.
set -euo pipefail
command -v valgrind >/dev/null || { echo "valgrind is required (apt install valgrind)"; exit 1; }
bin=$(cargo bench --bench cost --no-run 2>&1 | sed -n 's/.*Executable .*(\(.*\))/\1/p' | tail -1)
printf '%-18s %14s %10s %14s %12s\n' stage Ir allocs bytes peak
for stage in eval parse rasterize cache_hit eval_to_pixels store_read_none store_read_all; do
    err=$(mktemp)
    out=$(valgrind --tool=callgrind --callgrind-out-file=/dev/null \
        --toggle-collect=tauler_cost_measured "$bin" "$stage" 2>"$err")
    ir=$(sed -n 's/.*Collected : \([0-9]*\).*/\1/p' "$err")
    rm -f "$err"
    read -r _ allocs bytes peak <<<"$out"
    printf '%-18s %14s %10s %14s %12s\n' "$stage" "$ir" "$allocs" "$bytes" "$peak"
done
