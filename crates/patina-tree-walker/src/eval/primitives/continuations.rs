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

use super::super::Evaluator;
use super::super::error::EvalError;
use super::registry::PrimitiveRegistry;
use patina_core::TaggedValue;
use patina_core::cps_expr::PromptTag;
use std::rc::Rc;

/// Register continuation helper primitives
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    use super::super::EvalResult;
    use super::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // continuation? - Type predicate
    registry.register(PrimitiveFn::new_tagged(
        "patina.internal.control",
        "continuation?",
        Arity::Exact(1),
        "Returns #t if obj is a continuation, #f otherwise.",
        |eval, args, _| continuation_p(eval, args).map(EvalResult::Tagged),
    ));

    // make-continuation-prompt-tag - Create a prompt tag
    registry.register(PrimitiveFn::new_tagged(
        "patina.internal.control",
        "make-continuation-prompt-tag",
        Arity::Range(0, 1),
        "Create a new continuation prompt tag, optionally with a name.",
        |eval, args, _| make_continuation_prompt_tag(eval, args).map(EvalResult::Tagged),
    ));

    // continuation-prompt-tag? - Type predicate
    registry.register(PrimitiveFn::new_tagged(
        "patina.internal.control",
        "continuation-prompt-tag?",
        Arity::Exact(1),
        "Returns #t if obj is a continuation prompt tag, #f otherwise.",
        |eval, args, _| continuation_prompt_tag_p(eval, args).map(EvalResult::Tagged),
    ));

    // default-continuation-prompt-tag - Get the default prompt tag
    registry.register(PrimitiveFn::new_tagged(
        "patina.internal.control",
        "default-continuation-prompt-tag",
        Arity::Exact(0),
        "Returns the default continuation prompt tag.",
        |eval, args, _| default_continuation_prompt_tag(eval, args).map(EvalResult::Tagged),
    ));
}

/// Check if a value is a continuation
fn continuation_p(evaluator: &Evaluator, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    Ok(TaggedValue::boolean(heap.borrow().is_continuation(args[0])))
}

/// Create a new continuation prompt tag
fn make_continuation_prompt_tag(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    let heap = evaluator.global_env.heap();

    let name = if args.is_empty() {
        "prompt".to_string()
    } else if args.len() == 1 {
        let heap_ref = heap.borrow();
        if let Some(s) = heap_ref.get_string_contents(args[0]) {
            s
        } else if let Some(name) = heap_ref.get_symbol_or_identifier_name(args[0]) {
            name.to_string()
        } else {
            return Err(EvalError::TypeError(
                "make-continuation-prompt-tag: name must be a symbol or string".to_string(),
            ));
        }
    } else {
        return Err(EvalError::WrongArity {
            expected: "0 or 1".to_string(),
            actual: args.len(),
        });
    };

    Ok(heap
        .borrow_mut()
        .alloc_prompt_tag(Rc::new(PromptTag::new(name))))
}

/// Check if a value is a continuation prompt tag
fn continuation_prompt_tag_p(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    Ok(TaggedValue::boolean(heap.borrow().is_prompt_tag(args[0])))
}

/// Get the default continuation prompt tag
///
/// The default prompt tag is used when no explicit tag is provided.
/// Each call creates a new tag with the same name but unique ID.
/// Note: For full delimited continuation support, this should be a
/// thread-local singleton. See DELIMITED_CONTINUATIONS_DESIGN.md.
fn default_continuation_prompt_tag(
    evaluator: &Evaluator,
    _args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    let heap = evaluator.global_env.heap();
    Ok(heap
        .borrow_mut()
        .alloc_prompt_tag(Rc::new(PromptTag::new("default"))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::Evaluator;

    #[test]
    fn test_continuation_p() {
        let eval = Evaluator::new();
        // Non-continuations should return false
        let result = continuation_p(&eval, vec![TaggedValue::fixnum(42)]).unwrap();
        assert_eq!(result, TaggedValue::boolean(false));

        let result = continuation_p(&eval, vec![TaggedValue::TRUE]).unwrap();
        assert_eq!(result, TaggedValue::boolean(false));
    }

    #[test]
    fn test_make_continuation_prompt_tag() {
        let eval = Evaluator::new();
        // Create with default name
        let result_tv = make_continuation_prompt_tag(&eval, vec![]).unwrap();
        assert!(eval.global_env.heap().borrow().is_prompt_tag(result_tv));

        // Create with symbol name
        let sym = eval.global_env.heap().borrow_mut().intern_symbol("my-tag");
        let result_tv = make_continuation_prompt_tag(&eval, vec![sym]).unwrap();
        assert!(eval.global_env.heap().borrow().is_prompt_tag(result_tv));
    }

    #[test]
    fn test_continuation_prompt_tag_p() {
        let eval = Evaluator::new();
        let tag_tv = make_continuation_prompt_tag(&eval, vec![]).unwrap();
        let result = continuation_prompt_tag_p(&eval, vec![tag_tv]).unwrap();
        assert_eq!(result, TaggedValue::boolean(true));

        let result = continuation_prompt_tag_p(&eval, vec![TaggedValue::fixnum(42)]).unwrap();
        assert_eq!(result, TaggedValue::boolean(false));
    }
}
