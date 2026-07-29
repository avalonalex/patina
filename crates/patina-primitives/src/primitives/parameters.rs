//! Parameter primitives for R7RS dynamic parameters
//!
//! Parameters provide dynamic scoping in Scheme. They are created with
//! `make-parameter` and can be dynamically rebound with `parameterize`.
//!
//! # Examples
//!
//! ```scheme
//! (define radix (make-parameter 10))
//! (radix)  ; => 10
//! (radix 16)  ; Sets radix to 16
//! (radix)  ; => 16
//!
//! (parameterize ((radix 2))
//!   (radix))  ; => 2
//! (radix)  ; => 16 (restored after parameterize)
//! ```

use crate::apply_context::ApplyContext;
use patina_core::TaggedValue;
use patina_runtime::EvalError;
use std::cell::RefCell;
use std::rc::Rc;

/// make-parameter primitive
///
/// (make-parameter init [converter])
///
/// Creates a new parameter object with initial value `init`.
/// If `converter` is provided, it will be called to validate/convert
/// any value set on this parameter.
pub(super) fn make_parameter(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    // Check arity: 1 or 2 arguments
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "1 or 2".to_string(),
            actual: args.len(),
        });
    }

    let heap = ctx.heap();

    // Check if converter is provided and is a procedure
    let converter: Option<TaggedValue> = if args.len() == 2 {
        let conv_tagged = args[1];
        // Verify converter is a procedure using heap
        if heap.borrow().is_procedure(conv_tagged) || heap.borrow().is_parameter(conv_tagged) {
            Some(conv_tagged)
        } else {
            return Err(EvalError::TypeError(format!(
                "make-parameter: converter must be a procedure, got {}",
                heap.borrow().type_name(conv_tagged)
            )));
        }
    } else {
        None
    };

    // If converter is provided, apply it to initial value
    let init_value = if let Some(conv) = converter {
        ctx.apply_proc(conv, vec![args[0]])?
    } else {
        args[0]
    };

    // Create Parameter natively on the heap
    Ok(heap
        .borrow_mut()
        .alloc_parameter(Rc::new(RefCell::new(vec![init_value])), converter))
}

/// Register parameter primitives with the registry
pub(super) fn register(registry: &mut super::PrimitiveRegistry) {
    use crate::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // make-parameter - create a parameter object
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "make-parameter",
        Arity::Range(1, 2),
        "Returns a new parameter initialized to init. If a conversion procedure converter is specified, it is called with init and the result becomes the current value of the parameter.",
        make_parameter,
    ));
}
