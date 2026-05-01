use crate::ui::{Node, component, rsx, tw_merge, cva::Cva};

const CARD_VARIANTS: Cva = Cva {
    base: "rounded-lg border border-border bg-card text-card-foreground px-3 py-[10px]",
    variants: &[],
    defaults: &[],
};

const HEADER_BASE_TW: &str = "flex flex-col gap-[4px]";
const TITLE_BASE_TW: &str = "font-semibold text-[14px] leading-none tracking-tight";
const DESCRIPTION_BASE_TW: &str = "text-[12px] text-muted-foreground";
const CONTENT_BASE_TW: &str = "flex flex-col";
const FOOTER_BASE_TW: &str = "flex flex-row items-center";

/// A styled container with rounded corners, a border, and card background colour.
/// Wraps arbitrary child nodes and accepts an optional `tw` prop for Tailwind overrides.
///
/// # JSX
/// ```jsx
/// <Card tw="flex flex-col gap-[6px]">
///   <CardHeader>
///     <CardTitle><text>System Status</text></CardTitle>
///     <CardDescription><text>All services operational</text></CardDescription>
///   </CardHeader>
///   <CardContent>
///     <text tw="text-foreground text-[12px]">nginx · postgres · redis</text>
///   </CardContent>
/// </Card>
/// ```
///
/// # Shadcn
/// https://ui.shadcn.com/docs/components/card
#[component("@ui/card")]
pub fn card(children: Vec<Node>, tw: Option<String>) -> Node {
    let tw = CARD_VARIANTS.resolve(&[], tw.as_deref().unwrap_or(""));
    rsx! {
        <container tw={tw}>
            {children}
        </container>
    }
}

#[component("@ui/card")]
pub fn card_header(children: Vec<Node>, tw: Option<String>) -> Node {
    let tw = tw_merge(HEADER_BASE_TW, tw.as_deref().unwrap_or(""));
    rsx! { <container tw={tw}>{children}</container> }
}

#[component("@ui/card")]
pub fn card_title(children: Vec<Node>, tw: Option<String>) -> Node {
    let tw = tw_merge(TITLE_BASE_TW, tw.as_deref().unwrap_or(""));
    rsx! { <container tw={tw}>{children}</container> }
}

#[component("@ui/card")]
pub fn card_description(children: Vec<Node>, tw: Option<String>) -> Node {
    let tw = tw_merge(DESCRIPTION_BASE_TW, tw.as_deref().unwrap_or(""));
    rsx! { <container tw={tw}>{children}</container> }
}

#[component("@ui/card")]
pub fn card_content(children: Vec<Node>, tw: Option<String>) -> Node {
    let tw = tw_merge(CONTENT_BASE_TW, tw.as_deref().unwrap_or(""));
    rsx! { <container tw={tw}>{children}</container> }
}

#[component("@ui/card")]
pub fn card_footer(children: Vec<Node>, tw: Option<String>) -> Node {
    let tw = tw_merge(FOOTER_BASE_TW, tw.as_deref().unwrap_or(""));
    rsx! { <container tw={tw}>{children}</container> }
}
