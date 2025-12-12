//! Continuation primitives for (scheme base) and control operators
//!
//! Implements:
//! - call/cc (call-with-current-continuation) - Full continuation capture
//! - continuation? - Predicate for continuations
//! - make-continuation-prompt-tag - Create delimited continuation tags
//! - call-with-continuation-prompt - Establish prompt (reset)
//! - abort-current-continuation - Abort to prompt
//!
//! Note: These primitives require CPS evaluation mode to fully work.
//! In direct mode, call/cc will raise an error.

use super::super::error::EvalError;
use super::registry::PrimitiveRegistry;
use patina_core::cps_expr::PromptTag;
use patina_runtime::value::Value;
use std::rc::Rc;

/// Register all continuation primitives
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    use super::super::EvalResult;
    use super::registry::PrimitiveFn;
    use patina_runtime::value::Arity;

    // call/cc - Primary continuation capture mechanism
    // Note: This primitive is a placeholder that raises an error in direct mode.
    // In CPS mode, call/cc is handled specially by the CPS evaluator via CpsExpr::CallCC.
    // Library is patina.internal.control to match the internal library declaration.
    registry.register(PrimitiveFn::new(
        "patina.internal.control",
        "call/cc",
        Arity::Exact(1),
        "Capture the current continuation and pass it to the given procedure. \
         Requires CPS evaluation mode.",
        |_eval, args, _| call_cc_stub(args).map(EvalResult::Value),
    ));

    // call-with-current-continuation - Full name alias for call/cc
    registry.register(PrimitiveFn::new(
        "patina.internal.control",
        "call-with-current-continuation",
        Arity::Exact(1),
        "Capture the current continuation and pass it to the given procedure. \
         Requires CPS evaluation mode. Alias for call/cc.",
        |_eval, args, _| call_cc_stub(args).map(EvalResult::Value),
    ));

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

    // call-with-continuation-prompt - Establish a prompt (like reset)
    // Note: This primitive is a placeholder. In CPS mode, prompts are handled
    // by the CPS evaluator via CpsExpr::Prompt.
    registry.register(PrimitiveFn::new(
        "patina.internal.control",
        "call-with-continuation-prompt",
        Arity::Min(1),
        "Establish a continuation prompt and evaluate the thunk. \
         Requires CPS evaluation mode.",
        |_eval, args, _| call_with_continuation_prompt_stub(args).map(EvalResult::Value),
    ));

    // abort-current-continuation - Abort to nearest prompt
    // Note: This primitive is a placeholder. In CPS mode, abort is handled
    // by the CPS evaluator via CpsExpr::Abort.
    registry.register(PrimitiveFn::new(
        "patina.internal.control",
        "abort-current-continuation",
        Arity::Min(1),
        "Abort to the nearest continuation prompt with the given tag. \
         Requires CPS evaluation mode.",
        |_eval, args, _| abort_current_continuation_stub(args).map(EvalResult::Value),
    ));

    // dynamic-wind - Set up entry/exit handlers for dynamic extent
    // Note: In direct mode, this is a stub that raises an error.
    // In CPS mode, dynamic-wind is handled specially by the CPS evaluator.
    registry.register(PrimitiveFn::new(
        "patina.internal.control",
        "dynamic-wind",
        Arity::Exact(3),
        "Set up thunks to be called when entering/leaving a dynamic extent. \
         Requires CPS evaluation mode for proper continuation support.",
        |_eval, args, _| dynamic_wind_stub(args).map(EvalResult::Value),
    ));
}

/// Stub for call/cc in direct mode
///
/// In direct mode, we cannot capture the continuation because it's implicit
/// in the call stack. Users must use CPS mode for call/cc to work.
fn call_cc_stub(args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    // Check that the argument is a procedure
    match &args[0] {
        Value::Procedure(_) => {}
        _ => {
            return Err(EvalError::TypeError(
                "call/cc: argument must be a procedure".to_string(),
            ));
        }
    }

    // In direct mode, we can't capture the continuation
    Err(EvalError::InternalError(
        "call/cc requires CPS evaluation mode. \
         Use TreeWalker::new_with_cps() to enable continuation support."
            .to_string(),
    ))
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
fn default_continuation_prompt_tag() -> Result<Value, EvalError> {
    // Create the default tag - this should ideally be a singleton,
    // but for now we create a new one each time with a well-known name.
    // TODO: Use a thread-local singleton for the default tag
    Ok(Value::ContinuationPromptTag(Rc::new(PromptTag::new(
        "default",
    ))))
}

/// Stub for call-with-continuation-prompt in direct mode
fn call_with_continuation_prompt_stub(args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "at least 1".to_string(),
            actual: 0,
        });
    }

    // Check that the first argument is a procedure (thunk)
    match &args[0] {
        Value::Procedure(_) => {}
        _ => {
            return Err(EvalError::TypeError(
                "call-with-continuation-prompt: first argument must be a thunk".to_string(),
            ));
        }
    }

    // In direct mode, we can't properly handle prompts
    Err(EvalError::InternalError(
        "call-with-continuation-prompt requires CPS evaluation mode. \
         Use TreeWalker::new_with_cps() to enable continuation support."
            .to_string(),
    ))
}

/// Stub for abort-current-continuation in direct mode
fn abort_current_continuation_stub(args: Vec<Value>) -> Result<Value, EvalError> {
    if args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "at least 1".to_string(),
            actual: 0,
        });
    }

    // Check that the first argument is a prompt tag
    match &args[0] {
        Value::ContinuationPromptTag(_) => {}
        _ => {
            return Err(EvalError::TypeError(
                "abort-current-continuation: first argument must be a prompt tag".to_string(),
            ));
        }
    }

    // In direct mode, we can't abort to prompts
    Err(EvalError::InternalError(
        "abort-current-continuation requires CPS evaluation mode. \
         Use TreeWalker::new_with_cps() to enable continuation support."
            .to_string(),
    ))
}

/// Stub for dynamic-wind in direct mode
///
/// In direct mode, dynamic-wind can work for normal control flow but cannot
/// properly handle continuation jumps. We raise an error to avoid subtle bugs.
fn dynamic_wind_stub(args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 3 {
        return Err(EvalError::WrongArity {
            expected: "3".to_string(),
            actual: args.len(),
        });
    }

    // Check that all arguments are procedures
    for (i, arg) in args.iter().enumerate() {
        let name = match i {
            0 => "before",
            1 => "body",
            _ => "after",
        };
        if !matches!(arg, Value::Procedure(_)) {
            return Err(EvalError::TypeError(format!(
                "dynamic-wind: {} thunk must be a procedure, got {}",
                name,
                arg.type_name()
            )));
        }
    }

    // In direct mode, we can't properly handle continuation jumps
    Err(EvalError::InternalError(
        "dynamic-wind requires CPS evaluation mode for proper continuation support. \
         Use TreeWalker::new_with_cps() to enable continuation support."
            .to_string(),
    ))
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
