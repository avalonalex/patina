//! System introspection primitives
//!
//! R7RS §6.13 defines `features` for querying implementation capabilities.

use super::super::EvalResult;
use super::super::Evaluator;
use super::super::error::EvalError;
use super::registry::PrimitiveFn;
use super::registry::PrimitiveRegistry;
use patina_runtime::default_features;
use patina_runtime::value::{Arity, Value};

/// Register all system primitives in the registry
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "features",
        Arity::Exact(0),
        "Return a list of feature identifiers supported by this implementation (R7RS §6.13)",
        |eval, args, _| features(eval, args).map(EvalResult::Value),
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
fn features(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 0, "features")?;

    let feature_registry = default_features();
    let feature_list: Vec<Value> = feature_registry
        .all_features()
        .into_iter()
        .map(|name| Value::Symbol(name.into()))
        .collect();

    Ok(evaluator.list_from_vec(feature_list))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_features_returns_list() {
        let eval = Evaluator::new();
        let result = features(&eval, vec![]).unwrap();

        // Should be a list
        assert!(matches!(result, Value::Pair(_) | Value::Null));

        // Convert to vec for inspection
        let items = eval.list_to_vec(result, "test").unwrap();

        // Should contain r7rs and patina at minimum
        let has_r7rs = items
            .iter()
            .any(|v| matches!(v, Value::Symbol(s) if s.as_ref() == "r7rs"));
        let has_patina = items
            .iter()
            .any(|v| matches!(v, Value::Symbol(s) if s.as_ref() == "patina"));

        assert!(has_r7rs, "features should include r7rs");
        assert!(has_patina, "features should include patina");
    }
}
