//! System introspection primitives
//!
//! R7RS §6.13 defines `features` for querying implementation capabilities.
use crate::registry::PrimitiveFn;
use crate::registry::PrimitiveRegistry;
use patina_core::TaggedValue;
use patina_runtime::Arity;
use patina_runtime::EvalError;
use patina_runtime::SharedHeap;

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
fn features(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "0".to_string(),
            actual: args.len(),
        });
    }

    let mut h = heap.borrow_mut();
    // The heap's own registry, not `default_features()`: R7RS §4.2.1 defines
    // this as "a list of the feature identifiers which `cond-expand` treats as
    // true", and `cond-expand` reads the same field. Building a fresh default
    // here is how the two came to disagree about the backend identifier.
    let sym_tvs: Vec<TaggedValue> = h
        .features()
        .all_features()
        .into_iter()
        .map(|name| h.intern_symbol(&name))
        .collect();
    Ok(h.list_from_iter(sym_tvs))
}
