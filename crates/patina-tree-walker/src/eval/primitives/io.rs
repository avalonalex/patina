//! I/O primitives - display, write, newline
//!
//! ## Known Issues
//!
//! Due to rustyline's stdout buffering behavior, `display` currently uses `println!`
//! which adds an unwanted newline after each call. This is not R7RS compliant but
//! ensures output is immediately visible in the REPL. Using `print!` + `flush()`
//! causes output to be completely swallowed by rustyline.
//!
//! Future fix: Consider switching to reedline or implementing proper ExternalPrinter API.

use super::super::error::EvalError;
use super::Evaluator;
use patina_runtime::value::Value;

/// (display obj) - Write obj in human-readable format
///
/// WARNING: Currently adds a newline after output due to rustyline buffering issues.
/// This is NOT R7RS compliant but makes the REPL usable.
pub(super) fn display(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "display expects 1 argument".to_string(),
            actual: args.len(),
        });
    }

    let val = &args[0];
    let output = match val {
        Value::String(s) => s.borrow().to_string(),
        _ => val.to_string(),
    };

    // Use println to force immediate output (adds unwanted newline but works)
    println!("{}", output);

    Ok(Value::Unspecified)
}

/// (write obj) - Write obj in machine-readable format
///
/// Strings are printed with quotes. Same limitation as display - adds unwanted newline.
pub(super) fn write(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "write expects 1 argument".to_string(),
            actual: args.len(),
        });
    }

    let output = args[0].to_string();
    println!("{}", output);

    Ok(Value::Unspecified)
}

/// (newline) - Write a newline character
pub(super) fn newline(_eval: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "newline expects 0 arguments".to_string(),
            actual: args.len(),
        });
    }

    println!();

    Ok(Value::Unspecified)
}

pub(super) fn register(registry: &mut super::PrimitiveRegistry) {
    use super::super::EvalResult;
    use super::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // display - Write in human-readable format
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "display",
        Arity::Exact(1),
        "Writes obj to the textual output port in a human-readable format.",
        |eval, args, _tail| display(eval, args).map(EvalResult::Value),
    ));

    // write - Write in machine-readable format
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "write",
        Arity::Exact(1),
        "Writes obj to the textual output port in a machine-readable format.",
        |eval, args, _tail| write(eval, args).map(EvalResult::Value),
    ));

    // newline - Write a newline
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "newline",
        Arity::Exact(0),
        "Writes a newline to the textual output port.",
        |eval, args, _tail| newline(eval, args).map(EvalResult::Value),
    ));
}
