# d3 and linkedom are fetched on first use, not vendored

Neither d3 nor linkedom is in the repository or the binary. The first time a layout needs
them — an `import "d3"`, or a D3Figure's first draw — tauler downloads the projects' own
published files, checks each against a sha256 pinned in the source, and caches them under
`~/.cache/tauler/d3`. Every later load checks the hash again. `TAULER_D3_DIR` points at
local copies instead, for machines with no network.

The download rides the Package path (ADR
[0041](0041-packages-plug-into-the-existing-resolver-and-reload-path.md)): a cold `"d3"`
fails to resolve exactly like a cold `@gh/` import, the fetch runs off the loop, and its
completion triggers the ordinary reload. A D3Figure whose files are not here yet draws
nothing, which is already its "not drawn yet" state.

## Which files

The most official build each project publishes at an exact version:

- d3 7.9.0: `dist/d3.min.js` from npm's tarball. d3 publishes built files nowhere else —
  its GitHub releases carry none and `dist/` is not committed. d3js.org serves the same
  bytes as `d3.v7.min.js`, but that URL follows the latest 7.x, so a pin would break on d3's
  next release.
- linkedom 0.18.14: `worker.js` from its GitHub repository at the commit behind the tag (a
  tag can move; a commit cannot), byte-identical to its npm copy.

linkedom is there because idiomatic d3 — `select`, `append`, data joins, axes — needs a
DOM, and QuickJS has none. It is used for nothing else, and only inside the Draw runtime.
The alternative, d3 for the maths and JSX for the elements, needs no DOM, but loses
`d3-axis` and copy-paste from d3's own documentation, and needs a JSX-to-SVG serializer.

## Why not vendor

- **In the repository**, the bundle is 0.6 MB of third-party JavaScript to review on every
  bump, and the repository has none today.
- **In the binary**, the same 0.6 MB ships to every user whether or not their layout draws.
- **Built by `build.rs`**, every `cargo build` and `cargo install` would need node, npm and
  the network.
- **As a release asset of our own**, someone has to build, host and re-sign it.

Fetching costs one download per machine and a first-use delay, and it means tauler runs
code it downloaded. The pinned hash is what makes that acceptable: a changed upstream file,
or a changed cached file, is refused like a failed download.

## Licences and terms

Both libraries are ISC; what their files bundle is ISC, MIT, BSD-2-Clause, BSD-3-Clause
(d3-ease) and Unlicense — all permissive. tauler redistributes neither, so it owes no
notices. npm's terms allow software to download through its public APIs short of "an
unreasonable volume", and GitHub's forbid "excessive automated bulk activity"; one cached
download per machine is neither. Downloads are HTTPS only and identify themselves as tauler.

## Security

What runs is the pinned files and nothing else, inside a Draw runtime that has no `fetch`,
no filesystem and no shell, and that interrupts a `draw` running past its time limit. So
whatever d3 or linkedom code tries, it cannot reach anything outside that runtime.

Checked when the pins were set (2026-10-24): GitHub's advisory database lists nothing for
linkedom, d3, or what linkedom bundles (htmlparser2, entities, domutils, css-select,
dom-serializer); nth-check's advisory covers versions below 2.0.1, and linkedom bundles
3.0.1. Neither file reaches the network or runs strings as code by itself: linkedom's
`fetch(` is a local helper, its one `eval` maps error names like `"TypeError"` to
constructors, and its `require("cssstyle")` sits in a try/catch; d3's `fetch` calls are
d3-fetch (`d3.json`, `d3.csv`), which throw where there is no global `fetch`, and its one
`new Function` is d3-dsv building a row object from JSON-escaped column names. Bumping a
pin means repeating this check.

## The docs site

The site takes d3 from npm like any other site asset (ADR
[0032](0032-site-assets-come-from-npm-never-vendored.md)): `docs/package.json` pins it and
the build copies `d3.min.js` next to the web runtime. The page needs no linkedom — it has a
DOM.
