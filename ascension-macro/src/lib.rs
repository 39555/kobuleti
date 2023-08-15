
use proc_macro;
use proc_macro2;
use syn;
use proc_macro::TokenStream;
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{DeriveInput, Fields, Field};

#[proc_macro_derive(DisplayOnlyIdents)]
pub fn display_only_idents(input: TokenStream) -> TokenStream {
    // Parse the string representation
    let ast = syn::parse_macro_input!(input as  DeriveInput);
    match ast.data {
        syn::Data::Enum(ref enum_data) => {
            impl_display(&ast, enum_data).into()
        }
        _ => panic!("#[derive(DisplayEnumOnlyNames)] works only on enums"),
    }
}

fn impl_display(ast: &syn::DeriveInput, data: &syn::DataEnum) -> proc_macro2::TokenStream {
    let (impl_generics, ty_generics, where_clause) = ast.generics.split_for_impl();
    let name = &ast.ident;
    let variants = data
        .variants
        .iter()
        .map(|variant| { 
            let id = &variant.ident;
            let fields_in_variant = match &variant.fields {
                    Fields::Unnamed(_) => quote_spanned! {variant.span()=> (..)},
                    Fields::Unit       => quote_spanned! { variant.span()=> },
                    Fields::Named(_)   => quote_spanned! {variant.span()=> {..} },
            };
            quote! {
                #name::#id #fields_in_variant => { 
                    f.write_str(concat!(stringify!(#name),"::",stringify!(#id)))
                },
            }
        });
    
    quote! {
        impl #impl_generics Display for #name #ty_generics #where_clause{
            fn fmt(&self, f: &mut ::std::fmt::Formatter) -> ::std::result::Result<(), ::std::fmt::Error> {
                match *self {
                    #(#variants)*
                }
            }
        }
    }
}

