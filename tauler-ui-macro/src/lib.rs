use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use rstml::node::{Infallible, Node, NodeAttribute, NodeElement};
use syn::{FnArg, ItemFn, LitStr, Pat, Type};

#[proc_macro_attribute]
pub fn component(attr: TokenStream, item: TokenStream) -> TokenStream {
    let path: Option<LitStr> = if attr.is_empty() {
        None
    } else {
        match syn::parse::<LitStr>(attr) {
            Ok(lit) => Some(lit),
            Err(e) => return e.to_compile_error().into(),
        }
    };
    let func = syn::parse_macro_input!(item as ItemFn);
    gen_component(path, func).into()
}

fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .filter(|seg| !seg.is_empty())
        .map(|seg| {
            let mut c = seg.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().to_string() + c.as_str(),
            }
        })
        .collect()
}

fn needs_serde_default(ty: &Type) -> bool {
    if let Type::Path(tp) = ty {
        if let Some(seg) = tp.path.segments.last() {
            return matches!(seg.ident.to_string().as_str(), "Option" | "Vec");
        }
    }
    false
}

fn gen_component(path: Option<LitStr>, func: ItemFn) -> TokenStream2 {
    let vis = &func.vis;
    let fn_name = &func.sig.ident;
    let fn_str = fn_name.to_string();
    let stmts = &func.block.stmts;

    let component_name = format_ident!("{}", to_pascal_case(&fn_str));
    let props_name = format_ident!("{}Props", component_name);

    let params: Vec<(syn::Ident, Type)> = func
        .sig
        .inputs
        .iter()
        .filter_map(|arg| {
            if let FnArg::Typed(pt) = arg {
                if let Pat::Ident(pi) = &*pt.pat {
                    return Some((pi.ident.clone(), (*pt.ty).clone()));
                }
            }
            None
        })
        .collect();

    let props_fields: Vec<TokenStream2> = params
        .iter()
        .map(|(name, ty)| {
            let default_attr = needs_serde_default(ty).then(|| quote! { #[serde(default)] });
            quote! { #default_attr pub #name: #ty, }
        })
        .collect();

    let param_names: Vec<&syn::Ident> = params.iter().map(|(n, _)| n).collect();

    let entry_code = path.map(|p| {
        let module_path_str = p.value();
        let export_name_str = component_name.to_string();
        let global_name_str = format!("__ui_{fn_str}");
        let register_fn = format_ident!("__register_{fn_str}");
        let entry_const = format_ident!("__UI_ENTRY_{}", fn_str.to_uppercase());
        quote! {
            fn #register_fn(ctx: &rquickjs::Ctx<'_>) -> rquickjs::Result<()> {
                use crate::ui::UiComponent as _;
                ctx.globals().set(#global_name_str, rquickjs::Function::new(ctx.clone(), #component_name::js_fn)?)
            }
            #vis const #entry_const: crate::ui::registry::UiEntry = crate::ui::registry::UiEntry {
                module_path: #module_path_str,
                export_name: #export_name_str,
                global_name: #global_name_str,
                register: #register_fn,
            };
        }
    });

    quote! {
        #[derive(::serde::Deserialize, Default)]
        #vis struct #props_name { #(#props_fields)* }

        #vis struct #component_name;

        impl crate::ui::UiComponent for #component_name {
            type Props = #props_name;
            fn render(props: #props_name) -> crate::ui::Node {
                let #props_name { #(#param_names),* } = props;
                #(#stmts)*
            }
        }

        #entry_code
    }
}

type ParsedNode = Node<Infallible>;
type ParsedElement = NodeElement<Infallible>;

/// JSX-like macro for building `tauler::ui::Node` trees.
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
pub fn rsx(input: TokenStream) -> TokenStream {
    let result = rstml::Parser::new(rstml::ParserConfig::default()).parse_recoverable(input);
    let (nodes_opt, diagnostics) = result.split();

    let error_tokens: TokenStream2 = diagnostics
        .into_iter()
        .map(|d| d.emit_as_expr_tokens())
        .collect();

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
    let name = el.name().to_string();
    if name
        .chars()
        .next()
        .map(|c| c.is_uppercase())
        .unwrap_or(false)
    {
        return gen_component_call(el);
    }
    match name.as_str() {
        "container" => gen_container(el),
        "text" => gen_text_el(el),
        "image" => gen_image_el(el),
        _ => {
            let msg = format!(
                "unknown ui element <{name}>; use container, text, image, or a PascalCase component"
            );
            quote! { compile_error!(#msg) }
        }
    }
}

fn gen_component_call(el: &ParsedElement) -> TokenStream2 {
    let name: proc_macro2::TokenStream = el.name().to_string().parse().unwrap();
    let children = gen_children(&el.children);
    let attr_entries: Vec<TokenStream2> = el
        .attributes()
        .iter()
        .filter_map(|attr| {
            if let NodeAttribute::Attribute(kv) = attr {
                let key = kv.key.to_string();
                if let Some(expr) = kv.value() {
                    return Some(quote! { #key: (#expr) });
                }
            }
            None
        })
        .collect();
    quote! {
        {
            let __children = #children;
            #name::render_from_value(serde_json::json!({
                #(#attr_entries,)*
                "children": __children
            }))
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
