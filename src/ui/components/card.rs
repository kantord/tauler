use crate::ui::{Node, component, rsx, tw_merge};

const BASE_TW: &str = "rounded-lg border border-border bg-card text-card-foreground px-3 py-[10px]";

#[component("@ui/card")]
pub fn card(children: Vec<Node>, tw: Option<String>) -> Node {
    let tw = tw_merge(BASE_TW, tw.as_deref().unwrap_or(""));
    rsx! {
        <container tw={tw}>
            {children}
        </container>
    }
}
