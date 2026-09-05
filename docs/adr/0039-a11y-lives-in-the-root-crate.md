# a11y lives in the root crate, not tauler-core

The accessibility walk needs takumi geometry — the same layout read-back
`hit_test` uses (`src/hit_test.rs:183`) — and `tauler-core` is the
**wasm-clean** subset that must never gain a takumi/accesskit dependency
(its own `Cargo.toml` promises wasm builds a pure tree). So a11y
cannot live there. A separate `tauler-a11y` crate would force the geometry
walk into it (duplicating `hit_test`) or a root↔a11y dependency cycle..
`tauler-web` exists for the wasm boundary, which is not a11y's boundary; the
root crate already houses the other Linux render-pipeline consumers
(`hit_test`, `pointer`).

## Consequence

`src/a11y/` sits beside `src/hit_test.rs` and `src/pointer.rs` as a sibling
Linux-gated module, reusing their walk and dispatch directly. It gains
`accesskit` + `accesskit_unix` in the existing
`[target.'cfg(target_os = "linux")'.dependencies]` block alongside smithay
and x11rb — no feature flag, since `accesskit_unix`'s `update_if_active`
makes an idle a11y tree cost ~zero.