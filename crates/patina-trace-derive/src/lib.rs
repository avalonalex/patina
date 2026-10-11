//! `#[derive(Trace)]`: the trace of a Rust structure outside the heap that
//! holds values, generated from its fields (#686, `PRD/GC_PRD.md` §14).
//!
//! The trait, its implementations for the leaves and the examples are
//! `patina_core::Trace`'s; this crate only writes the impl. It takes every
//! field apart by name, as `scripts/check_gc_trace_names.py` asks of a trace
//! written by hand (#623), and traces each through its type's `Trace`, so a
//! field whose type has none does not compile. Two field attributes change
//! that:
//!
//! - `#[trace(skip, reason = "…")]` leaves the field out. The reason says what
//!   the field holds instead of a value, and is required.
//! - `#[trace(tail)]`, on an enum variant's `Box<Self>` field, continues the
//!   trace with the value it points to, in a loop rather than a recursive
//!   call, so a chain as deep as the program's recursion does not grow the
//!   native stack (#617). A variant has one at most.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{
    Data, DeriveInput, Field, Fields, GenericArgument, Ident, LitStr, PathArguments, Type,
    parse_macro_input, parse_quote,
};

#[proc_macro_derive(Trace, attributes(trace))]
pub fn derive_trace(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand(&input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// How the generated trace treats a field.
enum Kind {
    /// Traced through its type's `Trace`.
    Traced,
    /// Left out, with a reason.
    Skipped,
    /// The `Box<Self>` the trace continues with.
    Tail,
}

/// A struct's or a variant's fields, taken apart.
struct Shape {
    /// The pattern after the path: `{ a: __trace_0, b: _ }`, `(__trace_0, _)`,
    /// or nothing for a unit.
    pattern: TokenStream2,
    /// One trace call per traced field.
    traces: Vec<TokenStream2>,
    /// The binding of the tail field, if there is one.
    tail: Option<Ident>,
}

fn expand(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let body = match &input.data {
        Data::Struct(data) => {
            let shape = shape(&data.fields, name, false)?;
            let Shape {
                pattern, traces, ..
            } = shape;
            if traces.is_empty() {
                quote! {}
            } else {
                quote! {
                    let Self #pattern = self;
                    #(#traces)*
                }
            }
        }
        Data::Enum(data) => {
            let mut arms = Vec::new();
            let mut any_tail = false;
            for variant in &data.variants {
                let shape = shape(&variant.fields, name, true)?;
                any_tail |= shape.tail.is_some();
                arms.push((&variant.ident, shape));
            }
            if arms.is_empty() {
                quote! { match *self {} }
            } else if any_tail {
                // Each arm traces its fields, then gives the value its tail
                // points to, or returns: the chain is walked in a loop.
                let arms = arms.into_iter().map(|(variant, shape)| {
                    let Shape {
                        pattern,
                        traces,
                        tail,
                    } = shape;
                    match tail {
                        Some(tail) => quote! {
                            Self::#variant #pattern => {
                                #(#traces)*
                                &**#tail
                            }
                        },
                        None => quote! {
                            Self::#variant #pattern => {
                                #(#traces)*
                                return;
                            }
                        },
                    }
                });
                quote! {
                    let mut __node = self;
                    loop {
                        __node = match __node {
                            #(#arms)*
                        };
                    }
                }
            } else {
                let arms = arms.into_iter().map(|(variant, shape)| {
                    let Shape {
                        pattern, traces, ..
                    } = shape;
                    quote! {
                        Self::#variant #pattern => {
                            #(#traces)*
                        }
                    }
                });
                quote! {
                    match self {
                        #(#arms)*
                    }
                }
            }
        }
        Data::Union(data) => {
            return Err(syn::Error::new(
                data.union_token.span(),
                "a union cannot say which of its fields holds a value",
            ));
        }
    };

    let mut generics = input.generics.clone();
    for param in generics.type_params_mut() {
        param.bounds.push(parse_quote!(::patina_core::Trace));
    }
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();
    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics ::patina_core::Trace for #name #ty_generics #where_clause {
            fn trace<__V: ::patina_core::SlotVisitor + ?::core::marker::Sized>(
                &self,
                __visitor: &mut __V,
            ) {
                #body
            }
        }
    })
}

/// Take `fields` apart: a binding for each traced field, `_` for each
/// skipped one.
fn shape(fields: &Fields, name: &Ident, in_enum: bool) -> syn::Result<Shape> {
    let mut traces = Vec::new();
    let mut tail = None;
    let mut parts = Vec::new();
    for (index, field) in fields.iter().enumerate() {
        let binding = format_ident!("__trace_{}", index);
        let bound = match kind(field)? {
            Kind::Traced => {
                traces.push(quote! { ::patina_core::Trace::trace(#binding, __visitor); });
                true
            }
            Kind::Skipped => false,
            Kind::Tail => {
                if !in_enum {
                    return Err(syn::Error::new_spanned(
                        field,
                        "`tail` is for an enum's variants, whose chain ends at a variant without one",
                    ));
                }
                if !is_box_of_self(&field.ty, name) {
                    return Err(syn::Error::new_spanned(
                        &field.ty,
                        "a `tail` field is the `Box<Self>` the trace continues with",
                    ));
                }
                if tail.is_some() {
                    return Err(syn::Error::new_spanned(
                        field,
                        "a variant has one `tail` at most",
                    ));
                }
                tail = Some(binding.clone());
                true
            }
        };
        parts.push(match (&field.ident, bound) {
            (Some(ident), true) => quote! { #ident: #binding },
            (Some(ident), false) => quote! { #ident: _ },
            (None, true) => quote! { #binding },
            (None, false) => quote! { _ },
        });
    }
    let pattern = match fields {
        Fields::Named(_) => quote! { { #(#parts),* } },
        Fields::Unnamed(_) => quote! { ( #(#parts),* ) },
        Fields::Unit => quote! {},
    };
    Ok(Shape {
        pattern,
        traces,
        tail,
    })
}

/// A field's `#[trace(...)]` attributes, checked.
fn kind(field: &Field) -> syn::Result<Kind> {
    let mut skip: Option<Span> = None;
    let mut reason: Option<LitStr> = None;
    let mut tail: Option<Span> = None;
    for attr in &field.attrs {
        if !attr.path().is_ident("trace") {
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("skip") {
                if skip.replace(meta.path.span()).is_some() {
                    return Err(meta.error("`skip` given twice"));
                }
            } else if meta.path.is_ident("reason") {
                let value: LitStr = meta.value()?.parse()?;
                if value.value().trim().is_empty() {
                    return Err(syn::Error::new_spanned(
                        &value,
                        "the reason says what the field holds instead of a value",
                    ));
                }
                if reason.replace(value).is_some() {
                    return Err(meta.error("`reason` given twice"));
                }
            } else if meta.path.is_ident("tail") {
                if tail.replace(meta.path.span()).is_some() {
                    return Err(meta.error("`tail` given twice"));
                }
            } else {
                return Err(meta.error("expected `skip, reason = \"…\"` or `tail`"));
            }
            Ok(())
        })?;
    }
    match (skip, reason, tail) {
        (None, None, None) => Ok(Kind::Traced),
        (Some(_), Some(_), None) => Ok(Kind::Skipped),
        (None, None, Some(_)) => Ok(Kind::Tail),
        (Some(span), None, None) => Err(syn::Error::new(
            span,
            "`skip` needs `reason = \"…\"`: what the field holds instead of a value",
        )),
        (None, Some(reason), _) => {
            Err(syn::Error::new_spanned(reason, "`reason` goes with `skip`"))
        }
        (Some(span), _, Some(_)) => Err(syn::Error::new(
            span,
            "a field is skipped or a tail, not both",
        )),
    }
}

/// Whether `ty` is `Box<Self>` or `Box<Name<…>>`.
fn is_box_of_self(ty: &Type, name: &Ident) -> bool {
    let Type::Path(outer) = ty else {
        return false;
    };
    let Some(last) = outer.path.segments.last() else {
        return false;
    };
    if outer.qself.is_some() || last.ident != "Box" {
        return false;
    }
    let PathArguments::AngleBracketed(args) = &last.arguments else {
        return false;
    };
    let mut args = args.args.iter();
    let (Some(GenericArgument::Type(Type::Path(inner))), None) = (args.next(), args.next()) else {
        return false;
    };
    inner.qself.is_none()
        && inner
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "Self" || segment.ident == *name)
}

#[cfg(test)]
mod tests {
    use super::expand;

    /// The impl `expand` writes for `source`, with its tokens one space
    /// apart, or its error's message.
    fn expanded(source: &str) -> Result<String, String> {
        let input = syn::parse_str(source).expect("the test's input parses");
        expand(&input)
            .map(|tokens| tokens.to_string())
            .map_err(|error| error.to_string())
    }

    #[test]
    fn a_struct_is_taken_apart_by_name() {
        let out = expanded(
            r#"struct Frame<H> {
                procedure: TaggedValue,
                #[trace(skip, reason = "a frame depth")]
                depth: usize,
                handlers: Vec<H>,
            }"#,
        )
        .unwrap();
        assert!(out.contains(
            "let Self { procedure : __trace_0 , depth : _ , handlers : __trace_2 } = self ;"
        ));
        assert!(out.contains(":: patina_core :: Trace :: trace (__trace_0 , __visitor) ;"));
        assert!(out.contains(":: patina_core :: Trace :: trace (__trace_2 , __visitor) ;"));
        assert!(!out.contains("__trace_1"));
        // Each type parameter is bounded by `Trace`.
        assert!(out.contains("impl < H : :: patina_core :: Trace >"));
    }

    #[test]
    fn a_chain_is_walked_in_a_loop() {
        let out = expanded(
            r#"enum Chain {
                Halt,
                Wrap { value: TaggedValue, #[trace(tail)] next: Box<Chain> },
                Again(TaggedValue, #[trace(tail)] Box<Self>),
                Leaf { #[trace(skip, reason = "an id")] id: u64 },
            }"#,
        )
        .unwrap();
        assert!(out.contains("let mut __node = self ; loop { __node = match __node {"));
        assert!(out.contains("Self :: Halt => { return ; }"));
        assert!(out.contains(
            "Self :: Wrap { value : __trace_0 , next : __trace_1 } => { \
             :: patina_core :: Trace :: trace (__trace_0 , __visitor) ; & * * __trace_1 }"
        ));
        assert!(out.contains(
            "Self :: Again (__trace_0 , __trace_1) => { \
             :: patina_core :: Trace :: trace (__trace_0 , __visitor) ; & * * __trace_1 }"
        ));
        assert!(out.contains("Self :: Leaf { id : _ } => { return ; }"));
        // The tail is followed, never traced through `Trace`.
        assert!(!out.contains("trace (__trace_1"));
    }

    #[test]
    fn an_enum_without_a_tail_is_one_match() {
        let out = expanded("enum Handler { Plain(TaggedValue), None }").unwrap();
        assert!(!out.contains("loop"));
        assert!(out.contains("match self { Self :: Plain (__trace_0) => {"));
        assert!(out.contains("Self :: None => { }"));
    }

    #[test]
    fn a_skip_needs_its_reason() {
        let error = expanded("struct S { #[trace(skip)] depth: usize }").unwrap_err();
        assert!(error.contains("`skip` needs `reason"), "{error}");
        let error =
            expanded(r#"struct S { #[trace(skip, reason = "  ")] depth: usize }"#).unwrap_err();
        assert!(
            error.contains("the reason says what the field holds"),
            "{error}"
        );
        let error =
            expanded(r#"struct S { #[trace(reason = "a depth")] depth: usize }"#).unwrap_err();
        assert!(error.contains("`reason` goes with `skip`"), "{error}");
    }

    #[test]
    fn a_tail_is_an_enum_variants_box_of_itself() {
        let error = expanded("struct S { #[trace(tail)] next: Box<S> }").unwrap_err();
        assert!(
            error.contains("`tail` is for an enum's variants"),
            "{error}"
        );
        let error = expanded("enum E { A { #[trace(tail)] next: Box<Other> } }").unwrap_err();
        assert!(
            error.contains("the `Box<Self>` the trace continues with"),
            "{error}"
        );
        let error = expanded("enum E { A { #[trace(tail)] next: Vec<E> } }").unwrap_err();
        assert!(
            error.contains("the `Box<Self>` the trace continues with"),
            "{error}"
        );
        let error = expanded(
            "enum E { A { #[trace(tail)] first: Box<E>, #[trace(tail)] second: Box<E> } }",
        )
        .unwrap_err();
        assert!(error.contains("one `tail` at most"), "{error}");
        let error = expanded(r#"enum E { A { #[trace(skip, reason = "x", tail)] next: Box<E> } }"#)
            .unwrap_err();
        assert!(error.contains("skipped or a tail, not both"), "{error}");
    }

    #[test]
    fn an_unknown_attribute_and_a_union_are_refused() {
        let error = expanded("struct S { #[trace(pinned)] value: TaggedValue }").unwrap_err();
        assert!(error.contains("expected `skip"), "{error}");
        let error = expanded("union U { a: u64, b: f64 }").unwrap_err();
        assert!(error.contains("a union cannot say"), "{error}");
    }
}
