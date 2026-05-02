use std::collections::HashMap;
use std::sync::LazyLock;

use crate::ui::{component, cva::Cva, rsx};

const ICON_VARIANTS: Cva = Cva {
    base: "text-foreground",
    variants: &[],
    defaults: &[],
};

static ICON_MAP: LazyLock<HashMap<String, String>> = LazyLock::new(|| {
    let raw = include_str!("vendor/nerd-fonts/glyphnames.json");
    let json: serde_json::Value = serde_json::from_str(raw).expect("glyphnames.json is valid JSON");
    json.as_object()
        .expect("glyphnames.json root is an object")
        .iter()
        .filter(|(k, _)| *k != "METADATA")
        .filter_map(|(k, v)| v.get("char")?.as_str().map(|c| (k.clone(), c.to_owned())))
        .collect()
});

/// Renders a single Nerd Font glyph by icon name.
///
/// `name` uses the Nerd Fonts naming convention: `{family}-{icon}`,
/// e.g. `md-home`, `fa-github`, `cod-terminal`.
/// Full catalogue: <https://www.nerdfonts.com/cheat-sheet>
///
/// Unknown names render as `?`.
///
/// # SkipSnapshot
///
/// # JSX
/// ```jsx
/// <container tw="flex flex-col gap-[16px] p-[12px]">
///   <container tw="flex flex-row items-end gap-[20px]">
///     <container tw="flex flex-col items-center gap-[4px]">
///       <Icon name="md-star" tw="text-[12px]" />
///       <text tw="text-[9px] text-muted-foreground">12px</text>
///     </container>
///     <container tw="flex flex-col items-center gap-[4px]">
///       <Icon name="md-star" tw="text-[16px]" />
///       <text tw="text-[9px] text-muted-foreground">16px</text>
///     </container>
///     <container tw="flex flex-col items-center gap-[4px]">
///       <Icon name="md-star" tw="text-[20px]" />
///       <text tw="text-[9px] text-muted-foreground">20px</text>
///     </container>
///     <container tw="flex flex-col items-center gap-[4px]">
///       <Icon name="md-star" tw="text-[28px]" />
///       <text tw="text-[9px] text-muted-foreground">28px</text>
///     </container>
///     <container tw="flex flex-col items-center gap-[4px]">
///       <Icon name="md-star" tw="text-[36px]" />
///       <text tw="text-[9px] text-muted-foreground">36px</text>
///     </container>
///   </container>
///   <container tw="flex flex-row flex-wrap gap-x-[20px] gap-y-[12px]">
///     <container tw="flex flex-col items-center gap-[4px]">
///       <Icon name="md-home" tw="text-[20px]" />
///       <text tw="text-[9px] text-muted-foreground">md-home</text>
///     </container>
///     <container tw="flex flex-col items-center gap-[4px]">
///       <Icon name="md-heart" tw="text-[20px]" />
///       <text tw="text-[9px] text-muted-foreground">md-heart</text>
///     </container>
///     <container tw="flex flex-col items-center gap-[4px]">
///       <Icon name="fa-github" tw="text-[20px]" />
///       <text tw="text-[9px] text-muted-foreground">fa-github</text>
///     </container>
///     <container tw="flex flex-col items-center gap-[4px]">
///       <Icon name="cod-terminal" tw="text-[20px]" />
///       <text tw="text-[9px] text-muted-foreground">cod-terminal</text>
///     </container>
///     <container tw="flex flex-col items-center gap-[4px]">
///       <Icon name="oct-git_branch" tw="text-[20px]" />
///       <text tw="text-[9px] text-muted-foreground">oct-git_branch</text>
///     </container>
///     <container tw="flex flex-col items-center gap-[4px]">
///       <Icon name="dev-linux" tw="text-[20px]" />
///       <text tw="text-[9px] text-muted-foreground">dev-linux</text>
///     </container>
///     <container tw="flex flex-col items-center gap-[4px]">
///       <Icon name="md-folder" tw="text-[20px]" />
///       <text tw="text-[9px] text-muted-foreground">md-folder</text>
///     </container>
///     <container tw="flex flex-col items-center gap-[4px]">
///       <Icon name="fa-star" tw="text-[20px]" />
///       <text tw="text-[9px] text-muted-foreground">fa-star</text>
///     </container>
///     <container tw="flex flex-col items-center gap-[4px]">
///       <Icon name="oct-repo" tw="text-[20px]" />
///       <text tw="text-[9px] text-muted-foreground">oct-repo</text>
///     </container>
///     <container tw="flex flex-col items-center gap-[4px]">
///       <Icon name="cod-search" tw="text-[20px]" />
///       <text tw="text-[9px] text-muted-foreground">cod-search</text>
///     </container>
///     <container tw="flex flex-col items-center gap-[4px]">
///       <Icon name="md-wifi" tw="text-[20px]" />
///       <text tw="text-[9px] text-muted-foreground">md-wifi</text>
///     </container>
///   </container>
/// </container>
/// ```
#[component("@ui/icon")]
pub fn icon(name: String, tw: Option<String>) -> Node {
    let glyph = ICON_MAP.get(&name).map(|s| s.as_str()).unwrap_or("?");
    let tw = ICON_VARIANTS.resolve(&[], tw.as_deref().unwrap_or(""));
    rsx! { <text tw={tw}>{glyph}</text> }
}
