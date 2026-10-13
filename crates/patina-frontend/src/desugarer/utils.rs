// DesugarError is large, but boxing it would add complexity for minimal benefit
#![allow(clippy::result_large_err)]

//! Utility functions for desugaring

use super::error::{DesugarError, Result};
use patina_core::{Cx, SpineEnd, Value};
use patina_ir::{Formals, ScopedParam, Symbol};
use patina_runtime::ScopeSet;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

// ============================================================================
// List and identifier utilities
// ============================================================================

/// A proper list's elements; an improper or circular one is an error.
pub fn list_to_vec_tagged<'gc>(value: Value<'gc>, cx: &Cx<'gc>) -> Result<Vec<Value<'gc>>> {
    match cx.spine(value) {
        (elements, SpineEnd::Null) => Ok(elements),
        // The tail as Scheme writes it (#457): its `{:?}` was
        // `TaggedValue::fixnum(3)`.
        (_, SpineEnd::Improper(tail)) => Err(DesugarError::ExpectedProperList(format!(
            "got an improper list ending with {}",
            cx.format(tail)
        ))),
        // Walked, this collected elements until memory ran out (#459).
        (_, SpineEnd::Circular) => Err(DesugarError::ExpectedProperList(format!(
            "got a circular list: {}",
            cx.format(value)
        ))),
    }
}

/// Convert Scheme formals to Formals enum
pub fn convert_formals_tagged<'gc>(formals: Value<'gc>, cx: &Cx<'gc>) -> Result<Formals> {
    // Fixed arity: ()
    if formals == Value::NULL {
        return Ok(Formals::Fixed(vec![]));
    }

    // Walked below, a circular list collected parameters until memory ran
    // out (#459).
    if cx.spine_is_circular(formals) {
        return Err(DesugarError::InvalidFormals(format!(
            "a parameter list cannot be circular: {}",
            cx.format(formals)
        )));
    }

    // Variadic: single symbol (args), or an identifier, whose scopes are kept
    if let Some((name, scopes)) = symbol_or_identifier(formals, cx) {
        return Ok(Formals::Variadic(ScopedParam::with_scopes(name, scopes)));
    }

    // Either proper list (fixed) or improper list (mixed)
    if formals.is_pair() {
        let mut params: Vec<ScopedParam> = Vec::new();
        let mut current = formals;

        loop {
            if current == Value::NULL {
                // Proper list - fixed arity
                check_no_duplicates_scoped(&params, "lambda")?;
                return Ok(Formals::Fixed(params));
            }

            // Fast path: native pair
            if current.is_pair() {
                let (car, cdr) = cx.get_pair(current);
                let Some((name, scopes)) = symbol_or_identifier(car, cx) else {
                    return Err(DesugarError::InvalidFormals(format!(
                        "Parameter must be a symbol, got {}",
                        cx.format(car)
                    )));
                };
                params.push(ScopedParam::with_scopes(name, scopes));
                current = cdr;
                continue;
            }

            // Improper list terminator (not a pair): a symbol or an
            // identifier, mixed arity: (x y . rest)
            let Some((rest_name, rest_scopes)) = symbol_or_identifier(current, cx) else {
                return Err(DesugarError::InvalidFormals(format!(
                    "a rest parameter must be a symbol, got {}",
                    cx.format(current)
                )));
            };
            check_no_duplicates_scoped(&params, "lambda")?;
            if binds_identifier(&params, &rest_name, &rest_scopes) {
                return Err(DesugarError::DuplicateParameter {
                    name: rest_name.to_string(),
                    context: "lambda".to_string(),
                });
            }
            return Ok(Formals::Mixed {
                fixed: params,
                rest: ScopedParam::with_scopes(rest_name, rest_scopes),
            });
        }
    }

    Err(DesugarError::InvalidFormals(format!(
        "expected a symbol or a list of symbols, got {}",
        cx.format(formals)
    )))
}

/// Strip identifiers from a datum, replacing them with plain symbols.
///
/// In quoted data, identifiers from macro expansion should become plain symbols.
/// Identifier scopes are only needed during desugaring for binding resolution,
/// not in quoted output data.
pub fn strip_identifiers_tagged<'gc>(
    tv: Value<'gc>,
    cx: &mut Cx<'gc>,
    memo: &mut HashMap<Value<'gc>, Value<'gc>>,
) -> Value<'gc> {
    cx.map_syntax_identifiers_memo(tv, memo, |cx, _, name, _| Some(cx.intern_symbol(&name)))
}

/// A symbol or a scoped identifier as `(name, scopes)`.
///
/// The one answer to "what is this identifier called and what scopes does it
/// carry" — `Desugarer::identifier_of` is this function. Worth having exactly
/// one: a copy of it that dropped the scopes is what let a recursive macro's
/// per-expansion definitions collapse onto a single binding.
pub fn symbol_or_identifier<'gc>(tv: Value<'gc>, cx: &Cx<'gc>) -> Option<(Symbol, ScopeSet)> {
    if let Some(s) = cx.get_symbol_name(tv) {
        return Some((Rc::from(&*s), ScopeSet::new()));
    }
    cx.get_identifier_data_any(tv)
}

/// Parse define function syntax: `(name formals...)`.
///
/// Returns (name, name_scopes, formals) where formals is the rest of the list.
pub fn parse_define_function_tagged<'gc>(
    pattern: Value<'gc>,
    cx: &Cx<'gc>,
) -> Result<(Symbol, ScopeSet, Value<'gc>)> {
    // Fast path: native pair
    if pattern.is_pair() {
        let (car, cdr) = cx.get_pair(pattern);

        let (name, scopes) = symbol_or_identifier(car, cx).ok_or_else(|| {
            DesugarError::InvalidSyntax("define function name must be a symbol".to_string())
        })?;

        return Ok((name, scopes, cdr));
    }

    // Not a native pair — error
    Err(DesugarError::InvalidSyntax(
        "define function requires (name params...) pattern".to_string(),
    ))
}

/// Check for duplicate parameters.
///
/// Two parameters collide only when they share a name *and* a scope set. A
/// recursive macro that introduces the same template identifier on each
/// expansion produces several params spelled alike, but each carries the
/// distinct scope of the expansion that introduced it, so they are separate
/// bindings — comparing names alone would reject hygienic code such as:
///
/// ```scheme
/// (define-syntax gen
///   (syntax-rules ()
///     ((_ () (args ...)) (lambda (args ...) (list args ...)))
///     ((_ (x . rest) (args ...)) (gen rest (args ... a)))))
/// ((gen (1 2) ()) 10 20)  ;; => (10 20)
/// ```
pub fn check_no_duplicates_scoped(params: &[ScopedParam], context: &str) -> Result<()> {
    let mut seen = HashSet::new();
    for param in params {
        if !seen.insert((param.name.as_ref(), &param.scopes)) {
            return Err(DesugarError::DuplicateParameter {
                name: param.name.to_string(),
                context: context.to_string(),
            });
        }
    }
    Ok(())
}

/// Whether `params` already binds the identifier `(name, scopes)`.
///
/// The rest parameter of an improper formals list is checked against the fixed
/// params with the same name-plus-scopes rule as [`check_no_duplicates_scoped`].
fn binds_identifier(params: &[ScopedParam], name: &str, scopes: &ScopeSet) -> bool {
    params
        .iter()
        .any(|p| p.name.as_ref() == name && &p.scopes == scopes)
}

/// Extract all parameter names from a Formals structure
///
/// Used for tracking which names are shadowed by lambda parameters,
/// so they are not treated as macro calls.
pub fn formals_to_binders(formals: &Formals) -> Vec<(Rc<str>, ScopeSet)> {
    let binder = |p: &patina_ir::ScopedParam| (p.name.clone(), p.scopes.clone());
    match formals {
        Formals::Fixed(params) => params.iter().map(binder).collect(),
        Formals::Variadic(p) => vec![binder(p)],
        Formals::Mixed { fixed, rest } => {
            let mut binders: Vec<(Rc<str>, ScopeSet)> = fixed.iter().map(binder).collect();
            binders.push(binder(rest));
            binders
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run `f` in a context over a fresh heap.
    fn with_cx<R>(f: impl for<'gc> FnOnce(&mut Cx<'gc>) -> R) -> R {
        patina_core::NoGcScope::with_cx(&patina_core::new_shared_heap(), f)
    }

    #[test]
    fn stripping_quoted_identifiers_copies_only_changed_pairs() {
        with_cx(|cx| {
            let identifier = cx.alloc_identifier(Rc::from("x"), ScopeSet::new());
            let unchanged_tail = cx.alloc_pair(Value::fixnum(2), Value::NULL);
            let changed = cx.alloc_pair(identifier, unchanged_tail);
            let original = cx.alloc_pair(Value::fixnum(1), changed);
            let stripped = strip_identifiers_tagged(original, cx, &mut HashMap::new());
            assert_ne!(stripped, original);
            assert_eq!(cx.car(stripped), Value::fixnum(1));
            let changed = cx.cdr(stripped);
            let head = cx.car(changed);
            assert_eq!(cx.get_symbol_name(head).as_deref(), Some("x"));
            assert_eq!(cx.cdr(changed), unchanged_tail);
            let original_rest = cx.cdr(original);
            assert_eq!(cx.car(original_rest), identifier);
        });
    }

    #[test]
    fn stripping_a_quoted_improper_tail_converts_its_identifier() {
        with_cx(|cx| {
            let identifier = cx.alloc_identifier(Rc::from("tail"), ScopeSet::new());
            let original = cx.alloc_pair(Value::fixnum(1), identifier);
            let stripped = strip_identifiers_tagged(original, cx, &mut HashMap::new());
            let tail = cx.cdr(stripped);
            assert_eq!(cx.get_symbol_name(tail).as_deref(), Some("tail"));
            assert_eq!(cx.cdr(original), identifier);
        });
    }

    #[test]
    fn stripping_nested_pairs_and_vectors_preserves_unchanged_cycles() {
        with_cx(|cx| {
            let identifier = cx.alloc_identifier(Rc::from("nested"), ScopeSet::new());
            let cycle = cx.alloc_pair(Value::fixnum(7), Value::NULL);
            cx.set_cdr(cycle, cycle);
            let pair = cx.alloc_pair(identifier, cycle);
            let vector = cx.alloc_vector(vec![pair, cycle]);
            let original = cx.alloc_pair(vector, cycle);
            let stripped = strip_identifiers_tagged(original, cx, &mut HashMap::new());
            let vector = cx.car(stripped);
            assert_ne!(vector, cx.car(original));
            let copied = cx.vector_ref(vector, 0);
            let head = cx.car(copied);
            assert_eq!(cx.get_symbol_name(head).as_deref(), Some("nested"));
            assert_eq!(cx.cdr(copied), cycle);
            assert_eq!(cx.vector_ref(vector, 1), cycle);
            assert_eq!(cx.cdr(stripped), cycle);
            assert_eq!(cx.cdr(cycle), cycle);
            let original_vector = cx.car(original);
            let original_pair = cx.vector_ref(original_vector, 0);
            assert_eq!(cx.car(original_pair), identifier);
        });
    }
}
