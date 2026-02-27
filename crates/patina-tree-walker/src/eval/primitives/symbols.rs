//! Symbol primitives (R7RS Section 6.5)
//!
//! Implements symbol operations:
//! - `symbol=?` - Compare symbols by name
//! - `symbol->string` - Get symbol's name as string
//! - `string->symbol` - Create symbol from string

use crate::eval::primitives::registry::{PrimitiveFn, PrimitiveRegistry};
use crate::eval::{EvalError, EvalResult, Evaluator};
use patina_core::TaggedValue;
use patina_runtime::Arity;

/// Register symbol primitives in the registry
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "symbol=?",
        Arity::Min(2),
        "(symbol=? sym1 sym2 ...) - Returns #t if all symbols have the same name",
        |eval, args, _tail| symbol_equal(eval, args),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "symbol->string",
        Arity::Exact(1),
        "(symbol->string sym) - Returns the name of the symbol as a string",
        |eval, args, _tail| symbol_to_string(eval, args),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "scheme.base",
        "string->symbol",
        Arity::Exact(1),
        "(string->symbol str) - Returns a symbol with the given name",
        |eval, args, _tail| string_to_symbol(eval, args),
    ));
}

/// (symbol=? sym1 sym2 ...)
///
/// Returns #t if all arguments are symbols and all have the same names
/// in the sense of string=?.
fn symbol_equal(evaluator: &Evaluator, args: Vec<TaggedValue>) -> Result<EvalResult, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();

    // Check all args are symbols and get first name
    let first_name = match heap_ref.get_symbol_name(args[0]) {
        Some(name) => name,
        None => {
            return Err(EvalError::TypeError(format!(
                "symbol=? expects symbols, got {}",
                heap_ref.type_name(args[0])
            )));
        }
    };

    // Check remaining symbols have the same name
    for &arg in &args[1..] {
        match heap_ref.get_symbol_name(arg) {
            Some(name) => {
                if name != first_name {
                    return Ok(EvalResult::Tagged(TaggedValue::FALSE));
                }
            }
            None => {
                return Err(EvalError::TypeError(format!(
                    "symbol=? expects symbols, got {}",
                    heap_ref.type_name(arg)
                )));
            }
        }
    }

    Ok(EvalResult::Tagged(TaggedValue::TRUE))
}

/// (symbol->string sym)
///
/// Returns the name of the symbol as a string, but without adding escapes.
/// Note: R7RS says it's an error to mutate the returned string, but we
/// return a regular mutable string for simplicity (as most Schemes do).
fn symbol_to_string(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<EvalResult, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let heap_ref = heap.borrow();

    match heap_ref.get_symbol_name(args[0]) {
        Some(name) => {
            // Allocate string on heap and return TaggedValue
            let name_owned = name.to_string();
            drop(heap_ref); // Release immutable borrow before mutable borrow
            let tagged = heap.borrow_mut().alloc_string(name_owned);
            Ok(EvalResult::Tagged(tagged))
        }
        None => Err(EvalError::TypeError(format!(
            "symbol->string expects a symbol, got {}",
            heap_ref.type_name(args[0])
        ))),
    }
}

/// (string->symbol str)
///
/// Returns the symbol whose name is the given string. This procedure can
/// create symbols with names containing special characters that would
/// require escaping when written, but does not interpret escapes in its input.
fn string_to_symbol(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<EvalResult, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();

    // Get string contents from heap
    let name = heap.borrow().get_string_contents(args[0]).ok_or_else(|| {
        EvalError::TypeError(format!(
            "string->symbol expects a string, got {}",
            heap.borrow().type_name(args[0])
        ))
    })?;

    // Intern the symbol and return TaggedValue
    let tagged = heap.borrow_mut().intern_symbol(&name);
    Ok(EvalResult::Tagged(tagged))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym(name: &str, eval: &Evaluator) -> TaggedValue {
        eval.global_env.heap().borrow_mut().intern_symbol(name)
    }

    fn string(s: &str, eval: &Evaluator) -> TaggedValue {
        eval.global_env
            .heap()
            .borrow_mut()
            .alloc_string(s.to_string())
    }

    fn get(result: EvalResult) -> TaggedValue {
        match result {
            EvalResult::Tagged(tv) => tv,
            EvalResult::TailCallPrimitive { .. } => panic!("Expected EvalResult::Tagged"),
        }
    }

    #[test]
    fn test_symbol_equal_same() {
        let eval = Evaluator::new();
        let result = symbol_equal(&eval, vec![sym("foo", &eval), sym("foo", &eval)]).unwrap();
        assert_eq!(get(result), TaggedValue::TRUE);
    }

    #[test]
    fn test_symbol_equal_different() {
        let eval = Evaluator::new();
        let result = symbol_equal(&eval, vec![sym("foo", &eval), sym("bar", &eval)]).unwrap();
        assert_eq!(get(result), TaggedValue::FALSE);
    }

    #[test]
    fn test_symbol_equal_case_sensitive() {
        let eval = Evaluator::new();
        let result = symbol_equal(&eval, vec![sym("foo", &eval), sym("FOO", &eval)]).unwrap();
        assert_eq!(get(result), TaggedValue::FALSE);
    }

    #[test]
    fn test_symbol_equal_multiple() {
        let eval = Evaluator::new();
        let result = symbol_equal(
            &eval,
            vec![sym("x", &eval), sym("x", &eval), sym("x", &eval)],
        )
        .unwrap();
        assert_eq!(get(result), TaggedValue::TRUE);
    }

    #[test]
    fn test_symbol_to_string() {
        let eval = Evaluator::new();
        let result = symbol_to_string(&eval, vec![sym("flying-fish", &eval)]).unwrap();
        let tagged = get(result);
        let heap = eval.global_env.heap();
        let str_contents = heap.borrow().get_string_contents(tagged).unwrap();
        assert_eq!(str_contents, "flying-fish");
    }

    #[test]
    fn test_string_to_symbol() {
        let eval = Evaluator::new();
        let result = string_to_symbol(&eval, vec![string("foo", &eval)]).unwrap();
        let tagged = get(result);
        let heap = eval.global_env.heap();
        let heap_ref = heap.borrow();
        let sym_name = heap_ref.get_symbol_name(tagged).unwrap();
        assert_eq!(sym_name, "foo");
    }

    #[test]
    fn test_roundtrip() {
        let eval = Evaluator::new();
        // (symbol->string (string->symbol "test")) should equal "test"
        let sym_result = string_to_symbol(&eval, vec![string("test", &eval)]).unwrap();
        let sym_tagged = get(sym_result);

        let str_result = symbol_to_string(&eval, vec![sym_tagged]).unwrap();
        let str_tagged = get(str_result);
        let heap = eval.global_env.heap();
        let str_contents = heap.borrow().get_string_contents(str_tagged).unwrap();
        assert_eq!(str_contents, "test");
    }
}
