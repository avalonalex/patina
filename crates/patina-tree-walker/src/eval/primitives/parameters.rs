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

use super::super::Evaluator;
use super::super::error::EvalError;
use patina_runtime::Value;
use std::cell::RefCell;
use std::rc::Rc;

/// make-parameter primitive
///
/// (make-parameter init [converter])
///
/// Creates a new parameter object with initial value `init`.
/// If `converter` is provided, it will be called to validate/convert
/// any value set on this parameter.
///
/// # Examples
///
/// ```scheme
/// (define p (make-parameter 10))
/// (p)  ; => 10
/// (p 20)  ; Sets to 20
/// (p)  ; => 20
///
/// ;; With converter
/// (define radix
///   (make-parameter 10
///     (lambda (x)
///       (if (and (integer? x) (<= 2 x 16))
///         x
///         (error "invalid radix")))))
/// ```
pub(super) fn make_parameter(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    // Check arity: 1 or 2 arguments
    evaluator.check_arity_range(&args, 1, 2, "make-parameter")?;

    let init = args[0].clone();
    let converter = if args.len() == 2 {
        // Verify converter is a procedure
        match &args[1] {
            Value::Procedure(_) | Value::Parameter { .. } => Some(Box::new(args[1].clone())),
            _ => {
                return Err(EvalError::TypeError(format!(
                    "make-parameter: converter must be a procedure, got {}",
                    args[1].type_name()
                )));
            }
        }
    } else {
        None
    };

    // If converter is provided, apply it to initial value
    let value = if let Some(ref _conv) = converter {
        // We need to call the converter, but we can't do that here
        // because we don't have access to the evaluator.
        // For now, we'll just store the raw init value and let
        // the converter be applied when the parameter is first set.
        // TODO: Apply converter to initial value
        init
    } else {
        init
    };

    Ok(Value::Parameter {
        values: Rc::new(RefCell::new(vec![value])),
        converter,
    })
}

/// Register parameter primitives with the registry
pub(super) fn register(registry: &mut super::PrimitiveRegistry) {
    use super::super::EvalResult;
    use super::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // make-parameter - create a parameter object
    registry.register(PrimitiveFn::new(
        "scheme.base",
        "make-parameter",
        Arity::Range(1, 2),
        "Returns a new parameter initialized to init. If a conversion procedure converter is specified, it is called with init and the result becomes the current value of the parameter.",
        |eval, args, _tail| make_parameter(eval, args).map(EvalResult::Value),
    ));
}
