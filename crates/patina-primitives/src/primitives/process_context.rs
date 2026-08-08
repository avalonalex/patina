//! Process context primitives
//!
//! R7RS (scheme process-context) library procedures:
//! - command-line: Get command line arguments
//! - exit: Exit program (with handlers)
//! - emergency-exit: Exit immediately
//! - get-environment-variable: Get single env var
//! - get-environment-variables: Get all env vars
use crate::registry::PrimitiveFn;
use crate::registry::PrimitiveRegistry;
use patina_core::TaggedValue;
use patina_runtime::Arity;
use patina_runtime::EvalError;
use patina_runtime::SharedHeap;

/// Register all process-context primitives in the registry
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    registry.register(PrimitiveFn::new_heap(
        "scheme.process-context",
        "command-line",
        Arity::Exact(0),
        "Return the command line arguments as a list of strings",
        command_line,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.process-context",
        "exit",
        Arity::Range(0, 1),
        "Exit the program with optional status",
        exit_proc,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.process-context",
        "emergency-exit",
        Arity::Range(0, 1),
        "Exit immediately without running handlers",
        emergency_exit,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.process-context",
        "get-environment-variable",
        Arity::Exact(1),
        "Get the value of an environment variable",
        get_environment_variable,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.process-context",
        "get-environment-variables",
        Arity::Exact(0),
        "Get all environment variables as an alist",
        get_environment_variables,
    ));
}

/// Return the command line arguments as a list of strings.
///
/// The first element is the program name (implementation-dependent).
fn command_line(heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "0".to_string(),
            actual: args.len(),
        });
    }

    let mut h = heap.borrow_mut();
    // Allocate each arg string, then build the list
    let arg_tvs: Vec<TaggedValue> = std::env::args().map(|s| h.alloc_string(s)).collect();
    Ok(h.list_from_iter(arg_tvs))
}

/// Exit the program with optional status.
///
/// R7RS says this should run dynamic-wind handlers first, but since we don't
/// have dynamic-wind fully implemented, we just exit.
///
/// - No argument or #t: exit with success (0)
/// - #f: exit with failure (1)
/// - Integer: exit with that code
fn exit_proc(_heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
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
fn emergency_exit(_heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
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
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

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
    heap: &SharedHeap,
    args: &[TaggedValue],
) -> Result<TaggedValue, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "0".to_string(),
            actual: args.len(),
        });
    }

    let mut h = heap.borrow_mut();
    // Build alist entries as (name . value) pairs
    let entry_tvs: Vec<TaggedValue> = std::env::vars()
        .map(|(k, v)| {
            let key = h.alloc_string(k);
            let val = h.alloc_string(v);
            h.alloc_pair(key, val)
        })
        .collect();
    Ok(h.list_from_iter(entry_tvs))
}
