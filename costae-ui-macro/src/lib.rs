use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use rstml::node::{Infallible, Node, NodeAttribute, NodeElement};

type ParsedNode = Node<Infallible>;
type ParsedElement = NodeElement<Infallible>;

/// JSX-like macro for building `costae::ui::Node` trees.
///
/// Supports `<container>`, `<text>`, and `<image>` tags with a `tw` attribute.
/// Block expressions `{expr}` in children accept either a single `Node` or a
/// `Vec<Node>` (spliced via `IntoNodes`).
///
/// Example:
/// ```ignore
/// ui! {
///     <container tw={tw}>
///         {props.children}
///     </container>
/// }
/// ```
#[proc_macro]
pub fn ui(input: TokenStream) -> TokenStream {
    let result = rstml::Parser::new(rstml::ParserConfig::default()).parse_recoverable(input);
    let (nodes_opt, diagnostics) = result.split();

    let error_tokens: TokenStream2 = diagnostics.into_iter().map(|d| d.emit_as_expr_tokens()).collect();

    let nodes = nodes_opt.unwrap_or_default();
    let node_tokens = match nodes.as_slice() {
        [single] => gen_node(single),
        [] => quote! { compile_error!("ui! requires a root element") },
        _ => quote! { compile_error!("ui! requires exactly one root element") },
    };

    quote! { { #[allow(unused_braces)] { #error_tokens #node_tokens } } }.into()
}

fn gen_node(node: &ParsedNode) -> TokenStream2 {
    match node {
        Node::Element(el) => gen_element(el),
        Node::Block(block) => quote! { #block },
        Node::Text(t) => {
            let s = t.value_string();
            if s.trim().is_empty() {
                quote! {}
            } else {
                quote! {
                    crate::ui::Node::Text(crate::ui::TextNode { tw: None, text: #s.to_string() })
                }
            }
        }
        _ => quote! {},
    }
}

fn gen_element(el: &ParsedElement) -> TokenStream2 {
    match el.name().to_string().as_str() {
        "container" => gen_container(el),
        "text" => gen_text_el(el),
        "image" => gen_image_el(el),
        name => {
            let msg = format!("unknown ui element <{}>; use container, text, or image", name);
            quote! { compile_error!(#msg) }
        }
    }
}

fn get_tw(el: &ParsedElement) -> TokenStream2 {
    for attr in el.attributes() {
        if let NodeAttribute::Attribute(kv) = attr {
            if kv.key.to_string() == "tw" {
                if let Some(expr) = kv.value() {
                    return quote! { Some((#expr).to_string()) };
                }
            }
        }
    }
    quote! { None }
}

fn get_attr_expr<'a>(el: &'a ParsedElement, name: &str) -> Option<&'a syn::Expr> {
    el.attributes().iter().find_map(|attr| {
        if let NodeAttribute::Attribute(kv) = attr {
            if kv.key.to_string() == name {
                return kv.value();
            }
        }
        None
    })
}

fn gen_children(children: &[ParsedNode]) -> TokenStream2 {
    let pushes: Vec<TokenStream2> = children
        .iter()
        .filter_map(|child| match child {
            Node::Element(_) => {
                let n = gen_node(child);
                Some(quote! { __children.extend(crate::ui::IntoNodes::into_nodes(#n)); })
            }
            Node::Block(block) => {
                Some(quote! { __children.extend(crate::ui::IntoNodes::into_nodes(#block)); })
            }
            Node::Text(t) => {
                let s = t.value_string();
                if s.trim().is_empty() {
                    None
                } else {
                    Some(quote! {
                        __children.push(crate::ui::Node::Text(crate::ui::TextNode {
                            tw: None,
                            text: #s.to_string(),
                        }));
                    })
                }
            }
            _ => None,
        })
        .collect();

    quote! {
        {
            let mut __children: Vec<crate::ui::Node> = Vec::new();
            #(#pushes)*
            __children
        }
    }
}

fn get_style(el: &ParsedElement) -> TokenStream2 {
    match get_attr_expr(el, "style") {
        Some(expr) => quote! { #expr },
        None => quote! { None },
    }
}

fn gen_container(el: &ParsedElement) -> TokenStream2 {
    let tw = get_tw(el);
    let style = get_style(el);
    let children = gen_children(&el.children);
    quote! {
        crate::ui::Node::Container(crate::ui::ContainerNode {
            tw: #tw,
            style: #style,
            children: #children,
        })
    }
}

fn gen_text_el(el: &ParsedElement) -> TokenStream2 {
    let tw = get_tw(el);
    let parts: Vec<TokenStream2> = el
        .children
        .iter()
        .filter_map(|child| match child {
            Node::Text(t) => {
                let s = t.value_string();
                if s.trim().is_empty() {
                    None
                } else {
                    Some(quote! { __text.push_str(#s); })
                }
            }
            Node::Block(block) => Some(quote! { __text.push_str(&format!("{}", #block)); }),
            _ => None,
        })
        .collect();

    quote! {
        crate::ui::Node::Text(crate::ui::TextNode {
            tw: #tw,
            text: {
                let mut __text = String::new();
                #(#parts)*
                __text
            },
        })
    }
}

fn gen_image_el(el: &ParsedElement) -> TokenStream2 {
    let tw = get_tw(el);
    let src = get_attr_expr(el, "src")
        .map(|e| quote! { (#e).to_string() })
        .unwrap_or_else(|| quote! { compile_error!("<image> requires a src attribute") });
    let width = get_attr_expr(el, "width")
        .map(|e| quote! { Some(#e as f32) })
        .unwrap_or_else(|| quote! { None });
    let height = get_attr_expr(el, "height")
        .map(|e| quote! { Some(#e as f32) })
        .unwrap_or_else(|| quote! { None });

    quote! {
        crate::ui::Node::Image(crate::ui::ImageNode {
            tw: #tw,
            src: #src,
            width: #width,
            height: #height,
        })
    }
}
