use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields, ImplItem, ItemImpl};

#[proc_macro_attribute]
pub fn lifecycle_trace(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(item as ItemImpl);

    let is_lifecycle_impl = input
        .trait_
        .as_ref()
        .and_then(|(_, path, _)| path.segments.last())
        .map(|seg| seg.ident == "Lifecycle")
        .unwrap_or(false);

    if !is_lifecycle_impl {
        return syn::Error::new_spanned(
            &input,
            "#[lifecycle_trace] can only be applied to `impl Lifecycle for T` blocks",
        )
        .to_compile_error()
        .into();
    }

    let entering: ImplItem = syn::parse_quote! {
        fn wrap_enter(self, ctx: &mut Self::Context, output: &mut Self::Output) -> Result<Self::State, Self::Error> {
            let _lc = self.lifecycle_context();
            let _key = self.key();
            let _meta = serde_json::to_string(&_lc.metadata).unwrap_or_default();
            let result = self.enter(ctx, output);
            match &result {
                Ok(_) => tracing::info!(key = ?_key, display_name = %_lc.display_name, metadata = %_meta, "entering"),
                Err(e) => tracing::error!(key = ?_key, display_name = %_lc.display_name, metadata = %_meta, error = %e, "entering failed"),
            }
            result
        }
    };

    let reconciling: ImplItem = syn::parse_quote! {
        fn wrap_reconcile(self, state: &mut Self::State, ctx: &mut Self::Context, output: &mut Self::Output) -> Result<(), Self::Error> {
            let _lc = self.lifecycle_context();
            let _key = self.key();
            let _meta = serde_json::to_string(&_lc.metadata).unwrap_or_default();
            let result = self.reconcile_self(state, ctx, output);
            if let Err(e) = &result {
                tracing::error!(key = ?_key, display_name = %_lc.display_name, metadata = %_meta, error = %e, "reconciling failed");
            }
            result
        }
    };

    let exiting: ImplItem = syn::parse_quote! {
        fn wrap_exit(state: Self::State, ctx: &mut Self::Context, output: &mut Self::Output) -> Result<(), Self::Error> {
            let _lc = Self::lifecycle_state_context(&state);
            let _meta = serde_json::to_string(&_lc.metadata).unwrap_or_default();
            let result = Self::exit(state, ctx, output);
            match &result {
                Ok(_) => tracing::info!(display_name = %_lc.display_name, metadata = %_meta, "exiting"),
                Err(e) => tracing::error!(display_name = %_lc.display_name, metadata = %_meta, error = %e, "exiting failed"),
            }
            result
        }
    };

    input.items.push(entering);
    input.items.push(reconciling);
    input.items.push(exiting);

    quote! { #input }.into()
}

#[proc_macro_derive(Ephemeral, attributes(reconciler))]
pub fn derive_ephemeral(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    let fields = match &input.data {
        Data::Struct(s) => match &s.fields {
            Fields::Named(f) => &f.named,
            _ => panic!("Ephemeral only supports named fields"),
        },
        _ => panic!("Ephemeral only supports structs"),
    };

    let mut calls = vec![];
    for field in fields {
        let field_name = field.ident.as_ref().unwrap();
        for attr in &field.attrs {
            if !attr.path().is_ident("reconciler") {
                continue;
            }
            let mut output_ident: Option<syn::Ident> = None;
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("output") {
                    if let syn::Expr::Path(p) = meta.value()?.parse::<syn::Expr>()? {
                        output_ident = p.path.get_ident().cloned();
                    }
                }
                Ok(())
            })
            .unwrap();
            let output = output_ident.expect("reconciler attribute requires output = <field_name>");
            calls.push(quote! {
                { let mut __ctx = ::core::default::Default::default(); self.#field_name.reconcile(::std::vec::Vec::new(), &mut __ctx, &mut self.#output); }
            });
        }
    }

    quote! {
        impl #impl_generics Drop for #name #ty_generics #where_clause {
            fn drop(&mut self) { #(#calls)* }
        }
    }
    .into()
}
