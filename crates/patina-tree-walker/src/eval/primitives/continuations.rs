//! Continuation primitives for (scheme base) and control operators
//!
//! Implements helper primitives for continuations:
//! - continuation? - Predicate for continuations
//! - make-continuation-prompt-tag - Create delimited continuation tags
//! - continuation-prompt-tag? - Predicate for prompt tags
//! - default-continuation-prompt-tag - Get the default prompt tag
//!
//! Note: The core continuation operators (call/cc, dynamic-wind, etc.) are
//! handled directly by the CPS evaluator via special CpsExpr forms, not as
//! primitives. This module only provides supporting predicates and constructors.

use super::super::error::EvalError;
use super::registry::PrimitiveRegistry;
use patina_core::cps_expr::PromptTag;
use patina_runtime::value::Value;
use std::rc::Rc;

/// Register continuation helper primitives
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    use super::super::EvalResult;
    use super::registry::PrimitiveFn;
    use patina_runtime::value::Arity;

    // continuation? - Type predicate
    registry.register(PrimitiveFn::new(
        "patina.internal.control",
        "continuation?",
        Arity::Exact(1),
        "Returns #t if obj is a continuation, #f otherwise.",
        |_eval, args, _| continuation_p(args).map(EvalResult::Value),
    ));

    // make-continuation-prompt-tag - Create a prompt tag
    registry.register(PrimitiveFn::new(
        "patina.internal.control",
        "make-continuation-prompt-tag",
        Arity::Range(0, 1),
        "Create a new continuation prompt tag, optionally with a name.",
        |_eval, args, _| make_continuation_prompt_tag(args).map(EvalResult::Value),
    ));

    // continuation-prompt-tag? - Type predicate
    registry.register(PrimitiveFn::new(
        "patina.internal.control",
        "continuation-prompt-tag?",
        Arity::Exact(1),
        "Returns #t if obj is a continuation prompt tag, #f otherwise.",
        |_eval, args, _| continuation_prompt_tag_p(args).map(EvalResult::Value),
    ));

    // default-continuation-prompt-tag - Get the default prompt tag
    registry.register(PrimitiveFn::new(
        "patina.internal.control",
        "default-continuation-prompt-tag",
        Arity::Exact(0),
        "Returns the default continuation prompt tag.",
        |_eval, _args, _| default_continuation_prompt_tag().map(EvalResult::Value),
    ));
}

/// Check if a value is a continuation
fn continuation_p(args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    Ok(Value::Boolean(matches!(args[0], Value::Continuation(_))))
}

/// Create a new continuation prompt tag
fn make_continuation_prompt_tag(args: Vec<Value>) -> Result<Value, EvalError> {
    let name = if args.is_empty() {
        "prompt".to_string()
    } else if args.len() == 1 {
        match &args[0] {
            Value::Symbol(s) => s.to_string(),
            Value::String(s) => s.borrow().clone(),
            _ => {
                return Err(EvalError::TypeError(
                    "make-continuation-prompt-tag: name must be a symbol or string".to_string(),
                ));
            }
        }
    } else {
        return Err(EvalError::WrongArity {
            expected: "0 or 1".to_string(),
            actual: args.len(),
        });
    };

    Ok(Value::ContinuationPromptTag(Rc::new(PromptTag::new(name))))
}

/// Check if a value is a continuation prompt tag
fn continuation_prompt_tag_p(args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    Ok(Value::Boolean(matches!(
        args[0],
        Value::ContinuationPromptTag(_)
    )))
}

/// Get the default continuation prompt tag
///
/// The default prompt tag is used when no explicit tag is provided.
/// Each call creates a new tag with the same name but unique ID.
/// Note: For full delimited continuation support, this should be a
/// thread-local singleton. See DELIMITED_CONTINUATIONS_DESIGN.md.
fn default_continuation_prompt_tag() -> Result<Value, EvalError> {
    Ok(Value::ContinuationPromptTag(Rc::new(PromptTag::new(
        "default",
    ))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_continuation_p() {
        // Non-continuations should return false
        assert!(matches!(
            continuation_p(vec![Value::Integer(42)]).unwrap(),
            Value::Boolean(false)
        ));
        assert!(matches!(
            continuation_p(vec![Value::Boolean(true)]).unwrap(),
            Value::Boolean(false)
        ));
    }

    #[test]
    fn test_make_continuation_prompt_tag() {
        // Create with default name
        let result = make_continuation_prompt_tag(vec![]).unwrap();
        assert!(matches!(result, Value::ContinuationPromptTag(_)));

        // Create with symbol name
        let result = make_continuation_prompt_tag(vec![Value::symbol("my-tag")]).unwrap();
        assert!(matches!(result, Value::ContinuationPromptTag(_)));
    }

    #[test]
    fn test_continuation_prompt_tag_p() {
        let tag = make_continuation_prompt_tag(vec![]).unwrap();
        assert!(matches!(
            continuation_prompt_tag_p(vec![tag]).unwrap(),
            Value::Boolean(true)
        ));
        assert!(matches!(
            continuation_prompt_tag_p(vec![Value::Integer(42)]).unwrap(),
            Value::Boolean(false)
        ));
    }
}
