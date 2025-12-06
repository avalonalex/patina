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
use patina_runtime::value::{Arity, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Register all process-context primitives in the registry
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    registry.register(PrimitiveFn::new(
        "scheme.process-context",
        "command-line",
        Arity::Exact(0),
        "Return the command line arguments as a list of strings",
        |eval, args, _| command_line(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.process-context",
        "exit",
        Arity::Range(0, 1),
        "Exit the program with optional status",
        |eval, args, _| exit_proc(eval, args),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.process-context",
        "emergency-exit",
        Arity::Range(0, 1),
        "Exit immediately without running handlers",
        |eval, args, _| emergency_exit(eval, args),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.process-context",
        "get-environment-variable",
        Arity::Exact(1),
        "Get the value of an environment variable",
        |eval, args, _| get_environment_variable(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.process-context",
        "get-environment-variables",
        Arity::Exact(0),
        "Get all environment variables as an alist",
        |eval, args, _| get_environment_variables(eval, args).map(EvalResult::Value),
    ));
}

/// Return the command line arguments as a list of strings.
///
/// The first element is the program name (implementation-dependent).
fn command_line(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 0, "command-line")?;

    let args: Vec<Value> = std::env::args()
        .map(|s| Value::String(Rc::new(RefCell::new(s))))
        .collect();

    Ok(evaluator.list_from_vec(args))
}

/// Exit the program with optional status.
///
/// R7RS says this should run dynamic-wind handlers first, but since we don't
/// have dynamic-wind fully implemented, we just exit.
///
/// - No argument or #t: exit with success (0)
/// - #f: exit with failure (1)
/// - Integer: exit with that code
fn exit_proc(_evaluator: &Evaluator, args: Vec<Value>) -> Result<EvalResult, EvalError> {
    let code = if args.is_empty() {
        0 // Success
    } else {
        match &args[0] {
            Value::Boolean(true) => 0,
            Value::Boolean(false) => 1,
            Value::Integer(n) => *n as i32,
            _ => {
                return Err(EvalError::TypeError(
                    "exit expects boolean or integer".to_string(),
                ));
            }
        }
    };

    // Note: In a full implementation, we would run dynamic-wind handlers here
    std::process::exit(code);
}

/// Exit immediately without running handlers.
///
/// This corresponds to _exit() in POSIX.
fn emergency_exit(_evaluator: &Evaluator, args: Vec<Value>) -> Result<EvalResult, EvalError> {
    let code = if args.is_empty() {
        0 // Success
    } else {
        match &args[0] {
            Value::Boolean(true) => 0,
            Value::Boolean(false) => 1,
            Value::Integer(n) => *n as i32,
            _ => {
                return Err(EvalError::TypeError(
                    "emergency-exit expects boolean or integer".to_string(),
                ));
            }
        }
    };

    std::process::exit(code);
}

/// Get the value of an environment variable.
///
/// Returns #f if the variable is not set.
fn get_environment_variable(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "get-environment-variable")?;

    let name = match &args[0] {
        Value::String(s) => s.borrow().clone(),
        _ => {
            return Err(EvalError::TypeError(
                "get-environment-variable expects a string".to_string(),
            ));
        }
    };

    match std::env::var(&name) {
        Ok(value) => Ok(Value::String(Rc::new(RefCell::new(value)))),
        Err(_) => Ok(Value::Boolean(false)),
    }
}

/// Get all environment variables as an alist.
///
/// Each entry is (name . value) where both are strings.
fn get_environment_variables(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 0, "get-environment-variables")?;

    let entries: Vec<Value> = std::env::vars()
        .map(|(k, v)| {
            Value::Pair(Rc::new(RefCell::new((
                Value::String(Rc::new(RefCell::new(k))),
                Value::String(Rc::new(RefCell::new(v))),
            ))))
        })
        .collect();

    Ok(evaluator.list_from_vec(entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_line_returns_list() {
        let eval = Evaluator::new();
        let result = command_line(&eval, vec![]).unwrap();

        // Should be a list (could be empty in test context)
        assert!(matches!(result, Value::Pair(_) | Value::Null));
    }

    #[test]
    fn test_get_environment_variable_path() {
        let eval = Evaluator::new();
        // PATH should exist on most systems
        let name = Value::String(Rc::new(RefCell::new("PATH".to_string())));
        let result = get_environment_variable(&eval, vec![name]).unwrap();

        // Should return a string (PATH is usually set)
        assert!(matches!(result, Value::String(_)));
    }

    #[test]
    fn test_get_environment_variable_not_found() {
        let eval = Evaluator::new();
        let name = Value::String(Rc::new(RefCell::new(
            "PATINA_NONEXISTENT_VAR_12345".to_string(),
        )));
        let result = get_environment_variable(&eval, vec![name]).unwrap();

        // Should return #f for non-existent variable
        assert!(matches!(result, Value::Boolean(false)));
    }

    #[test]
    fn test_get_environment_variables_returns_alist() {
        let eval = Evaluator::new();
        let result = get_environment_variables(&eval, vec![]).unwrap();

        // Should be a list
        assert!(matches!(result, Value::Pair(_) | Value::Null));

        // If it's a pair, check that the car is also a pair (alist entry)
        if let Value::Pair(p) = result {
            let entry = p.borrow().0.clone();
            assert!(matches!(entry, Value::Pair(_)));
        }
    }
}
