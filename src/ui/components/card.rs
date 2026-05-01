use crate::ui::{Node, component, rsx, cva::Cva};

const CARD_VARIANTS: Cva = Cva {
    base: "rounded-lg border border-border bg-card text-card-foreground px-3 py-[10px]",
    variants: &[],
    defaults: &[],
};

const CARD_HEADER_VARIANTS: Cva = Cva {
    base: "flex flex-col gap-[4px]",
    variants: &[],
    defaults: &[],
};

const CARD_TITLE_VARIANTS: Cva = Cva {
    base: "font-semibold text-[14px] leading-none tracking-tight",
    variants: &[],
    defaults: &[],
};

const CARD_DESCRIPTION_VARIANTS: Cva = Cva {
    base: "text-[12px] text-muted-foreground",
    variants: &[],
    defaults: &[],
};

const CARD_CONTENT_VARIANTS: Cva = Cva {
    base: "flex flex-col",
    variants: &[],
    defaults: &[],
};

const CARD_FOOTER_VARIANTS: Cva = Cva {
    base: "flex flex-row items-center",
    variants: &[],
    defaults: &[],
};

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
    rsx! { <container tw={tw}>{children}</container> }
}

#[component("@ui/card")]
pub fn card_header(children: Vec<Node>, tw: Option<String>) -> Node {
    let tw = CARD_HEADER_VARIANTS.resolve(&[], tw.as_deref().unwrap_or(""));
    rsx! { <container tw={tw}>{children}</container> }
}

#[component("@ui/card")]
pub fn card_title(children: Vec<Node>, tw: Option<String>) -> Node {
    let tw = CARD_TITLE_VARIANTS.resolve(&[], tw.as_deref().unwrap_or(""));
    rsx! { <container tw={tw}>{children}</container> }
}

#[component("@ui/card")]
pub fn card_description(children: Vec<Node>, tw: Option<String>) -> Node {
    let tw = CARD_DESCRIPTION_VARIANTS.resolve(&[], tw.as_deref().unwrap_or(""));
    rsx! { <container tw={tw}>{children}</container> }
}

#[component("@ui/card")]
pub fn card_content(children: Vec<Node>, tw: Option<String>) -> Node {
    let tw = CARD_CONTENT_VARIANTS.resolve(&[], tw.as_deref().unwrap_or(""));
    rsx! { <container tw={tw}>{children}</container> }
}

#[component("@ui/card")]
pub fn card_footer(children: Vec<Node>, tw: Option<String>) -> Node {
    let tw = CARD_FOOTER_VARIANTS.resolve(&[], tw.as_deref().unwrap_or(""));
    rsx! { <container tw={tw}>{children}</container> }
}
