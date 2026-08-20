use crate::ui::{component, cva::Cva, rsx, Node};

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
/// Wraps arbitrary child nodes and accepts an optional `class` attribute for Tailwind overrides.
///
/// # JSX
/// ```jsx
/// <Card class="flex flex-col gap-[6px]">
///   <CardHeader>
///     <CardTitle><span>System Status</span></CardTitle>
///     <CardDescription><span>All services operational</span></CardDescription>
///   </CardHeader>
///   <CardContent>
///     <span class="text-foreground text-[12px]">nginx · postgres · redis</span>
///   </CardContent>
/// </Card>
/// ```
///
/// # Shadcn
/// https://ui.shadcn.com/docs/components/card
#[component("@ui/card")]
pub fn card(children: Vec<Node>, class: Option<String>) -> Node {
    let class = CARD_VARIANTS.resolve(&[], class.as_deref().unwrap_or(""));
    rsx! { <div class={class}>{children}</div> }
}

#[component("@ui/card")]
pub fn card_header(children: Vec<Node>, class: Option<String>) -> Node {
    let class = CARD_HEADER_VARIANTS.resolve(&[], class.as_deref().unwrap_or(""));
    rsx! { <div class={class}>{children}</div> }
}

#[component("@ui/card")]
pub fn card_title(children: Vec<Node>, class: Option<String>) -> Node {
    let class = CARD_TITLE_VARIANTS.resolve(&[], class.as_deref().unwrap_or(""));
    rsx! { <div class={class}>{children}</div> }
}

#[component("@ui/card")]
pub fn card_description(children: Vec<Node>, class: Option<String>) -> Node {
    let class = CARD_DESCRIPTION_VARIANTS.resolve(&[], class.as_deref().unwrap_or(""));
    rsx! { <div class={class}>{children}</div> }
}

#[component("@ui/card")]
pub fn card_content(children: Vec<Node>, class: Option<String>) -> Node {
    let class = CARD_CONTENT_VARIANTS.resolve(&[], class.as_deref().unwrap_or(""));
    rsx! { <div class={class}>{children}</div> }
}

#[component("@ui/card")]
pub fn card_footer(children: Vec<Node>, class: Option<String>) -> Node {
    let class = CARD_FOOTER_VARIANTS.resolve(&[], class.as_deref().unwrap_or(""));
    rsx! { <div class={class}>{children}</div> }
}
