use crate::ui::{component, cva::Cva, rsx, Node};

const BADGE_VARIANTS: Cva = Cva {
    base:
        "inline-flex items-center rounded-full border px-[10px] py-[2px] text-[12px] font-semibold",
    variants: &[(
        "variant",
        &[
            (
                "default",
                "border-transparent bg-primary text-primary-foreground",
            ),
            (
                "secondary",
                "border-transparent bg-secondary text-secondary-foreground",
            ),
            (
                "destructive",
                "border-transparent bg-destructive text-destructive-foreground",
            ),
            ("outline", "text-foreground"),
        ],
    )],
    defaults: &[("variant", "default")],
};

/// A small inline label for status, category, or count.
///
/// # JSX
/// ```jsx
/// <div class="flex flex-row gap-[8px]">
///   <Badge><span>Default</span></Badge>
///   <Badge variant="secondary"><span>Secondary</span></Badge>
///   <Badge variant="destructive"><span>Destructive</span></Badge>
///   <Badge variant="outline"><span>Outline</span></Badge>
/// </div>
/// ```
///
/// # Shadcn
/// https://ui.shadcn.com/docs/components/badge
#[component("@ui/badge")]
pub fn badge(children: Vec<Node>, variant: Option<String>, class: Option<String>) -> Node {
    let class = BADGE_VARIANTS.resolve(
        &[("variant", variant.as_deref())],
        class.as_deref().unwrap_or(""),
    );
    rsx! {
        <div class={class}>
            {children}
        </div>
    }
}
