//! System introspection primitives
//!
//! R7RS §6.13 defines `features` for querying implementation capabilities.
use crate::registry::PrimitiveFn;
use crate::registry::PrimitiveRegistry;
use patina_core::TaggedValue;
use patina_runtime::Arity;
use patina_runtime::EvalError;
use patina_runtime::SharedHeap;
use patina_runtime::default_features;

/// Register all system primitives in the registry
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "features",
        Arity::Exact(0),
        "Return a list of feature identifiers supported by this implementation (R7RS §6.13)",
        features,
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
fn features(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "0".to_string(),
            actual: args.len(),
        });
    }

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
