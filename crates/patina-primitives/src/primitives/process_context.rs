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

/// Exit the program with optional status, without unwinding.
///
/// R7RS 6.14 has `exit` run the after thunk of every outstanding
/// `dynamic-wind` first. That takes the backend's own continuation machinery,
/// so both backends intercept `exit` before it reaches the registry — the VM as
/// a control primitive, the tree-walker when it applies a primitive (#336).
/// What is left here is `exit` for a caller with no dynamic-wind machinery,
/// which ends the process as `emergency-exit` does.
///
/// - No argument or #t: exit with success (0)
/// - #f: exit with failure (1)
/// - Integer: exit with that code
fn exit_proc(_heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    exit_now(args)
}

/// Exit immediately without running handlers.
///
/// This corresponds to _exit() in POSIX.
fn emergency_exit(_heap: &SharedHeap, args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    exit_now(args)
}

/// End the process with the status `args` ask for, running nothing first.
fn exit_now(args: &[TaggedValue]) -> Result<TaggedValue, EvalError> {
    let status = patina_runtime::exit_status::requested_status(args)?;
    // A success is withheld from a program that already reported an error (-k).
    std::process::exit(patina_runtime::exit_status::status_for_exit(status));
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
