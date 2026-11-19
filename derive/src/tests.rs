//! The derive's refusals: each type that has no literal form is refused, and
//! the message says what to write instead. The compiler shows these
//! messages; the `compile_fail` examples on the derive's page check that
//! the compiler refuses such a type.

use syn::{parse_quote, DeriveInput};

use crate::shape::Derived;

fn refusal(input: &DeriveInput) -> String {
    match Derived::parse(input) {
        Ok(_) => panic!("the derive accepts `{}`", input.ident),
        Err(error) => error.to_string(),
    }
}

#[test]
fn a_variant_with_other_than_one_unnamed_field_is_refused() {
    for input in [
        parse_quote! { enum Shape { Circle(u32, u32) } },
        parse_quote! { enum Shape { Circle() } },
    ] {
        assert_eq!(
            refusal(&input),
            "a variant with unnamed fields holds exactly one record: write \
             `Circle(Record)`, or name the fields as `Circle { name: Type }`"
        );
    }
}

#[test]
fn a_variant_that_holds_a_record_takes_its_constructor() {
    assert_eq!(
        refusal(&parse_quote! {
            enum Proposal { #[literal(rename = "Mail")] Email(Draft) }
        }),
        "`Email` holds a record, and the record's constructor is the variant's: \
         remove `rename`, and rename the record instead"
    );
    assert_eq!(
        refusal(&parse_quote! { enum Proposal { Email(#[literal(default)] Draft) } }),
        "`default` does not apply to an unnamed field; it applies to a named field"
    );
}

#[test]
fn a_struct_with_two_unnamed_fields_is_refused() {
    assert_eq!(
        refusal(&parse_quote! { struct Pair(u32, u32); }),
        "a literal names each field of a constructor: give each field of `Pair` a \
         name, or keep one field to make `Pair` a newtype"
    );
}

#[test]
fn a_generic_type_is_refused() {
    assert_eq!(
        refusal(&parse_quote! { struct Wrapped<T> { value: T } }),
        "#[derive(Literal)] does not support a generic type: implement `Literal` for \
         `Wrapped` by hand, or derive it for a type without parameters"
    );
}

#[test]
fn two_constructors_with_one_name_are_refused() {
    assert_eq!(
        refusal(&parse_quote! {
            enum Verdict { Accept, #[literal(rename = "Accept")] Approve }
        }),
        "two constructors of `Verdict` are named `Accept`: a read could not tell them \
         apart, so give one of them another name with #[literal(rename = \"...\")]"
    );
}

#[test]
fn two_fields_with_one_name_are_refused() {
    assert_eq!(
        refusal(&parse_quote! {
            struct Review { by: String, #[literal(rename = "by")] author: String }
        }),
        "two fields of `Review` are stored as `by`: give one of them another name with \
         #[literal(rename = \"...\")]"
    );
}

#[test]
fn an_enum_without_variants_is_refused() {
    assert_eq!(
        refusal(&parse_quote! { enum Never {} }),
        "`Never` has no variant, so no value of it prints: add a variant"
    );
}

#[test]
fn a_union_is_refused() {
    assert_eq!(
        refusal(&parse_quote! { union Bits { a: u32, b: f32 } }),
        "a union has no literal form: make `Bits` an enum"
    );
}

#[test]
fn a_name_the_grammar_cannot_read_is_refused() {
    for input in [
        parse_quote! { #[literal(rename = "two words")] struct Review { by: String } },
        parse_quote! { #[literal(rename = "datetime")] struct Review { by: String } },
        parse_quote! { struct Review { #[literal(rename = "None")] by: String } },
        parse_quote! { struct Review { #[literal(rename = "1st")] by: String } },
    ] {
        assert!(
            refusal(&input).contains("cannot be a stored name"),
            "{}",
            refusal(&input)
        );
    }
}

#[test]
fn an_option_out_of_its_place_is_refused() {
    assert_eq!(
        refusal(&parse_quote! { #[literal(new = make)] enum Verdict { Accept } }),
        "`new` does not apply to an enum; it applies to a struct"
    );
    assert_eq!(
        refusal(&parse_quote! { struct Review { #[literal(new = make)] by: String } }),
        "`new` does not apply to a named field; it applies to a struct"
    );
    assert_eq!(
        refusal(&parse_quote! { struct Review { #[literal(skip, default)] by: String } }),
        "a skipped field is not in the literal: remove `rename`, `default` and `with`, \
         or remove `skip`"
    );
    assert_eq!(
        refusal(&parse_quote! { struct Review { #[literal(default, default)] by: String } }),
        "`default` is given twice"
    );
    assert!(
        refusal(&parse_quote! { struct Review { #[literal(flatten)] by: String } })
            .starts_with("unknown option of #[literal]")
    );
}

#[test]
fn every_form_with_a_literal_is_accepted() {
    for input in [
        parse_quote! { struct Review { by: String, #[literal(skip)] seen: bool } },
        parse_quote! { struct Marker; },
        parse_quote! { #[literal(new = Tag::new)] struct Tag(String); },
        parse_quote! { enum Verdict { Accept, Revise { reason: String } } },
        parse_quote! { enum Proposal { Email(Draft), Note { text: String }, Dropped } },
        parse_quote! { struct Raw { r#type: String } },
    ] {
        let input: DeriveInput = input;
        assert!(Derived::parse(&input).is_ok(), "{}", input.ident);
    }
}

#[test]
fn two_fields_marked_sum_are_refused() {
    assert_eq!(
        refusal(&parse_quote! {
            struct Line { #[literal(sum)] shape: Shape, #[literal(sum)] mood: Mood }
        }),
        "two fields of `Line` are marked `sum`, and only one variant can name a \
         constructor: mark one field `sum`, and make the other an ordinary field"
    );
}

#[test]
fn a_sum_field_takes_no_other_option() {
    assert_eq!(
        refusal(&parse_quote! { struct Line { #[literal(sum, default)] shape: Shape } }),
        "the field marked `sum` is read from the constructor's name: remove `rename`, \
         `default`, `skip` and `with` from it"
    );
    assert_eq!(
        refusal(&parse_quote! {
            #[literal(rename = "Entry")] struct Line { #[literal(sum)] shape: Shape }
        }),
        "the variants of the field marked `sum` name the constructors of `Line`: remove \
         `rename`, and rename a variant instead"
    );
    assert_eq!(
        refusal(&parse_quote! { enum Log { Line { #[literal(sum)] shape: Shape } } }),
        "a field of a variant cannot be marked `sum`: mark a field of a struct"
    );
}

/// A type that gives `crate = path` names the core only by that path, in
/// each form.
#[test]
fn the_crate_option_names_the_core_by_its_path() {
    let inputs: [DeriveInput; 4] = [
        parse_quote! { #[literal(crate = core_lib)] struct Review { by: String } },
        parse_quote! { #[literal(crate = core_lib)] enum Verdict { Accept, Revise { why: String } } },
        parse_quote! { #[literal(crate = core_lib)] struct Tag(String); },
        parse_quote! {
            #[literal(crate = core_lib)] struct Line { by: String, #[literal(sum)] shape: Shape }
        },
    ];
    for input in inputs {
        let derived = Derived::parse(&input).expect("a form with a literal");
        let code = crate::expand::expand(&derived).to_string();
        assert!(!code.contains("prodrome"), "{code}");
        assert!(code.contains("core_lib :: literal :: codec"), "{code}");
    }
}

#[test]
fn the_crate_option_applies_to_a_type_only() {
    assert_eq!(
        refusal(&parse_quote! { struct Review { #[literal(crate = core_lib)] by: String } }),
        "`crate` does not apply to a named field; it applies to a struct or an enum"
    );
    assert_eq!(
        refusal(&parse_quote! {
            #[literal(crate = core_lib, crate = core_lib)] struct Review { by: String }
        }),
        "`crate` is given twice"
    );
}
