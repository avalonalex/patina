//! Symbol primitives (R7RS Section 6.5)
//!
//! Implements symbol operations:
//! - `symbol=?` - Compare symbols by name
//! - `symbol->string` - Get symbol's name as string
//! - `string->symbol` - Create symbol from string

use crate::registry::{PrimitiveFn, PrimitiveRegistry};
use patina_core::TaggedValue;
use patina_runtime::Arity;
use patina_runtime::EvalError;
use patina_runtime::SharedHeap;

/// Register symbol primitives in the registry
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "symbol=?",
        Arity::Min(2),
        "(symbol=? sym1 sym2 ...) - Returns #t if all symbols have the same name",
        symbol_equal,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "symbol->string",
        Arity::Exact(1),
        "(symbol->string sym) - Returns the name of the symbol as a string",
        symbol_to_string,
    ));

    registry.register(PrimitiveFn::new_heap(
        "scheme.base",
        "string->symbol",
        Arity::Exact(1),
        "(string->symbol str) - Returns a symbol with the given name",
        string_to_symbol,
    ));
}

/// (symbol=? sym1 sym2 ...)
///
/// Returns #t if all arguments are symbols and all have the same names
/// in the sense of string=?.
fn symbol_equal(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() < 2 {
        return Err(EvalError::WrongArity {
            expected: "at least 2".to_string(),
            actual: args.len(),
        });
    }

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
                    return Ok(TaggedValue::FALSE);
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

    Ok(TaggedValue::TRUE)
}

/// (symbol->string sym)
///
/// Returns the name of the symbol as a string, but without adding escapes.
/// Note: R7RS says it's an error to mutate the returned string, but we
/// return a regular mutable string for simplicity (as most Schemes do).
fn symbol_to_string(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let heap_ref = heap.borrow();

    match heap_ref.get_symbol_name(args[0]) {
        Some(name) => {
            // Allocate string on heap and return TaggedValue
            let name_owned = name.to_string();
            drop(heap_ref); // Release immutable borrow before mutable borrow
            let tagged = heap.borrow_mut().alloc_string(name_owned);
            Ok(tagged)
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
fn string_to_symbol(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    // Get string contents from heap
    let name = heap.borrow().get_string_contents(args[0]).ok_or_else(|| {
        EvalError::TypeError(format!(
            "string->symbol expects a string, got {}",
            heap.borrow().type_name(args[0])
        ))
    })?;

    // Intern the symbol and return TaggedValue
    let tagged = heap.borrow_mut().intern_symbol(&name);
    Ok(tagged)
}
