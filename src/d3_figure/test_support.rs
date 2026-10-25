//! Offline stand-ins for d3 and linkedom, shared by the jsx.rs and draw.rs tests.
//! They exercise tauler's plumbing — resolving, loading, handing a selection to
//! `draw`, serializing the `<svg>` — not d3. Real files are covered only by the
//! `#[ignore]`d tests that download them.

use super::assets::{pinned_to, Asset};
use super::fetch::Fetcher;
use std::path::Path;
use std::sync::Arc;

/// A fake d3: shaped like d3's UMD header. Defines `scaleLinear` (doubles its
/// argument) and `select(node)`, whose selection supports `append`, `attr`,
/// `node`, `remove` and `call` — enough for the draw runtime to build an `<svg>`
/// and for a `draw` to add to it. The loader wraps it with the real 578-name
/// export list (both names are in it); every other name exports `undefined`.
pub const FAKE_D3_UMD: &str = r#"!function(t,n){"object"==typeof exports&&"undefined"!=typeof module?n(exports):"function"==typeof define&&define.amd?define(["exports"],n):n((t="undefined"!=typeof globalThis?globalThis:t||self).d3=t.d3||{})}(this,(function(t){t.scaleLinear=x=>2*x;function S(n){this.n=n}S.prototype.append=function(g){var c=this.n.ownerDocument.createElement(g);this.n.appendChild(c);return new S(c)};S.prototype.attr=function(k,v){this.n.setAttribute(k,v);return this};S.prototype.node=function(){return this.n};S.prototype.remove=function(){if(this.n.parentNode)this.n.parentNode.removeChild(this.n);return this};S.prototype.call=function(f){f(this);return this};t.select=n=>new S(n)}));"#;

/// A fake linkedom: an ES module, like linkedom's `worker.js`, exporting
/// `parseHTML(html)` → `{ document }`. The document has a `body` and creates
/// elements that keep attributes in order and serialize through `outerHTML`.
pub const FAKE_LINKEDOM: &str = r##"class Element {
  constructor(ownerDocument, tagName) {
    this.ownerDocument = ownerDocument;
    this.tagName = tagName;
    this.attributes = [];
    this.childNodes = [];
    this.parentNode = null;
  }
  setAttribute(name, value) {
    const found = this.attributes.find((a) => a[0] === name);
    if (found) found[1] = String(value);
    else this.attributes.push([name, String(value)]);
  }
  appendChild(child) {
    child.parentNode = this;
    this.childNodes.push(child);
    return child;
  }
  removeChild(child) {
    this.childNodes = this.childNodes.filter((c) => c !== child);
    child.parentNode = null;
    return child;
  }
  get outerHTML() {
    const attrs = this.attributes.map(([k, v]) => ` ${k}="${v}"`).join("");
    const inner = this.childNodes.map((c) => c.outerHTML).join("");
    return `<${this.tagName}${attrs}>${inner}</${this.tagName}>`;
  }
}
export function parseHTML(_html) {
  const document = {
    createElement: (tag) => new Element(document, tag),
    createElementNS: (_ns, tag) => new Element(document, tag),
  };
  document.body = new Element(document, "body");
  return { document };
}
"##;

/// Writes [`FAKE_D3_UMD`] and [`FAKE_LINKEDOM`] into `dir` under the names a
/// Fetcher over [`fake_d3_assets`] looks for.
pub fn write_fake_files(dir: &Path) {
    std::fs::create_dir_all(dir).expect("create d3 dir");
    std::fs::write(dir.join("d3.min.js"), FAKE_D3_UMD).expect("write d3");
    std::fs::write(dir.join("linkedom.js"), FAKE_LINKEDOM).expect("write linkedom");
}

/// An offline Fetcher over `dir` whose pins match [`FAKE_D3_UMD`] and
/// [`FAKE_LINKEDOM`]. It reads `dir` on every call, so files written later are
/// found.
pub fn fake_d3_fetcher(dir: &Path) -> Fetcher {
    Fetcher::build(dir.to_path_buf(), fake_d3_assets(), None)
}

/// `[d3, linkedom]` assets pinned to [`FAKE_D3_UMD`] and [`FAKE_LINKEDOM`].
pub fn fake_d3_assets() -> [&'static Asset; 2] {
    let pinned = |file_name, content| {
        pinned_to(
            file_name,
            "https://example.invalid/never-downloaded",
            None,
            content,
        )
    };
    [
        pinned("d3.min.js", FAKE_D3_UMD),
        pinned("linkedom.js", FAKE_LINKEDOM),
    ]
}

/// A PackageContext with no Packages (empty Lockfile path, empty cache) and
/// `fetcher` for `"d3"`.
pub fn d3_only_pkg_ctx(dir: &Path, fetcher: Fetcher) -> crate::pkg::PackageContext {
    let (reload_tx, _reload_rx) = std::sync::mpsc::channel();
    crate::pkg::PackageContext {
        fetch_manager: Arc::new(crate::pkg::fetch_manager::FetchManager::new(reload_tx)),
        lockfile_path: dir.join("tauler-pkg.lock"),
        cache_root: dir.join("cache"),
        d3: Arc::new(fetcher),
    }
}
