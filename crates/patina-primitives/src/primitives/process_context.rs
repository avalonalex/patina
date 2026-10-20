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
use patina_core::{Cx, Value};
use patina_runtime::Arity;
use patina_runtime::EvalError;

/// Register all process-context primitives in the registry
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    registry.register(PrimitiveFn::new_cx(
        "scheme.process-context",
        "command-line",
        Arity::Exact(0),
        "Return the command line arguments as a list of strings",
        command_line,
    ));

    registry.register(PrimitiveFn::new_cx(
        "scheme.process-context",
        "exit",
        Arity::Range(0, 1),
        "Exit the program with optional status",
        exit_proc,
    ));

    registry.register(PrimitiveFn::new_cx(
        "scheme.process-context",
        "emergency-exit",
        Arity::Range(0, 1),
        "Exit immediately without running handlers",
        emergency_exit,
    ));

    registry.register(PrimitiveFn::new_cx(
        "scheme.process-context",
        "get-environment-variable",
        Arity::Exact(1),
        "Get the value of an environment variable",
        get_environment_variable,
    ));

    registry.register(PrimitiveFn::new_cx(
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
fn command_line<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "0".to_string(),
            actual: args.len(),
        });
    }

    // Copied out of the instance before allocating. Keep the configured
    // Rust strings unchanged even if Scheme mutates this result (#593).
    let command_line = cx.instance().command_line().to_vec();
    let arg_tvs: Vec<Value<'gc>> = command_line
        .into_iter()
        .map(|s| cx.alloc_string(s))
        .collect();
    Ok(cx.list_from_iter(arg_tvs))
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
fn exit_proc<'gc>(_cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    exit_now(args)
}

/// End the process without running any outstanding `dynamic-wind` after thunk
/// (R7RS 6.14).
///
/// The output a program left in open file ports is still written: what
/// `emergency-exit` skips is the program's cleanup, not its output. chibi
/// agrees; Gauche drops that output, as POSIX `_exit` would (#343).
fn emergency_exit<'gc>(_cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    exit_now(args)
}

/// End the process with the status `args` ask for, running no after thunk
/// first.
fn exit_now<'gc>(args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    let status = patina_runtime::exit_status::requested_status(Value::tagged_slice(args))?;
    // A success is withheld from a program that already reported an error (-k).
    patina_runtime::exit_status::end_process(status);
}

/// Get the value of an environment variable.
///
/// Returns #f if the variable is not set.
fn get_environment_variable<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let name: String = {
        match cx.get_string_contents(args[0]) {
            Some(s) => s,
            None => {
                return Err(EvalError::TypeError(
                    "get-environment-variable expects a string".to_string(),
                ));
            }
        }
    };

    match std::env::var(&name) {
        Ok(value) => Ok(cx.alloc_string(value)),
        Err(_) => Ok(Value::FALSE),
    }
}

/// Get all environment variables as an alist.
///
/// Each entry is (name . value) where both are strings.
fn get_environment_variables<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "0".to_string(),
            actual: args.len(),
        });
    }

    // Build alist entries as (name . value) pairs
    let entry_tvs: Vec<Value<'gc>> = std::env::vars()
        .map(|(k, v)| {
            let key = cx.alloc_string(k);
            let val = cx.alloc_string(v);
            cx.alloc_pair(key, val)
        })
        .collect();
    Ok(cx.list_from_iter(entry_tvs))
}
