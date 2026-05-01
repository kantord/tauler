use crate::ui::{Node, component, rsx, cva::Cva};

const BADGE_VARIANTS: Cva = Cva {
    base: "inline-flex items-center rounded-full border px-[10px] py-[2px] text-[12px] font-semibold",
    variants: &[
        ("variant", &[
            ("default", "border-transparent bg-primary text-primary-foreground"),
            ("secondary", "border-transparent bg-secondary text-secondary-foreground"),
            ("destructive", "border-transparent bg-destructive text-destructive-foreground"),
            ("outline", "text-foreground"),
        ]),
    ],
    defaults: &[("variant", "default")],
};

/// A small inline label for status, category, or count.
/// Accepts a `variant` prop (`default`, `secondary`, `destructive`, `outline`)
/// and an optional `tw` prop for Tailwind overrides.
///
/// # JSX
/// ```jsx
/// <container tw="flex flex-row gap-[8px]">
///   <Badge><text>Default</text></Badge>
///   <Badge variant="secondary"><text>Secondary</text></Badge>
///   <Badge variant="destructive"><text>Destructive</text></Badge>
///   <Badge variant="outline"><text>Outline</text></Badge>
/// </container>
/// ```
///
/// # Shadcn
/// https://ui.shadcn.com/docs/components/badge
#[component("@ui/badge")]
pub fn badge(children: Vec<Node>, variant: Option<String>, tw: Option<String>) -> Node {
    let tw = BADGE_VARIANTS.resolve(&[("variant", variant.as_deref())], tw.as_deref().unwrap_or(""));
    rsx! {
        <container tw={tw}>
            {children}
        </container>
    }
}
