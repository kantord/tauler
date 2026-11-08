# taulerbox

**Experimental.** A native window (Linux and macOS) that shows a
[tauler](https://crates.io/crates/tauler) layout's Panels as pixel buffers, drawn alongside
a microVM compartment running a nested desktop. It is a prototype for
[#582](https://github.com/kantord/tauler/issues/582), is not built or tested in CI, and
nothing in it is stable.

Where native Tauler creates one desktop window per Panel, taulerbox creates one window and
draws every Panel into it at its declared position. Sandboxing exists in taulerbox only.

Documentation: <https://kantord.github.io/tauler/docs/develop/taulerbox/>
