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

/// The component's declared return type, as tokens usable from the generated impl.
///
/// `Node` and `Vec<Node>` are rewritten fully qualified: the macro keeps only a
/// function's body, so component modules never had to import `Node` and mostly
/// do not. Any other type is used verbatim — a component may return arbitrary
/// serialisable data, which is how `<I3Layout>` hands back panels *and* gaps.
fn output_type(output: &syn::ReturnType) -> TokenStream2 {
    let syn::ReturnType::Type(_, ty) = output else {
        return quote! { crate::ui::Node };
    };
    let Type::Path(p) = &**ty else {
        return quote! { #ty };
    };
    match p.path.segments.last() {
        Some(seg) if seg.ident == "Node" => quote! { crate::ui::Node },
        Some(seg) if seg.ident == "Vec" && vec_elem_is_node(seg) => {
            quote! { ::std::vec::Vec<crate::ui::Node> }
        }
        _ => quote! { #ty },
    }
}

fn vec_elem_is_node(seg: &syn::PathSegment) -> bool {
    let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
        return false;
    };
    args.args.iter().any(|a| {
        matches!(a, syn::GenericArgument::Type(Type::Path(p))
            if p.path.segments.last().is_some_and(|s| s.ident == "Node"))
    })
}

fn gen_component(path: Option<LitStr>, func: ItemFn) -> TokenStream2 {
    let vis = &func.vis;
    let fn_name = &func.sig.ident;
    let fn_str = fn_name.to_string();
    let stmts = &func.block.stmts;

    // A component declaring `-> Vec<Node>` emits siblings rather than one node;
    // any other type is returned as data (see `output_type`).
    let output_ty = output_type(&func.sig.output);

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

    // Two bindings for one component, because there are two JavaScript engines to reach
    // (ADR 0025). The rquickjs one registers a global in a QuickJS realm; the
    // wasm-bindgen one exports a function the browser glue assigns onto `globalThis`
    // under the same name. Both are generated here so neither can be forgotten, and
    // both are gated so neither crate has to carry the other's dependencies.
    let entry_code = path.map(|p| {
        let module_path_str = p.value();
        let export_name_str = component_name.to_string();
        let global_name_str = format!("__ui_{fn_str}");
        let register_fn = format_ident!("__register_{fn_str}");
        let entry_const = format_ident!("__UI_ENTRY_{}", fn_str.to_uppercase());
        let wasm_fn = format_ident!("__wasm_ui_{fn_str}");
        quote! {
            #[cfg(feature = "quickjs")]
            fn #register_fn(ctx: &rquickjs::Ctx<'_>) -> rquickjs::Result<()> {
                use crate::ui::UiComponent as _;
                ctx.globals().set(#global_name_str, rquickjs::Function::new(ctx.clone(), #component_name::js_fn)?)
            }
            #[cfg(feature = "quickjs")]
            #vis const #entry_const: crate::ui::registry::EsEntry = crate::ui::registry::EsEntry {
                module_path: #module_path_str,
                export_name: #export_name_str,
                global_name: #global_name_str,
                register: #register_fn,
            };

            #[cfg(target_arch = "wasm32")]
            #[::wasm_bindgen::prelude::wasm_bindgen(js_name = #global_name_str)]
            #vis fn #wasm_fn(
                props: ::wasm_bindgen::JsValue,
            ) -> ::std::result::Result<::wasm_bindgen::JsValue, ::wasm_bindgen::JsValue> {
                crate::ui::wasm_render::<#component_name>(props)
            }
        }
    });

    quote! {
        #[derive(::serde::Deserialize, Default)]
        #vis struct #props_name { #(#props_fields)* }

        #vis struct #component_name;

        impl crate::ui::UiComponent for #component_name {
            type Props = #props_name;
            type Output = #output_ty;
            fn render(props: #props_name) -> Self::Output {
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
/// Supports any lowercase HTML tag with a `class` attribute, and PascalCase components.
/// Block expressions `{expr}` in children accept either a single `Node` or a
/// `Vec<Node>` (spliced via `IntoNodes`).
///
/// Example:
/// ```ignore
/// ui! {
///     <div class={class}>
///         {props.children}
///     </div>
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
                quote! { crate::ui::Node::Text(#s.to_string()) }
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
    gen_element_node(el, &name)
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

fn get_class(el: &ParsedElement) -> TokenStream2 {
    for attr in el.attributes() {
        if let NodeAttribute::Attribute(kv) = attr {
            if kv.key.to_string() == "class" {
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
                        __children.push(crate::ui::Node::Text(#s.to_string()));
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

/// One shape for every lowercase tag.
///
/// There is no per-tag branch here on purpose: an element is a tag name, some styling
/// and some children, and which tags exist is the layout walker's business, not the
/// macro's. `src`/`width`/`height` are read for every tag and simply stay `None` on
/// the ones that never carry them.
fn gen_element_node(el: &ParsedElement, tag: &str) -> TokenStream2 {
    let class = get_class(el);
    let style = get_style(el);
    // Each takes an `Option<serde_json::Value>` and boxes it, so a component never has
    // to name the box. A Rust component only ever forwards handlers it was handed.
    let handler = |name: &str| {
        get_attr_expr(el, name)
            .map(|e| quote! { (#e).map(::std::boxed::Box::new) })
            .unwrap_or_else(|| quote! { None })
    };
    let on_click = handler("on_click");
    let on_drag = handler("on_drag");
    let children = gen_children(&el.children);
    let src = get_attr_expr(el, "src")
        .map(|e| quote! { Some((#e).to_string()) })
        .unwrap_or_else(|| quote! { None });
    let width = get_attr_expr(el, "width")
        .map(|e| quote! { Some(#e as f32) })
        .unwrap_or_else(|| quote! { None });
    let height = get_attr_expr(el, "height")
        .map(|e| quote! { Some(#e as f32) })
        .unwrap_or_else(|| quote! { None });

    quote! {
        crate::ui::Node::Element(crate::ui::ElementNode {
            tag: #tag.to_string(),
            class: #class,
            style: #style,
            on_click: #on_click,
            on_drag: #on_drag,
            src: #src,
            width: #width,
            height: #height,
            children: #children,
        })
    }
}
