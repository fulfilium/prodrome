//! The code that the derive writes for a [`Derived`] type: an impl of
//! `Literal`, and for a record an impl of `Record`.
//!
//! The code names the core as `::prodrome`. The core declares
//! `extern crate self as prodrome`, so the same path works inside it.

use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote, ToTokens};

use crate::shape::{
    Absent, Build, Codec, Constructor, Derived, Field, Newtype, Record, Role, Shape, Variant,
};

/// The impls for `derived`.
pub fn expand(derived: &Derived) -> TokenStream {
    let ident = &derived.ident;
    match &derived.shape {
        Shape::Record(record) => expand_record(ident, record),
        Shape::Sum(variants) => expand_sum(ident, variants),
        Shape::Newtype(newtype) => expand_newtype(ident, newtype),
    }
}

fn codec() -> TokenStream {
    quote!(::prodrome::literal::codec)
}

fn literal() -> TokenStream {
    quote!(::prodrome::literal)
}

fn expand_record(ident: &Ident, record: &Record) -> TokenStream {
    let codec = codec();
    let literal = literal();
    let name = &record.constructor.name;
    let table = table(&record.constructor);
    let prints = record.constructor.fields.iter().filter_map(|field| {
        let member = &field.ident;
        print(field, &quote!(&self.#member))
    });
    let (reads, build) = read(&record.constructor, &quote!(Self), &record.build);
    quote! {
        impl #codec::Record for #ident {
            const CONSTRUCTOR: #codec::Constructor = #table;

            fn values(&self) -> ::std::vec::Vec<#literal::Value> {
                ::std::vec![#(#prints),*]
            }

            fn from_call(
                call: &#literal::Call,
            ) -> ::std::result::Result<Self, #literal::ProdromeError> {
                #reads
                #build
            }
        }

        impl #codec::Literal for #ident {
            const CONSTRUCTORS: &'static [#codec::Constructor] =
                &[<Self as #codec::Record>::CONSTRUCTOR];

            fn to_value(&self) -> #literal::Value {
                #codec::record(
                    <Self as #codec::Record>::CONSTRUCTOR,
                    <Self as #codec::Record>::values(self),
                )
            }

            fn from_value(
                value: &#literal::Value,
            ) -> ::std::result::Result<Self, #literal::ProdromeError> {
                <Self as #codec::Record>::from_call(#codec::call_of(value, #name)?)
            }
        }
    }
}

fn expand_sum(ident: &Ident, variants: &[Variant]) -> TokenStream {
    let codec = codec();
    let literal = literal();
    let tables = variants.iter().map(|variant| table(&variant.constructor));
    let arms_print = variants.iter().map(|variant| {
        let tag = &variant.ident;
        let table = table(&variant.constructor);
        let binds = bindings(&variant.constructor);
        let prints = variant
            .constructor
            .fields
            .iter()
            .enumerate()
            .filter_map(|(index, field)| print(field, &local(index).to_token_stream()));
        quote! {
            Self::#tag { #(#binds,)* .. } => #codec::record(#table, ::std::vec![#(#prints),*]),
        }
    });
    let arms_read = variants.iter().map(|variant| {
        let tag = &variant.ident;
        let name = &variant.constructor.name;
        let (reads, build) = read(&variant.constructor, &quote!(Self::#tag), &Build::Fields);
        quote! {
            #name => {
                #reads
                #build
            }
        }
    });
    let what = format!("one of {ident}'s constructors");
    quote! {
        impl #codec::Literal for #ident {
            const CONSTRUCTORS: &'static [#codec::Constructor] = &[#(#tables),*];

            fn to_value(&self) -> #literal::Value {
                match self {
                    #(#arms_print)*
                }
            }

            fn from_value(
                value: &#literal::Value,
            ) -> ::std::result::Result<Self, #literal::ProdromeError> {
                let call = #codec::call(value)?;
                match call.name.as_str() {
                    #(#arms_read)*
                    other => ::std::result::Result::Err(#codec::no_arm(#what, other)),
                }
            }
        }
    }
}

fn expand_newtype(ident: &Ident, newtype: &Newtype) -> TokenStream {
    let codec = codec();
    let literal = literal();
    let inner = &newtype.inner;
    let build = match &newtype.build {
        Build::Fields => quote!(::std::result::Result::Ok(Self(inner))),
        Build::New(path) => quote!(#path(inner)),
    };
    quote! {
        impl #codec::Literal for #ident {
            fn to_value(&self) -> #literal::Value {
                <#inner as #codec::Literal>::to_value(&self.0)
            }

            fn from_value(
                value: &#literal::Value,
            ) -> ::std::result::Result<Self, #literal::ProdromeError> {
                let inner = <#inner as #codec::Literal>::from_value(value)?;
                #build
            }

            fn from_field(
                value: &#literal::Value,
                context: &dyn ::std::fmt::Display,
            ) -> ::std::result::Result<Self, #literal::ProdromeError> {
                let inner = <#inner as #codec::Literal>::from_field(value, context)?;
                #build
            }
        }
    }
}

/// The constructor as a `Constructor`: its name, and its stored fields in
/// print order.
fn table(constructor: &Constructor) -> TokenStream {
    let name = &constructor.name;
    let fields = constructor
        .fields
        .iter()
        .filter_map(|field| match &field.role {
            Role::Stored(stored) => Some(&stored.name),
            Role::Skipped => None,
        });
    quote!((#name, &[#(#fields),*]))
}

/// The local variable that holds the field at `index` of a constructor.
fn local(index: usize) -> Ident {
    format_ident!("field_{index}")
}

/// The pattern that binds each field of a variant to its local variable.
fn bindings(constructor: &Constructor) -> impl Iterator<Item = TokenStream> + '_ {
    constructor.fields.iter().enumerate().map(|(index, field)| {
        let member = &field.ident;
        let local = local(index);
        quote!(#member: #local)
    })
}

/// The value of a stored field, printed from the reference `place`; none
/// for a skipped field.
fn print(field: &Field, place: &TokenStream) -> Option<TokenStream> {
    let codec = codec();
    let ty = &field.ty;
    match &field.role {
        Role::Stored(stored) => Some(match &stored.codec {
            Codec::Literal => quote!(<#ty as #codec::Literal>::to_value(#place)),
            Codec::With(path) => quote!(#path::to_value(#place)),
        }),
        Role::Skipped => None,
    }
}

/// The statements that read each stored field of `constructor` from
/// `call` into its local variable, and the expression that builds the
/// value `path` from them.
fn read(
    constructor: &Constructor,
    path: &TokenStream,
    build: &Build,
) -> (TokenStream, TokenStream) {
    let codec = codec();
    let reads = constructor
        .fields
        .iter()
        .enumerate()
        .filter_map(|(index, field)| {
            let stored = match &field.role {
                Role::Stored(stored) => stored,
                Role::Skipped => return None,
            };
            let ty = &field.ty;
            let name = &stored.name;
            let reader = match &stored.codec {
                Codec::Literal => quote!(<#ty as #codec::Literal>::from_field),
                Codec::With(module) => quote!(#module::from_field),
            };
            let local = local(index);
            Some(match stored.absent {
                Absent::Refused => quote!(let #local = #codec::read(call, #name, #reader)?;),
                Absent::Default => {
                    quote!(let #local = #codec::read_or_default(call, #name, #reader)?;)
                }
            })
        });
    let reads = quote!(#(#reads)*);
    let build = match build {
        Build::Fields => {
            let members = constructor.fields.iter().enumerate().map(|(index, field)| {
                let member = &field.ident;
                match field.role {
                    Role::Stored(_) => {
                        let local = local(index);
                        quote!(#member: #local)
                    }
                    Role::Skipped => quote!(#member: ::std::default::Default::default()),
                }
            });
            quote!(::std::result::Result::Ok(#path { #(#members),* }))
        }
        Build::New(function) => {
            let locals = constructor
                .fields
                .iter()
                .enumerate()
                .filter_map(|(index, field)| match field.role {
                    Role::Stored(_) => Some(local(index)),
                    Role::Skipped => None,
                });
            quote!(#function(#(#locals),*))
        }
    };
    (reads, build)
}
