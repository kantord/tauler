use std::fs;
use std::path::{Path, PathBuf};

/// A single extracted component entry.
struct Component {
    /// The `@ui/...` module path from the `#[component("...")]` attribute.
    module_path: String,
    /// PascalCase export name derived from the function name.
    export_name: String,
    /// Prose lines from `///` doc comments (excluding `# JSX` / `# Shadcn` sections).
    prose: Vec<String>,
    /// Lines inside the `# JSX` fenced code block, if present.
    jsx_block: Option<Vec<String>>,
    /// The URL that follows `# Shadcn`, if present.
    shadcn_url: Option<String>,
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
            // strip one leading space if present
            let content = rest.strip_prefix(' ').unwrap_or(rest);
            doc_lines.push(content.to_string());
            idx -= 1;
        } else {
            break;
        }
    }
    doc_lines.reverse();
    doc_lines
}

/// Parse doc-comment lines into prose, optional JSX block, and optional Shadcn URL.
fn parse_doc_comments(
    raw: &[String],
) -> (Vec<String>, Option<Vec<String>>, Option<String>) {
    let mut prose: Vec<String> = Vec::new();
    let mut jsx_block: Option<Vec<String>> = None;
    let mut shadcn_url: Option<String> = None;

    let mut i = 0;
    while i < raw.len() {
        let line = &raw[i];

        if line.trim() == "# JSX" {
            // Collect the fenced code block that follows.
            i += 1;
            // skip opening fence (e.g. "```jsx")
            if i < raw.len() && raw[i].trim_start().starts_with("```") {
                i += 1;
                let mut block: Vec<String> = Vec::new();
                while i < raw.len() && !raw[i].trim_start().starts_with("```") {
                    block.push(raw[i].clone());
                    i += 1;
                }
                // skip closing fence
                if i < raw.len() {
                    i += 1;
                }
                jsx_block = Some(block);
            }
            continue;
        }

        if line.trim() == "# Shadcn" {
            // The URL is on the next line.
            i += 1;
            if i < raw.len() {
                shadcn_url = Some(raw[i].trim().to_string());
                i += 1;
            }
            continue;
        }

        // Regular prose — skip section headers that belong to JSX/Shadcn blocks.
        prose.push(line.clone());
        i += 1;
    }

    (prose, jsx_block, shadcn_url)
}

/// Scan a single `.rs` file and return any components found.
fn extract_components(path: &Path) -> Vec<Component> {
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    let lines: Vec<&str> = source.lines().collect();
    let mut components = Vec::new();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        // Match `#[component("@ui/...")]`
        if let Some(inner) = trimmed
            .strip_prefix("#[component(\"")
            .and_then(|s| s.strip_suffix("\")]"))
        {
            if !inner.starts_with('@') {
                continue;
            }
            let module_path = inner.to_string();

            // The next non-empty line should be the `pub fn <name>(...)` declaration.
            let fn_line_idx = (i + 1..lines.len())
                .find(|&j| !lines[j].trim().is_empty());

            let fn_name = fn_line_idx.and_then(|j| {
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
            });

            let fn_name = match fn_name {
                Some(n) => n,
                None => continue,
            };

            let export_name = to_pascal_case(&fn_name);

            let raw_docs = collect_doc_comment_lines(&lines, i);
            let (prose, jsx_block, shadcn_url) = parse_doc_comments(&raw_docs);

            components.push(Component {
                module_path,
                export_name,
                prose,
                jsx_block,
                shadcn_url,
            });
        }
    }

    components
}

/// Collect all `.rs` files under `dir` recursively.
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

fn find_screenshot_binary() -> Option<PathBuf> {
    for candidate in &["target/debug/costae-screenshot", "target/release/costae-screenshot"] {
        let p = PathBuf::from(candidate);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn build_jsx_module(jsx_block: &[String], all_components: &[Component]) -> String {
    let mut imports = String::new();
    for line in jsx_block {
        let mut rest = line.as_str();
        while let Some(lt_pos) = rest.find('<') {
            rest = &rest[lt_pos + 1..];
            let mut name_chars = rest.chars();
            if let Some(first) = name_chars.next() {
                if first.is_uppercase() {
                    let tail: String = name_chars
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    let name = format!("{}{}", first, tail);
                    if let Some(dep) = all_components.iter().find(|c| c.export_name == name) {
                        let line = format!("import {{ {} }} from '{}';\n", dep.export_name, dep.module_path);
                        if !imports.contains(&line) {
                            imports.push_str(&line);
                        }
                    }
                    rest = &rest[name.len()..];
                }
            }
        }
    }
    let body = jsx_block.join("\n");
    format!("{imports}export default function render() {{\n  return (\n{body}\n  );\n}}\n")
}

fn render_screenshot(
    component: &Component,
    all_components: &[Component],
    assets_dir: &Path,
) -> Option<PathBuf> {
    let jsx_block = component.jsx_block.as_ref()?;
    let bin = find_screenshot_binary()?;

    let source = build_jsx_module(jsx_block, all_components);
    let tmp_input = std::env::temp_dir().join(format!("costae-docgen-{}.jsx", component.export_name.to_lowercase()));
    fs::write(&tmp_input, &source).ok()?;

    fs::create_dir_all(assets_dir).ok()?;
    let output_path = assets_dir.join(format!("{}.png", component.export_name.to_lowercase()));

    let status = std::process::Command::new(&bin)
        .arg("--input").arg(&tmp_input)
        .arg("--output").arg(&output_path)
        .status()
        .ok()?;

    let _ = fs::remove_file(&tmp_input);
    if status.success() { Some(output_path) } else { None }
}

fn render_markdown(components: &[Component], screenshots: &[Option<PathBuf>]) -> String {
    let mut out = String::new();
    out.push_str("# Components\n\n");
    out.push_str("Auto-generated by `costae-docgen`. Do not edit by hand.\n\n");

    for (comp, screenshot) in components.iter().zip(screenshots.iter()) {
        out.push_str(&format!("## {}\n\n", comp.export_name));
        out.push_str(&format!("**Module:** `{}`\n\n", comp.module_path));

        if let Some(url) = &comp.shadcn_url {
            out.push_str(&format!("**Shadcn reference:** {}\n\n", url));
        }

        if let Some(path) = screenshot {
            let filename = path.file_name().unwrap().to_string_lossy();
            out.push_str(&format!("![{} screenshot](./assets/{})\n\n", comp.export_name, filename));
        }

        // Prose (skip blank-only leading/trailing lines)
        let prose_trimmed: Vec<&String> = comp
            .prose
            .iter()
            .skip_while(|l| l.trim().is_empty())
            .collect();
        let mut end = prose_trimmed.len();
        while end > 0 && prose_trimmed[end - 1].trim().is_empty() {
            end -= 1;
        }
        for line in &prose_trimmed[..end] {
            out.push_str(line);
            out.push('\n');
        }
        if !prose_trimmed[..end].is_empty() {
            out.push('\n');
        }

        if let Some(block) = &comp.jsx_block {
            out.push_str("### Usage\n\n");
            out.push_str("```jsx\n");
            for line in block {
                out.push_str(line);
                out.push('\n');
            }
            out.push_str("```\n\n");
        }
    }

    out
}

fn main() {
    // Resolve paths relative to the workspace root.
    // When run via `cargo run -p costae-docgen` the cwd is the workspace root.
    let components_dir = Path::new("src/ui/components");
    let docs_dir = Path::new("docs");
    let output_path = docs_dir.join("components.md");

    if !components_dir.exists() {
        eprintln!(
            "error: components directory not found at {}",
            components_dir.display()
        );
        std::process::exit(1);
    }

    let mut rs_files: Vec<PathBuf> = collect_rs_files(components_dir)
        .into_iter()
        .filter(|p| {
            !p.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .contains("test")
        })
        .collect();
    rs_files.sort(); // deterministic ordering

    let mut all_components: Vec<Component> = rs_files
        .iter()
        .flat_map(|p| extract_components(p))
        .collect();

    // Sort by export name for stable output.
    all_components.sort_by(|a, b| a.export_name.cmp(&b.export_name));

    if all_components.is_empty() {
        eprintln!("warning: no components found — docs/components.md will only contain a header");
    }

    let assets_dir = docs_dir.join("assets");
    let screenshots: Vec<Option<PathBuf>> = all_components
        .iter()
        .map(|c| render_screenshot(c, &all_components, &assets_dir))
        .collect();

    let markdown = render_markdown(&all_components, &screenshots);

    if let Err(e) = fs::create_dir_all(docs_dir) {
        eprintln!("error: could not create docs/ directory: {}", e);
        std::process::exit(1);
    }

    if let Err(e) = fs::write(&output_path, &markdown) {
        eprintln!("error: could not write {}: {}", output_path.display(), e);
        std::process::exit(1);
    }

    println!(
        "wrote {} ({} component(s))",
        output_path.display(),
        all_components.len()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_screenshot_saves_png_for_component_with_jsx_block() {
        if find_screenshot_binary().is_none() {
            eprintln!("skipping: costae-screenshot binary not found (run `cargo build -p costae-screenshot` first)");
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
        };
        let all_components = vec![Component {
            module_path: "@ui/card".to_string(),
            export_name: "Card".to_string(),
            prose: vec![],
            jsx_block: None,
            shadcn_url: None,
        }];

        let result = render_screenshot(&component, &all_components, assets_dir);

        assert!(result.is_some(), "expected Some(path) but got None");
        let path = result.unwrap();
        assert!(path.exists(), "PNG file does not exist at {:?}", path);
        assert!(path.metadata().unwrap().len() > 0, "PNG file is empty");
    }
}
