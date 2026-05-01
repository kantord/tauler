use crate::ui::{Node, component, rsx, tw_merge};

const BASE_TW: &str = "rounded-lg border border-border bg-card text-card-foreground px-3 py-[10px]";

/// A styled container with rounded corners, a border, and card background colour.
/// Wraps arbitrary child nodes and accepts an optional `tw` prop for Tailwind overrides.
///
/// # JSX
/// ```jsx
/// <Card tw="flex flex-col">
///   <text tw="text-foreground text-[14px] font-bold">System Status</text>
///   <text tw="text-muted-foreground text-[12px]">All services operational</text>
/// </Card>
/// ```
///
/// # Shadcn
/// https://ui.shadcn.com/docs/components/card
#[component("@ui/card")]
pub fn card(children: Vec<Node>, tw: Option<String>) -> Node {
    let tw = tw_merge(BASE_TW, tw.as_deref().unwrap_or(""));
    rsx! {
        <container tw={tw}>
            {children}
        </container>
    }
}
