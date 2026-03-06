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
use crate::registry::PrimitiveRegistry;
use patina_core::TaggedValue;
use patina_core::cps_expr::PromptTag;
use patina_runtime::EvalError;
use patina_runtime::SharedHeap;
use std::rc::Rc;

/// Register continuation helper primitives
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    use crate::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // continuation? - Type predicate
    registry.register(PrimitiveFn::new_heap(
        "patina.internal.control",
        "continuation?",
        Arity::Exact(1),
        "Returns #t if obj is a continuation, #f otherwise.",
        continuation_p,
    ));

    // make-continuation-prompt-tag - Create a prompt tag
    registry.register(PrimitiveFn::new_heap(
        "patina.internal.control",
        "make-continuation-prompt-tag",
        Arity::Range(0, 1),
        "Create a new continuation prompt tag, optionally with a name.",
        make_continuation_prompt_tag,
    ));

    // continuation-prompt-tag? - Type predicate
    registry.register(PrimitiveFn::new_heap(
        "patina.internal.control",
        "continuation-prompt-tag?",
        Arity::Exact(1),
        "Returns #t if obj is a continuation prompt tag, #f otherwise.",
        continuation_prompt_tag_p,
    ));

    // default-continuation-prompt-tag - Get the default prompt tag
    registry.register(PrimitiveFn::new_heap(
        "patina.internal.control",
        "default-continuation-prompt-tag",
        Arity::Exact(0),
        "Returns the default continuation prompt tag.",
        default_continuation_prompt_tag,
    ));
}

/// Check if a value is a continuation
fn continuation_p(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    Ok(TaggedValue::boolean(heap.borrow().is_continuation(args[0])))
}

/// Create a new continuation prompt tag
fn make_continuation_prompt_tag(
    heap: &SharedHeap,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
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
    heap: &SharedHeap,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    Ok(TaggedValue::boolean(heap.borrow().is_prompt_tag(args[0])))
}

/// Get the default continuation prompt tag
///
/// The default prompt tag is used when no explicit tag is provided.
/// Each call creates a new tag with the same name but unique ID.
/// Note: For full delimited continuation support, this should be a
/// thread-local singleton. See DELIMITED_CONTINUATIONS_DESIGN.md.
fn default_continuation_prompt_tag(
    heap: &SharedHeap,
    _args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    Ok(heap
        .borrow_mut()
        .alloc_prompt_tag(Rc::new(PromptTag::new("default"))))
}
