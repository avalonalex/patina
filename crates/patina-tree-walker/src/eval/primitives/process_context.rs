//! Process context primitives
//!
//! R7RS (scheme process-context) library procedures:
//! - command-line: Get command line arguments
//! - exit: Exit program (with handlers)
//! - emergency-exit: Exit immediately
//! - get-environment-variable: Get single env var
//! - get-environment-variables: Get all env vars

use super::super::EvalResult;
use super::super::Evaluator;
use super::super::error::EvalError;
use super::registry::PrimitiveFn;
use super::registry::PrimitiveRegistry;
use patina_core::TaggedValue;
use patina_runtime::Arity;

/// Register all process-context primitives in the registry
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    registry.register(PrimitiveFn::new_tagged(
        "scheme.process-context",
        "command-line",
        Arity::Exact(0),
        "Return the command line arguments as a list of strings",
        |eval, args, _| command_line(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.process-context",
        "exit",
        Arity::Range(0, 1),
        "Exit the program with optional status",
        |eval, args, _| exit_proc(eval, args),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.process-context",
        "emergency-exit",
        Arity::Range(0, 1),
        "Exit immediately without running handlers",
        |eval, args, _| emergency_exit(eval, args),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.process-context",
        "get-environment-variable",
        Arity::Exact(1),
        "Get the value of an environment variable",
        |eval, args, _| get_environment_variable(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.process-context",
        "get-environment-variables",
        Arity::Exact(0),
        "Get all environment variables as an alist",
        |eval, args, _| get_environment_variables(eval, args).map(EvalResult::Tagged),
    ));
}

/// Return the command line arguments as a list of strings.
///
/// The first element is the program name (implementation-dependent).
fn command_line(evaluator: &Evaluator, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "0".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let mut h = heap.borrow_mut();
    // Allocate each arg string, then build a proper list in reverse
    let arg_tvs: Vec<TaggedValue> = std::env::args().map(|s| h.alloc_string(s)).collect();
    let mut result = TaggedValue::NULL;
    for tv in arg_tvs.into_iter().rev() {
        result = h.alloc_pair(tv, result);
    }
    Ok(result)
}

/// Exit the program with optional status.
///
/// R7RS says this should run dynamic-wind handlers first, but since we don't
/// have dynamic-wind fully implemented, we just exit.
///
/// - No argument or #t: exit with success (0)
/// - #f: exit with failure (1)
/// - Integer: exit with that code
fn exit_proc(_evaluator: &Evaluator, args: Vec<TaggedValue>) -> Result<EvalResult, EvalError> {
    let code = if args.is_empty() {
        0 // Success
    } else {
        exit_code_from_arg(args[0])?
    };

    // Note: In a full implementation, we would run dynamic-wind handlers here
    std::process::exit(code);
}

/// Exit immediately without running handlers.
///
/// This corresponds to _exit() in POSIX.
fn emergency_exit(_evaluator: &Evaluator, args: Vec<TaggedValue>) -> Result<EvalResult, EvalError> {
    let code = if args.is_empty() {
        0 // Success
    } else {
        exit_code_from_arg(args[0])?
    };

    std::process::exit(code);
}

/// Extract an exit code from a TaggedValue argument
fn exit_code_from_arg(arg: TaggedValue) -> Result<i32, EvalError> {
    if arg == TaggedValue::TRUE {
        Ok(0)
    } else if arg == TaggedValue::FALSE {
        Ok(1)
    } else if arg.is_fixnum() {
        Ok(arg.as_fixnum_unchecked() as i32)
    } else {
        Err(EvalError::TypeError(
            "exit expects boolean or integer".to_string(),
        ))
    }
}

/// Get the value of an environment variable.
///
/// Returns #f if the variable is not set.
fn get_environment_variable(
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

    let name: String = {
        let heap_ref = heap.borrow();
        match heap_ref.get_string_contents(args[0]) {
            Some(s) => s,
            None => {
                return Err(EvalError::TypeError(
                    "get-environment-variable expects a string".to_string(),
                ));
            }
        }
    };

    match std::env::var(&name) {
        Ok(value) => Ok(heap.borrow_mut().alloc_string(value)),
        Err(_) => Ok(TaggedValue::FALSE),
    }
}

/// Get all environment variables as an alist.
///
/// Each entry is (name . value) where both are strings.
fn get_environment_variables(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "0".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let mut h = heap.borrow_mut();
    // Build alist entries as (name . value) pairs
    let entry_tvs: Vec<TaggedValue> = std::env::vars()
        .map(|(k, v)| {
            let key = h.alloc_string(k);
            let val = h.alloc_string(v);
            h.alloc_pair(key, val)
        })
        .collect();
    // Build proper list in reverse
    let mut result = TaggedValue::NULL;
    for tv in entry_tvs.into_iter().rev() {
        result = h.alloc_pair(tv, result);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alloc_string(s: &str, eval: &Evaluator) -> TaggedValue {
        eval.global_env
            .heap()
            .borrow_mut()
            .alloc_string(s.to_string())
    }

    #[test]
    fn test_command_line_returns_list() {
        let eval = Evaluator::new();
        let result = command_line(&eval, vec![]).unwrap();
        // Should be a list (null or pair)
        assert!(result.is_null() || result.is_pair());
    }

    #[test]
    fn test_get_environment_variable_path() {
        let eval = Evaluator::new();
        let name = alloc_string("PATH", &eval);
        let result = get_environment_variable(&eval, vec![name]).unwrap();
        // PATH is usually set — should return a string
        assert!(result.is_string());
    }

    #[test]
    fn test_get_environment_variable_not_found() {
        let eval = Evaluator::new();
        let name = alloc_string("PATINA_NONEXISTENT_VAR_12345", &eval);
        let result = get_environment_variable(&eval, vec![name]).unwrap();
        assert_eq!(result, TaggedValue::FALSE);
    }

    #[test]
    fn test_get_environment_variables_returns_alist() {
        let eval = Evaluator::new();
        let result = get_environment_variables(&eval, vec![]).unwrap();
        // Should be a list
        assert!(result.is_null() || result.is_pair());

        // If non-empty, first entry should be a pair (alist entry)
        if !result.is_null() {
            let heap = eval.global_env.heap();
            let heap_ref = heap.borrow();
            let car = heap_ref.car(result);
            assert!(car.is_pair());
        }
    }
}
