use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use clap::Parser;

mod web;

/// Generate the components page of the docs site from doc comments on UI
/// components, rendering each `# JSX` example to a screenshot alongside it.
#[derive(Parser)]
struct Args {
    /// Directory containing UI component source files
    #[arg(long, default_value = "tauler-core/src/ui/components")]
    components_dir: PathBuf,

    /// Path to write the generated MDX file
    #[arg(long, default_value = "docs/src/content/docs/docs/components.md")]
    output: PathBuf,

    /// Directory to write rendered component screenshots
    #[arg(long, default_value = "docs/src/assets")]
    assets_dir: PathBuf,

    /// Directory to write takumi's per-component box geometry, for the browser comparison
    #[arg(long, default_value = "docs/.tauler/geometry")]
    geometry_dir: PathBuf,

    /// Directory to write the browser assets the docs site serves
    #[arg(long, default_value = "docs/public/tauler")]
    web_dir: PathBuf,

    /// Directory to write the Tailwind inputs the docs build compiles
    #[arg(long, default_value = "docs/.tauler")]
    build_dir: PathBuf,
}

struct Component {
    module_path: String,
    export_name: String,
    prose: Vec<String>,
    jsx_block: Option<Vec<String>>,
    shadcn_url: Option<String>,
    #[cfg_attr(not(test), allow(dead_code))]
    skip_snapshot: bool,
    /// Registered for a JS shim to call, not for a layout to import. Excluded
    /// from the generated docs: naming it would invite an import that returns
    /// something other than what the matching JSX tag does.
    internal: bool,
    /// Shown as its screenshot rather than run in the browser. `<Icon>` is the one, and
    /// the reason is fonts: `append_symbol_fallback` resolves its Nerd Font through
    /// fontconfig, so the takumi side depends on the host and the two cannot be made to
    /// agree by serving a file (ADR 0028).
    skip_web: bool,
}

struct DocComments {
    prose: Vec<String>,
    jsx_block: Option<Vec<String>>,
    shadcn_url: Option<String>,
    skip_snapshot: bool,
    internal: bool,
    skip_web: bool,
}

fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect()
}

/// Collect all `///` doc-comment lines immediately before a given line index.
/// Returns them in source order (top-to-bottom).
fn collect_doc_comment_lines(lines: &[&str], attr_line_idx: usize) -> Vec<String> {
    let mut doc_lines: Vec<String> = Vec::new();
    let mut idx = attr_line_idx as isize - 1;
    while idx >= 0 {
        let line = lines[idx as usize].trim();
        if let Some(rest) = line.strip_prefix("///") {
            doc_lines.push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
            idx -= 1;
        } else {
            break;
        }
    }
    doc_lines.reverse();
    doc_lines
}

fn parse_fenced_code_block(raw: &[String], i: &mut usize) -> Option<Vec<String>> {
    if !raw.get(*i)?.trim_start().starts_with("```") {
        return None;
    }
    *i += 1;
    let mut block = Vec::new();
    while *i < raw.len() && !raw[*i].trim_start().starts_with("```") {
        block.push(raw[*i].clone());
        *i += 1;
    }
    if *i < raw.len() {
        *i += 1;
    }
    Some(block)
}

fn parse_doc_comments(raw: &[String]) -> DocComments {
    let mut prose = Vec::new();
    let mut jsx_block = None;
    let mut shadcn_url = None;
    let mut skip_snapshot = false;
    let mut internal = false;
    let mut skip_web = false;
    let mut i = 0;
    while i < raw.len() {
        if raw[i].trim() == "# JSX" {
            i += 1;
            jsx_block = parse_fenced_code_block(raw, &mut i);
        } else if raw[i].trim() == "# Shadcn" {
            i += 1;
            if i < raw.len() {
                shadcn_url = Some(raw[i].trim().to_string());
                i += 1;
            }
        } else if raw[i].trim() == "# SkipSnapshot" {
            skip_snapshot = true;
            i += 1;
        } else if raw[i].trim() == "# Internal" {
            internal = true;
            i += 1;
        } else if raw[i].trim() == "# SkipWeb" {
            skip_web = true;
            i += 1;
        } else {
            prose.push(raw[i].clone());
            i += 1;
        }
    }
    DocComments {
        prose,
        jsx_block,
        shadcn_url,
        skip_snapshot,
        internal,
        skip_web,
    }
}

fn component_module_path(line: &str) -> Option<String> {
    let inner = line
        .trim()
        .strip_prefix("#[component(\"")
        .and_then(|s| s.strip_suffix("\")]"))?;
    if inner.starts_with('@') {
        Some(inner.to_string())
    } else {
        None
    }
}

fn fn_name_after_attr(lines: &[&str], attr_idx: usize) -> Option<String> {
    let j = (attr_idx + 1..lines.len()).find(|&j| !lines[j].trim().is_empty())?;
    let fn_line = lines[j].trim();
    // Accept both `pub fn name` and `fn name`.
    let after_fn = fn_line
        .strip_prefix("pub fn ")
        .or_else(|| fn_line.strip_prefix("fn "))?;
    let name: String = after_fn
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn extract_components(path: &Path) -> Result<Vec<Component>, std::io::Error> {
    let source = fs::read_to_string(path)?;
    let lines: Vec<&str> = source.lines().collect();
    let mut components = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(module_path) = component_module_path(line) else {
            continue;
        };
        let Some(fn_name) = fn_name_after_attr(&lines, i) else {
            continue;
        };
        let doc = parse_doc_comments(&collect_doc_comment_lines(&lines, i));
        components.push(Component {
            module_path,
            export_name: to_pascal_case(&fn_name),
            prose: doc.prose,
            jsx_block: doc.jsx_block,
            shadcn_url: doc.shadcn_url,
            skip_snapshot: doc.skip_snapshot,
            internal: doc.internal,
            skip_web: doc.skip_web,
        });
    }
    Ok(components)
}

fn collect_rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return result,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            result.extend(collect_rs_files(&path));
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            result.push(path);
        }
    }
    result
}

fn component_rs_files(components_dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = collect_rs_files(components_dir)
        .into_iter()
        .filter(|p| {
            !p.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .contains("test")
        })
        .collect();
    files.sort();
    files
}

fn load_all_components(components_dir: &Path) -> Vec<Component> {
    let mut components: Vec<Component> = component_rs_files(components_dir)
        .iter()
        .flat_map(|p| match extract_components(p) {
            Ok(comps) => comps,
            Err(e) => {
                eprintln!("warning: failed to read {}: {}", p.display(), e);
                vec![]
            }
        })
        .collect();
    // `# Internal` components exist for a JS shim to call; a layout author who
    // imported one would get different behaviour from the JSX tag of the same
    // name, so they are left out of the docs entirely.
    components.retain(|c| !c.internal);
    components.sort_by(|a, b| a.export_name.cmp(&b.export_name));
    components
}

const SCREENSHOT_BINARY_CANDIDATES: &[&str] = &[
    "target/debug/tauler-screenshot",
    "target/release/tauler-screenshot",
];

fn find_screenshot_binary() -> Option<PathBuf> {
    let root = workspace_root();
    for candidate in SCREENSHOT_BINARY_CANDIDATES {
        let p = root.join(candidate);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn parse_pascal_tag_name(s: &str) -> Option<(&str, String)> {
    let mut chars = s.chars();
    let first = chars.next().filter(|c| c.is_uppercase())?;
    let tail: String = chars
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect();
    let name = format!("{}{}", first, tail);
    Some((&s[name.len()..], name))
}

fn jsx_component_names(jsx_block: &[String]) -> Vec<String> {
    let mut names = Vec::new();
    for line in jsx_block {
        let mut rest = line.as_str();
        while let Some(lt_pos) = rest.find('<') {
            rest = &rest[lt_pos + 1..];
            if let Some((remaining, name)) = parse_pascal_tag_name(rest) {
                rest = remaining;
                names.push(name);
            }
        }
    }
    names
}

fn format_import(comp: &Component) -> String {
    format!(
        "import {{ {} }} from '{}';\n",
        comp.export_name, comp.module_path
    )
}

fn collect_jsx_imports(jsx_block: &[String], all_components: &[Component]) -> String {
    let mut seen: HashSet<&str> = HashSet::new();
    jsx_component_names(jsx_block)
        .into_iter()
        .filter_map(|name| all_components.iter().find(|c| c.export_name == name))
        .filter(|dep| seen.insert(dep.export_name.as_str()))
        .map(format_import)
        .collect()
}

fn build_jsx_module(jsx_block: &[String], all_components: &[Component]) -> String {
    let imports = collect_jsx_imports(jsx_block, all_components);
    format!(
        "{imports}export default function render() {{\n{}\n}}\n",
        web::render_body(jsx_block)
    )
}

/// What rendering one component produced.
struct Rendered {
    png: PathBuf,
    /// Every Tailwind utility the resolved tree carried. Harvested from the render rather
    /// than scanned from the source, because theme tokens are already rewritten by then.
    classes: BTreeSet<String>,
}

fn render_screenshot(
    component: &Component,
    all_components: &[Component],
    assets_dir: &Path,
    geometry_dir: Option<&Path>,
) -> Option<Rendered> {
    let jsx_block = component.jsx_block.as_ref()?;
    let bin = find_screenshot_binary()?;

    let source = build_jsx_module(jsx_block, all_components);
    let tmp_jsx_file = std::env::temp_dir().join(format!(
        "tauler-docgen-{}.jsx",
        component.export_name.to_lowercase()
    ));
    fs::write(&tmp_jsx_file, &source).ok()?;

    fs::create_dir_all(assets_dir).ok()?;
    let output_path = assets_dir.join(format!("{}.png", component.export_name.to_lowercase()));

    let inter_font = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .join("assets/fonts/inter/InterVariable.ttf");
    let classes_file = std::env::temp_dir().join(format!(
        "tauler-docgen-{}-classes.txt",
        component.export_name.to_lowercase()
    ));

    // Written beside the screenshot rather than in a pass of its own: the geometry has to
    // come from the same render as the pixels, or the comparison is against a layout that
    // was never photographed (ADR 0028).
    let geometry_file = geometry_dir.map(|dir| {
        let _ = fs::create_dir_all(dir);
        dir.join(format!("{}.json", component.export_name.to_lowercase()))
    });

    let mut command = std::process::Command::new(&bin);
    if let Some(path) = &geometry_file {
        command.arg("--geometry-out").arg(path);
    }
    let status = command
        .arg("--classes-out")
        .arg(&classes_file)
        .arg("--input")
        .arg(&tmp_jsx_file)
        .arg("--output")
        .arg(&output_path)
        .arg("--font-path")
        .arg(&inter_font)
        .status()
        .ok()?;

    let _ = fs::remove_file(&tmp_jsx_file);
    if !status.success() {
        return None;
    }
    let classes = fs::read_to_string(&classes_file)
        .map(|body| {
            body.lines()
                .filter(|l| !l.trim().is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let _ = fs::remove_file(&classes_file);
    Some(Rendered {
        png: output_path,
        classes,
    })
}

fn trim_blank_lines(lines: &[String]) -> &[String] {
    let start = lines
        .iter()
        .position(|l| !l.trim().is_empty())
        .unwrap_or(lines.len());
    let end = lines
        .iter()
        .rposition(|l| !l.trim().is_empty())
        .map(|i| i + 1)
        .unwrap_or(0);
    if start <= end {
        &lines[start..end]
    } else {
        &[]
    }
}

fn mdx_safe_line(line: &str) -> String {
    // Convert <https://...> autolinks to [url](url) — MDX parses angle brackets as JSX.
    let mut result = String::new();
    let mut rest = line;
    while let Some(start) = rest.find('<') {
        let after = &rest[start + 1..];
        if after.starts_with("http://") || after.starts_with("https://") {
            if let Some(end) = after.find('>') {
                let url = &after[..end];
                result.push_str(&rest[..start]);
                result.push_str(&format!("[{url}]({url})"));
                rest = &after[end + 1..];
                continue;
            }
        }
        result.push_str(&rest[..start + 1]);
        rest = &rest[start + 1..];
    }
    result.push_str(rest);
    result
}

fn render_prose(out: &mut String, prose: &[String]) {
    let trimmed = trim_blank_lines(prose);
    for line in trimmed {
        out.push_str(&mdx_safe_line(line));
        out.push('\n');
    }
    if !trimmed.is_empty() {
        out.push('\n');
    }
}

/// Every PascalCase tag in `block` that names a known component.
///
/// UI components are ES modules, not globals, so a usage example without its
/// imports throws at eval rather than rendering. Tags that are not components —
/// layout nodes like `<span>`, and globals like `<Module>` and `<I3Layout>` —
/// are left alone: an import for those would break the example it is meant to fix.
fn components_used_in<'a>(block: &[String], all: &'a [Component]) -> Vec<&'a Component> {
    let mut used: Vec<&Component> = Vec::new();
    for line in block {
        for candidate in line.split('<').skip(1) {
            let name: String = candidate
                .chars()
                .take_while(|c| c.is_alphanumeric())
                .collect();
            if !name.chars().next().is_some_and(char::is_uppercase) {
                continue;
            }
            if let Some(comp) = all.iter().find(|c| c.export_name == name) {
                if !used.iter().any(|c| c.export_name == comp.export_name) {
                    used.push(comp);
                }
            }
        }
    }
    used
}

/// One `import` line per module, names sorted, so the block is paste-ready.
fn render_imports(out: &mut String, used: &[&Component]) {
    let mut modules: Vec<&str> = used.iter().map(|c| c.module_path.as_str()).collect();
    modules.sort_unstable();
    modules.dedup();

    for module in modules {
        let mut names: Vec<&str> = used
            .iter()
            .filter(|c| c.module_path == module)
            .map(|c| c.export_name.as_str())
            .collect();
        names.sort_unstable();
        out.push_str(&format!(
            "import {{ {} }} from \"{}\";\n",
            names.join(", "),
            module
        ));
    }
    out.push('\n');
}

fn render_jsx_usage(out: &mut String, block: &[String], used: &[&Component]) {
    out.push_str("### Usage\n\n```jsx\n");
    if !used.is_empty() {
        render_imports(out, used);
    }
    for line in block {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("```\n\n");
}

fn render_component_section(
    comp: &Component,
    screenshot: &Option<PathBuf>,
    all: &[Component],
) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "## {}\n\n**Module:** `{}`\n\n",
        comp.export_name, comp.module_path
    ));
    if let Some(url) = &comp.shadcn_url {
        out.push_str(&format!("**Shadcn reference:** {}\n\n", url));
    }
    if let Some(path) = screenshot {
        let filename = path.file_name().unwrap().to_string_lossy();
        // The live embed first, the screenshot behind it. The screenshot is not decoration
        // — it is what a reader sees when the wasm module is absent, which is the ordinary
        // state of a checkout without a Rust toolchain (ADR 0026).
        if !comp.skip_web {
            out.push_str(&format!(
                "<div class=\"tauler-mount\" data-tauler-example=\"{}\"></div>\n\n",
                comp.export_name.to_lowercase()
            ));
        }
        out.push_str(&format!(
            "<div class=\"tauler-fallback\" data-tauler-example=\"{}\">\n\n",
            comp.export_name.to_lowercase()
        ));
        // Relative to the generated page, so Astro processes the image and the
        // URL stays correct whatever `base` the site is deployed under.
        out.push_str(&format!(
            "![{} screenshot](../../../assets/{})\n\n",
            comp.export_name, filename
        ));
        out.push_str("</div>\n\n");
    }
    render_prose(&mut out, &comp.prose);
    if let Some(block) = &comp.jsx_block {
        // The component being documented is always needed, even when the example
        // only nests it inside something else.
        let mut used = components_used_in(block, all);
        if !used.iter().any(|c| c.export_name == comp.export_name) {
            used.insert(0, comp);
        }
        render_jsx_usage(&mut out, block, &used);
    }
    out
}

fn render_markdown(components: &[Component], rendered: &[Option<Rendered>]) -> String {
    let mut out = String::new();
    out.push_str("---\ntitle: Components\ndescription: Auto-generated by tauler-docgen. Do not edit by hand.\n---\n\n");
    out.push_str(&mount_head());
    for (comp, r) in components.iter().zip(rendered.iter()) {
        out.push_str(&render_component_section(
            comp,
            &r.as_ref().map(|r| r.png.clone()),
            components,
        ));
    }
    out.push_str(MOUNT_SCRIPT);
    out
}

/// What every embed on the page needs before any of them can run: the compiled Tailwind,
/// and the import map that makes `@ui/card` resolve. The map is inline and near the top
/// because an import map has to be in the document before the first module loads.
fn mount_head() -> String {
    format!(
        "<link rel=\"stylesheet\" href=\"/tauler/tauler.css\" />\n\n<script type=\"importmap\">\n{}</script>\n\n",
        web::import_map_json()
    )
}

/// Boots the wasm module once and mounts every embed on the page.
///
/// Nothing here decides anything — `boot` and `Mount` are `tauler-web`'s. If the module
/// fails to load (a checkout without a Rust toolchain, most often), every embed keeps its
/// screenshot and the page is stale rather than empty.
const MOUNT_SCRIPT: &str = r#"
<script type="module">
  import { boot, Mount, registerTransport, setStreamValue } from '/tauler/runtime.js'

  globalThis.__taulerErrors = []
  const mounts = [...document.querySelectorAll('.tauler-mount')]
  if (mounts.length) {
    try {
      await boot('/tauler/tauler_web_bg.wasm')

      // A Module, implemented in the page instead of as a subprocess.
      //
      // It is what makes the Slider example on this page work, and it is the whole
      // demonstration: the layout file says `useEvents("tauler-demo-volume")` and
      // `useStringStream("tauler-demo-volume")` and cannot tell whether the other end is
      // a process, a worker, an SSH connection or these four lines. A Transport is
      // whatever fills the map and takes the intents.
      registerTransport('tauler-demo-volume', (event) => {
        if (event?.type === 'set') setStreamValue('tauler-demo-volume', null, event.value)
      })

      for (const el of mounts) {
        const name = el.dataset.taulerExample
        const { default: render } = await import(`/tauler/examples/${name}.js`)
        new Mount(el, render).tick()
        document.querySelector(`.tauler-fallback[data-tauler-example="${name}"]`)
          ?.setAttribute('hidden', '')
      }
    } catch (e) {
      globalThis.__taulerErrors.push(String((e && e.stack) || e))
      console.warn('tauler: falling back to screenshots', e)
    }
  }
</script>
"#;

/// Write every browser asset, and the Tailwind input that compiles the classes they use.
fn generate_web(
    args: &Args,
    components: &[Component],
    rendered: &[Option<Rendered>],
) -> std::io::Result<()> {
    fs::create_dir_all(&args.web_dir)?;
    web::write_ui_modules(&args.web_dir)?;
    web::write_constants(&args.web_dir)?;

    let runtime = workspace_root().join("tauler-web/js/runtime.js");
    fs::copy(&runtime, args.web_dir.join("runtime.js"))?;

    // Units are opt-in — most pages never import this — so it stays its own
    // file rather than growing runtime.js (ADR 0036).
    let units = workspace_root().join("tauler-web/js/units.js");
    fs::copy(&units, args.web_dir.join("units.js"))?;

    let fonts_dir = args.web_dir.join("fonts");
    fs::create_dir_all(&fonts_dir)?;
    fs::copy(
        workspace_root().join("assets/fonts/inter/InterVariable.ttf"),
        fonts_dir.join("InterVariable.ttf"),
    )?;

    let mut classes = BTreeSet::new();
    for (comp, r) in components.iter().zip(rendered.iter()) {
        if let Some(r) = r {
            classes.extend(r.classes.iter().cloned());
        }
        let Some(block) = &comp.jsx_block else {
            continue;
        };
        if comp.internal || comp.skip_web {
            continue;
        }
        let imports = collect_jsx_imports(block, components);
        let source = web::build_web_module(&imports, block);
        web::write_example(&args.web_dir, comp, &source)?;
    }
    web::write_stylesheet_input(&args.build_dir, &classes, "/tauler/fonts/InterVariable.ttf")
}

fn main() {
    let args = Args::parse();
    let components_dir = args.components_dir.as_path();
    let output_path = args.output.as_path();
    let assets_dir = args.assets_dir.as_path();

    if !components_dir.exists() {
        eprintln!(
            "error: components directory not found at {}",
            components_dir.display()
        );
        std::process::exit(1);
    }

    let all_components = load_all_components(components_dir);
    if all_components.is_empty() {
        eprintln!("warning: no components found — output will only contain a header");
    }
    let rendered: Vec<Option<Rendered>> = all_components
        .iter()
        .map(|c| render_screenshot(c, &all_components, assets_dir, Some(&args.geometry_dir)))
        .collect();

    if let Err(e) = generate_web(&args, &all_components, &rendered) {
        eprintln!("error: could not write the browser assets: {e}");
        std::process::exit(1);
    }

    let markdown = render_markdown(&all_components, &rendered);

    if let Some(parent) = output_path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            eprintln!("error: could not create {}: {}", parent.display(), e);
            std::process::exit(1);
        }
    }
    if let Err(e) = fs::write(output_path, &markdown) {
        eprintln!("error: could not write {}: {}", output_path.display(), e);
        std::process::exit(1);
    }
    println!(
        "wrote {} ({} component(s))",
        output_path.display(),
        all_components.len()
    );
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("CARGO_MANIFEST_DIR has no parent")
        .to_path_buf()
}

#[cfg(test)]
mod visual_regression {
    use super::*;

    // Same LCG multiplier used in layout-poc/src/main.rs — keep in sync if changed.
    const LCG_MULTIPLIER: u64 = 6364136223846793005;

    fn pixel_hash(pixels: &[u8]) -> String {
        let h = pixels.iter().fold(0u64, |h, &b| {
            h.wrapping_mul(LCG_MULTIPLIER).wrapping_add(b as u64)
        });
        format!("{h:016x}  ({} px)", pixels.len() / 4)
    }

    /// Renders every component that has a JSX block and asserts the pixel hash
    /// via insta.  Screenshots go to a temporary directory — publishing them is
    /// `just docs`'s job.  Skips silently if the screenshot binary is absent.
    ///
    /// Workflow:
    ///   cargo build -p tauler-screenshot          # build renderer once
    ///   cargo test -p tauler-docgen               # fails if any hash changed
    ///   cargo insta review                        # inspect PNG + accept or reject
    #[test]
    fn component_screenshots_match_approved_hashes() {
        if find_screenshot_binary().is_none() {
            eprintln!("skipping: tauler-screenshot binary not found (run `cargo build -p tauler-screenshot` first)");
            return;
        }

        let root = workspace_root();
        let components_dir = root.join("tauler-core/src/ui/components");
        let render_dir = tempfile::tempdir().expect("temp dir for rendered screenshots");
        let assets_dir = render_dir.path().to_path_buf();
        let snap_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("snapshots");

        let all_components = load_all_components(&components_dir);

        insta::with_settings!({ snapshot_path => &snap_dir, prepend_module_to_snapshot => false }, {
            for comp in &all_components {
                if comp.jsx_block.is_none() { continue; }
                if comp.skip_snapshot { continue; }
                let png_path = render_screenshot(comp, &all_components, &assets_dir, None)
                    .unwrap_or_else(|| panic!("render_screenshot failed for {}", comp.export_name))
                    .png;
                let img = image::open(&png_path)
                    .unwrap_or_else(|e| panic!("failed to open {}: {e}", png_path.display()))
                    .into_rgba8();
                let hash = pixel_hash(&img.into_raw());
                insta::assert_snapshot!(comp.export_name.to_lowercase(), hash);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_screenshot_saves_png_for_component_with_jsx_block() {
        if find_screenshot_binary().is_none() {
            eprintln!(
                "skipping: tauler-screenshot binary not found (run `cargo build -p tauler-screenshot` first)"
            );
            return;
        }

        let tmp = tempfile::tempdir().expect("tempdir failed");
        let assets_dir = tmp.path();

        let component = Component {
            module_path: "@ui/card".to_string(),
            export_name: "Card".to_string(),
            prose: vec![],
            jsx_block: Some(vec!["<Card />".to_string()]),
            shadcn_url: None,
            skip_snapshot: false,
            internal: false,
            skip_web: false,
        };
        let all_components = vec![Component {
            module_path: "@ui/card".to_string(),
            export_name: "Card".to_string(),
            prose: vec![],
            jsx_block: None,
            shadcn_url: None,
            skip_snapshot: false,
            internal: false,
            skip_web: false,
        }];

        let result = render_screenshot(&component, &all_components, assets_dir, None);

        assert!(result.is_some(), "expected a render but got None");
        let rendered = result.unwrap();
        let path = rendered.png;
        assert!(path.exists(), "PNG file does not exist at {:?}", path);
        assert!(path.metadata().unwrap().len() > 0, "PNG file is empty");
        assert!(
            !rendered.classes.is_empty(),
            "the class harvest came back empty; Tailwind would compile nothing"
        );
    }

    /// A component registered only so a JS shim can call it is not something a
    /// layout author should import — documenting it would hand them a name that
    /// does the wrong thing.
    #[test]
    fn parse_doc_comments_sets_internal_when_marker_present() {
        let raw = vec!["Some prose.".to_string(), "# Internal".to_string()];
        let doc = parse_doc_comments(&raw);
        assert!(doc.internal);
        assert!(
            !doc.prose.iter().any(|l| l.contains("# Internal")),
            "the marker must not leak into the prose"
        );
    }

    #[test]
    fn parse_doc_comments_defaults_internal_to_false() {
        let doc = parse_doc_comments(&["Some prose.".to_string()]);
        assert!(!doc.internal);
    }

    #[test]
    fn parse_doc_comments_sets_skip_snapshot_when_marker_present() {
        let raw = vec!["# SkipSnapshot".to_string()];
        let doc = parse_doc_comments(&raw);
        assert!(doc.skip_snapshot);
    }

    #[test]
    fn parse_doc_comments_skip_snapshot_defaults_to_false() {
        let raw = vec!["Some prose.".to_string()];
        let doc = parse_doc_comments(&raw);
        assert!(!doc.skip_snapshot);
    }

    fn component(module_path: &str, export_name: &str, jsx: Option<Vec<&str>>) -> Component {
        Component {
            module_path: module_path.to_string(),
            export_name: export_name.to_string(),
            prose: vec![],
            jsx_block: jsx.map(|b| b.into_iter().map(str::to_string).collect()),
            shadcn_url: None,
            skip_snapshot: false,
            internal: false,
            skip_web: false,
        }
    }

    /// The usage block is what people paste. Without the import it throws at
    /// eval, because UI components are ES modules and not globals.
    #[test]
    fn usage_block_is_preceded_by_the_import_it_needs() {
        let comp = component("@ui/datatable", "DataTable", Some(vec!["<DataTable />"]));
        let out = render_component_section(&comp, &None, std::slice::from_ref(&comp));
        assert!(
            out.contains(r#"import { DataTable } from "@ui/datatable";"#),
            "missing import in:\n{out}"
        );
    }

    /// An example built from several components of one module needs them all,
    /// in one import — the `Table` example uses six.
    #[test]
    fn every_component_used_in_the_example_is_imported() {
        let all = vec![
            component("@ui/table", "Table", None),
            component("@ui/table", "TableRow", None),
            component("@ui/table", "TableCell", None),
        ];
        let comp = component(
            "@ui/table",
            "Table",
            Some(vec![
                "<Table>",
                "  <TableRow><TableCell /></TableRow>",
                "</Table>",
            ]),
        );
        let out = render_component_section(&comp, &None, &all);
        assert!(
            out.contains(r#"import { Table, TableCell, TableRow } from "@ui/table";"#),
            "expected one grouped import, got:\n{out}"
        );
    }

    /// Components used from a different module get their own import line.
    #[test]
    fn components_from_other_modules_get_their_own_import() {
        let all = vec![
            component("@ui/card", "Card", None),
            component("@ui/progress", "Progress", None),
        ];
        let comp = component("@ui/card", "Card", Some(vec!["<Card><Progress /></Card>"]));
        let out = render_component_section(&comp, &None, &all);
        assert!(out.contains(r#"import { Card } from "@ui/card";"#), "{out}");
        assert!(
            out.contains(r#"import { Progress } from "@ui/progress";"#),
            "{out}"
        );
    }

    /// Lowercase tags are layout nodes, not components — importing `<span>`
    /// would be a name that does not resolve.
    #[test]
    fn layout_nodes_are_not_imported() {
        let comp = component(
            "@ui/card",
            "Card",
            Some(vec!["<Card><span>hi</span></Card>"]),
        );
        let out = render_component_section(&comp, &None, std::slice::from_ref(&comp));
        assert!(!out.contains("text }"), "layout node imported in:\n{out}");
    }

    /// `<Module>` and `<I3Layout>` are globals, so an import for them would
    /// break the example rather than fix it.
    #[test]
    fn unknown_pascal_case_tags_are_not_imported() {
        let comp = component("@ui/card", "Card", Some(vec!["<Module><Card /></Module>"]));
        let out = render_component_section(&comp, &None, std::slice::from_ref(&comp));
        assert!(!out.contains("Module }"), "global imported in:\n{out}");
    }

    #[test]
    fn a_component_with_no_example_gets_no_import() {
        let comp = component("@ui/card", "Card", None);
        let out = render_component_section(&comp, &None, std::slice::from_ref(&comp));
        assert!(!out.contains("import"), "{out}");
    }
}
