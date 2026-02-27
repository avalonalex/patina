//! System introspection primitives
//!
//! R7RS §6.13 defines `features` for querying implementation capabilities.

use super::super::EvalResult;
use super::super::Evaluator;
use super::super::error::EvalError;
use super::registry::PrimitiveFn;
use super::registry::PrimitiveRegistry;
use patina_core::TaggedValue;
use patina_runtime::Arity;
use patina_runtime::default_features;

/// Register all system primitives in the registry
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "features",
        Arity::Exact(0),
        "Return a list of feature identifiers supported by this implementation (R7RS §6.13)",
        |eval, args, _| features(eval, args).map(EvalResult::Tagged),
    ));
}

/// Return a list of feature identifiers supported by this implementation.
///
/// R7RS §6.13: Returns a list of the feature identifiers which cond-expand
/// treats as true. It is an error to modify this list.
///
/// Example:
/// ```scheme
/// (features) => (aarch64 darwin exact-closed full-unicode ieee-float
///                little-endian macosx patina posix r7rs ratios unix)
/// ```
fn features(evaluator: &Evaluator, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "0".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let mut h = heap.borrow_mut();
    let feature_registry = default_features();
    let sym_tvs: Vec<TaggedValue> = feature_registry
        .all_features()
        .into_iter()
        .map(|name| h.intern_symbol(&name))
        .collect();
    let mut result = TaggedValue::NULL;
    for tv in sym_tvs.into_iter().rev() {
        result = h.alloc_pair(tv, result);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_features_returns_list() {
        let eval = Evaluator::new();
        let result_tv = features(&eval, vec![]).unwrap();
        let heap = eval.global_env.heap();

        // Should be a non-empty list
        assert!(result_tv.is_pair());

        // Walk the list and check for r7rs and patina symbols
        let mut has_r7rs = false;
        let mut has_patina = false;
        let mut current = result_tv;
        while !current.is_null() {
            let h = heap.borrow();
            let car = h.car(current);
            if let Some(name) = h.get_symbol_name(car) {
                if name == "r7rs" {
                    has_r7rs = true;
                }
                if name == "patina" {
                    has_patina = true;
                }
            }
            current = h.cdr(current);
        }

        assert!(has_r7rs, "features should include r7rs");
        assert!(has_patina, "features should include patina");
    }
}
