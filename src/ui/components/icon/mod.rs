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
/// <div class="flex flex-col gap-[16px] p-[12px]">
///   <div class="flex flex-row items-end gap-[20px]">
///     <div class="flex flex-col items-center gap-[4px]">
///       <Icon name="md-star" class="text-[12px]" />
///       <span class="text-[9px] text-muted-foreground">12px</span>
///     </div>
///     <div class="flex flex-col items-center gap-[4px]">
///       <Icon name="md-star" class="text-[16px]" />
///       <span class="text-[9px] text-muted-foreground">16px</span>
///     </div>
///     <div class="flex flex-col items-center gap-[4px]">
///       <Icon name="md-star" class="text-[20px]" />
///       <span class="text-[9px] text-muted-foreground">20px</span>
///     </div>
///     <div class="flex flex-col items-center gap-[4px]">
///       <Icon name="md-star" class="text-[28px]" />
///       <span class="text-[9px] text-muted-foreground">28px</span>
///     </div>
///     <div class="flex flex-col items-center gap-[4px]">
///       <Icon name="md-star" class="text-[36px]" />
///       <span class="text-[9px] text-muted-foreground">36px</span>
///     </div>
///   </div>
///   <div class="flex flex-row flex-wrap gap-x-[20px] gap-y-[12px]">
///     <div class="flex flex-col items-center gap-[4px]">
///       <Icon name="md-home" class="text-[20px]" />
///       <span class="text-[9px] text-muted-foreground">md-home</span>
///     </div>
///     <div class="flex flex-col items-center gap-[4px]">
///       <Icon name="md-heart" class="text-[20px]" />
///       <span class="text-[9px] text-muted-foreground">md-heart</span>
///     </div>
///     <div class="flex flex-col items-center gap-[4px]">
///       <Icon name="fa-github" class="text-[20px]" />
///       <span class="text-[9px] text-muted-foreground">fa-github</span>
///     </div>
///     <div class="flex flex-col items-center gap-[4px]">
///       <Icon name="cod-terminal" class="text-[20px]" />
///       <span class="text-[9px] text-muted-foreground">cod-terminal</span>
///     </div>
///     <div class="flex flex-col items-center gap-[4px]">
///       <Icon name="oct-git_branch" class="text-[20px]" />
///       <span class="text-[9px] text-muted-foreground">oct-git_branch</span>
///     </div>
///     <div class="flex flex-col items-center gap-[4px]">
///       <Icon name="dev-linux" class="text-[20px]" />
///       <span class="text-[9px] text-muted-foreground">dev-linux</span>
///     </div>
///     <div class="flex flex-col items-center gap-[4px]">
///       <Icon name="md-folder" class="text-[20px]" />
///       <span class="text-[9px] text-muted-foreground">md-folder</span>
///     </div>
///     <div class="flex flex-col items-center gap-[4px]">
///       <Icon name="fa-star" class="text-[20px]" />
///       <span class="text-[9px] text-muted-foreground">fa-star</span>
///     </div>
///     <div class="flex flex-col items-center gap-[4px]">
///       <Icon name="oct-repo" class="text-[20px]" />
///       <span class="text-[9px] text-muted-foreground">oct-repo</span>
///     </div>
///     <div class="flex flex-col items-center gap-[4px]">
///       <Icon name="cod-search" class="text-[20px]" />
///       <span class="text-[9px] text-muted-foreground">cod-search</span>
///     </div>
///     <div class="flex flex-col items-center gap-[4px]">
///       <Icon name="md-wifi" class="text-[20px]" />
///       <span class="text-[9px] text-muted-foreground">md-wifi</span>
///     </div>
///   </div>
/// </div>
/// ```
#[component("@ui/icon")]
pub fn icon(name: String, class: Option<String>) -> Node {
    let glyph = ICON_MAP.get(&name).map(|s| s.as_str()).unwrap_or("?");
    let class = ICON_VARIANTS.resolve(&[], class.as_deref().unwrap_or(""));
    rsx! { <span class={class}>{glyph}</span> }
}
