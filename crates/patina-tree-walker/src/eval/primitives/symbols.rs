//! Symbol primitives (R7RS Section 6.5)
//!
//! Implements symbol operations:
//! - `symbol=?` - Compare symbols by name
//! - `symbol->string` - Get symbol's name as string
//! - `string->symbol` - Create symbol from string

use crate::eval::primitives::registry::{PrimitiveFn, PrimitiveRegistry};
use crate::eval::{EvalError, EvalResult, Evaluator};
use patina_runtime::{Arity, Value};
use std::cell::RefCell;
use std::rc::Rc;

/// Register symbol primitives in the registry
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "symbol=?",
        Arity::Min(2),
        "(symbol=? sym1 sym2 ...) - Returns #t if all symbols have the same name",
        |eval, args, _tail| symbol_equal(eval, args),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "symbol->string",
        Arity::Exact(1),
        "(symbol->string sym) - Returns the name of the symbol as a string",
        |_eval, args, _tail| symbol_to_string(args),
    ));

    registry.register(PrimitiveFn::new(
        "scheme.base",
        "string->symbol",
        Arity::Exact(1),
        "(string->symbol str) - Returns a symbol with the given name",
        |_eval, args, _tail| string_to_symbol(args),
    ));
}

/// (symbol=? sym1 sym2 ...)
///
/// Returns #t if all arguments are symbols and all have the same names
/// in the sense of string=?.
fn symbol_equal(eval: &Evaluator, args: Vec<Value>) -> Result<EvalResult, EvalError> {
    eval.check_arity_min(&args, 2, "symbol=?")?;

    // First, check that all arguments are symbols
    for arg in &args {
        if !matches!(arg, Value::Symbol(_)) {
            return Err(EvalError::TypeError(format!(
                "symbol=? expects symbols, got {}",
                arg
            )));
        }
    }

    // Extract the first symbol's name
    let first_name = match &args[0] {
        Value::Symbol(s) => s.as_ref(),
        _ => unreachable!(), // Already checked above
    };

    // Check that all remaining symbols have the same name
    for arg in &args[1..] {
        match arg {
            Value::Symbol(s) => {
                if s.as_ref() != first_name {
                    return Ok(EvalResult::Value(Value::Boolean(false)));
                }
            }
            _ => unreachable!(), // Already checked above
        }
    }

    Ok(EvalResult::Value(Value::Boolean(true)))
}

/// (symbol->string sym)
///
/// Returns the name of the symbol as a string, but without adding escapes.
/// Note: R7RS says it's an error to mutate the returned string, but we
/// return a regular mutable string for simplicity (as most Schemes do).
fn symbol_to_string(args: Vec<Value>) -> Result<EvalResult, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    match &args[0] {
        Value::Symbol(s) => Ok(EvalResult::Value(Value::String(Rc::new(RefCell::new(
            s.to_string(),
        ))))),
        other => Err(EvalError::TypeError(format!(
            "symbol->string expects a symbol, got {}",
            other
        ))),
    }
}

/// (string->symbol str)
///
/// Returns the symbol whose name is the given string. This procedure can
/// create symbols with names containing special characters that would
/// require escaping when written, but does not interpret escapes in its input.
fn string_to_symbol(args: Vec<Value>) -> Result<EvalResult, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    match &args[0] {
        Value::String(s) => {
            let name = s.borrow().clone();
            Ok(EvalResult::Value(Value::Symbol(Rc::from(name))))
        }
        other => Err(EvalError::TypeError(format!(
            "string->symbol expects a string, got {}",
            other
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_symbol_equal_same() {
        let eval = Evaluator::new();
        let result = symbol_equal(
            &eval,
            vec![
                Value::Symbol(Rc::from("foo")),
                Value::Symbol(Rc::from("foo")),
            ],
        )
        .unwrap();
        assert!(matches!(result, EvalResult::Value(Value::Boolean(true))));
    }

    #[test]
    fn test_symbol_equal_different() {
        let eval = Evaluator::new();
        let result = symbol_equal(
            &eval,
            vec![
                Value::Symbol(Rc::from("foo")),
                Value::Symbol(Rc::from("bar")),
            ],
        )
        .unwrap();
        assert!(matches!(result, EvalResult::Value(Value::Boolean(false))));
    }

    #[test]
    fn test_symbol_equal_case_sensitive() {
        let eval = Evaluator::new();
        let result = symbol_equal(
            &eval,
            vec![
                Value::Symbol(Rc::from("foo")),
                Value::Symbol(Rc::from("FOO")),
            ],
        )
        .unwrap();
        assert!(matches!(result, EvalResult::Value(Value::Boolean(false))));
    }

    #[test]
    fn test_symbol_equal_multiple() {
        let eval = Evaluator::new();
        let result = symbol_equal(
            &eval,
            vec![
                Value::Symbol(Rc::from("x")),
                Value::Symbol(Rc::from("x")),
                Value::Symbol(Rc::from("x")),
            ],
        )
        .unwrap();
        assert!(matches!(result, EvalResult::Value(Value::Boolean(true))));
    }

    #[test]
    fn test_symbol_to_string() {
        let result = symbol_to_string(vec![Value::Symbol(Rc::from("flying-fish"))]).unwrap();
        match result {
            EvalResult::Value(Value::String(s)) => {
                assert_eq!(&*s.borrow(), "flying-fish");
            }
            _ => panic!("Expected string"),
        }
    }

    #[test]
    fn test_string_to_symbol() {
        let result = string_to_symbol(vec![Value::String(Rc::new(RefCell::new(
            "foo".to_string(),
        )))])
        .unwrap();
        match result {
            EvalResult::Value(Value::Symbol(s)) => {
                assert_eq!(s.as_ref(), "foo");
            }
            _ => panic!("Expected symbol"),
        }
    }

    #[test]
    fn test_roundtrip() {
        // (symbol->string (string->symbol "test")) should equal "test"
        let sym_result = string_to_symbol(vec![Value::String(Rc::new(RefCell::new(
            "test".to_string(),
        )))])
        .unwrap();
        let sym = match sym_result {
            EvalResult::Value(v) => v,
            _ => panic!("Expected value"),
        };

        let str_result = symbol_to_string(vec![sym]).unwrap();
        match str_result {
            EvalResult::Value(Value::String(s)) => {
                assert_eq!(&*s.borrow(), "test");
            }
            _ => panic!("Expected string"),
        }
    }
}
