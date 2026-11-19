//! The code that the derive writes for a [`Derived`] type: an impl of
//! `Literal`, and for a record an impl of `Record`.
//!
//! The code names the core as `::prodrome`. The core declares
//! `extern crate self as prodrome`, so the same path works inside it.

use proc_macro2::{Ident, TokenStream};
use quote::{format_ident, quote, ToTokens};

use crate::shape::{
    Absent, Build, Codec, Constructor, Derived, Field, Headed, Newtype, Record, Role, Shape,
    Variant,
};

/// The impls for `derived`.
pub fn expand(derived: &Derived) -> TokenStream {
    let ident = &derived.ident;
    match &derived.shape {
        Shape::Record(record) => expand_record(ident, record),
        Shape::Headed(headed) => expand_headed(ident, headed),
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
    let reads = reads(&record.constructor.fields);
    let build = build(
        &record.constructor.fields,
        &[],
        &quote!(Self),
        &record.build,
    );
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
            Self::#tag { #(#binds,)* .. } => (#table, ::std::vec![#(#prints),*]),
        }
    });
    let arms_read = variants.iter().map(|variant| {
        let tag = &variant.ident;
        let name = &variant.constructor.name;
        let reads = reads(&variant.constructor.fields);
        let build = build(
            &variant.constructor.fields,
            &[],
            &quote!(Self::#tag),
            &Build::Fields,
        );
        quote! {
            #name => {
                #reads
                #build
            }
        }
    });
    let what = format!("one of {ident}'s constructors");
    quote! {
        impl #codec::Sum for #ident {
            fn arm(&self) -> (#codec::Constructor, ::std::vec::Vec<#literal::Value>) {
                match self {
                    #(#arms_print)*
                }
            }

            fn from_arm(
                call: &#literal::Call,
            ) -> ::std::result::Result<Self, #literal::ProdromeError> {
                match call.name.as_str() {
                    #(#arms_read)*
                    other => ::std::result::Result::Err(#codec::no_arm(#what, other)),
                }
            }
        }

        impl #codec::Literal for #ident {
            const CONSTRUCTORS: &'static [#codec::Constructor] = &[#(#tables),*];

            fn to_value(&self) -> #literal::Value {
                let (constructor, values) = <Self as #codec::Sum>::arm(self);
                #codec::record(constructor, values)
            }

            fn from_value(
                value: &#literal::Value,
            ) -> ::std::result::Result<Self, #literal::ProdromeError> {
                <Self as #codec::Sum>::from_arm(#codec::call(value)?)
            }
        }
    }
}

fn expand_headed(ident: &Ident, headed: &Headed) -> TokenStream {
    let codec = codec();
    let literal = literal();
    let sum = &headed.sum.ty;
    let sum_member = &headed.sum.ident;
    let stored: Vec<(&Field, &String)> = headed
        .fields
        .iter()
        .filter_map(|field| match &field.role {
            Role::Stored(stored) => Some((field, &stored.name)),
            Role::Skipped => None,
        })
        .collect();
    let head_names = stored.iter().map(|(_, name)| name);
    let head = stored.iter().map(|(field, name)| {
        let member = &field.ident;
        let value = print(field, &quote!(&self.#member));
        quote!((#name, #value))
    });
    let checks = stored.iter().map(|(_, name)| {
        let message = format!(
            "the field `{name}` of `{ident}` is also a field of a variant of its sum: \
             give one of them another name with #[literal(rename = \"...\")]"
        );
        quote! {
            ::std::assert!(
                !#codec::names_field(<#sum as #codec::Literal>::CONSTRUCTORS, #name),
                #message
            );
        }
    });
    let reads = reads(&headed.fields);
    let sum_local = format_ident!("field_sum");
    let build = build(
        &headed.fields,
        &[Inserted {
            position: headed.sum.position,
            member: sum_member,
            local: sum_local.clone(),
        }],
        &quote!(Self),
        &headed.build,
    );
    quote! {
        const _: () = {
            #(#checks)*
        };

        impl #codec::Literal for #ident {
            const CONSTRUCTORS: &'static [#codec::Constructor] = {
                const ARMS: &[#codec::Constructor] = <#sum as #codec::Literal>::CONSTRUCTORS;
                const HEAD: &[&str] = &[#(#head_names),*];
                const NAMES: [&str; #codec::headed_len(HEAD, ARMS)] =
                    #codec::headed_names(HEAD, ARMS);
                const TABLE: [#codec::Constructor; ARMS.len()] =
                    #codec::headed_table(&NAMES, HEAD.len(), ARMS);
                &TABLE
            };

            fn to_value(&self) -> #literal::Value {
                let (constructor, values) = <#sum as #codec::Sum>::arm(&self.#sum_member);
                #codec::headed(::std::vec![#(#head),*], constructor, values)
            }

            fn from_value(
                value: &#literal::Value,
            ) -> ::std::result::Result<Self, #literal::ProdromeError> {
                let call = #codec::call(value)?;
                let #sum_local = <#sum as #codec::Sum>::from_arm(call)?;
                #reads
                #build
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

/// The statements that read each stored field of `fields` from `call`
/// into its local variable.
fn reads(fields: &[Field]) -> TokenStream {
    let codec = codec();
    let reads = fields.iter().enumerate().filter_map(|(index, field)| {
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
    quote!(#(#reads)*)
}

/// A field that a read sets apart from `fields`: the field `member`, after
/// the first `position` of `fields`, holding the local variable `local`.
struct Inserted<'a> {
    position: usize,
    member: &'a Ident,
    local: Ident,
}

/// How a read sets a member of the value that it builds.
enum Member {
    /// From the local variable that holds the member's read.
    Read(Ident),
    /// To the member type's `Default`: the member is skipped.
    Default,
}

/// The expression that builds the value `path` from the local variables of
/// `fields` and of `inserted`, in declaration order.
fn build(
    fields: &[Field],
    inserted: &[Inserted<'_>],
    path: &TokenStream,
    build: &Build,
) -> TokenStream {
    let mut members: Vec<(&Ident, Member)> = Vec::new();
    for (index, field) in fields.iter().enumerate() {
        for insert in inserted.iter().filter(|insert| insert.position == index) {
            members.push((insert.member, Member::Read(insert.local.clone())));
        }
        members.push((
            &field.ident,
            match field.role {
                Role::Stored(_) => Member::Read(local(index)),
                Role::Skipped => Member::Default,
            },
        ));
    }
    for insert in inserted
        .iter()
        .filter(|insert| insert.position == fields.len())
    {
        members.push((insert.member, Member::Read(insert.local.clone())));
    }
    match build {
        Build::Fields => {
            let members = members.iter().map(|(member, value)| match value {
                Member::Read(local) => quote!(#member: #local),
                Member::Default => quote!(#member: ::std::default::Default::default()),
            });
            quote!(::std::result::Result::Ok(#path { #(#members),* }))
        }
        Build::New(function) => {
            let locals = members.iter().filter_map(|(_, value)| match value {
                Member::Read(local) => Some(local),
                Member::Default => None,
            });
            quote!(#function(#(#locals),*))
        }
    }
}
