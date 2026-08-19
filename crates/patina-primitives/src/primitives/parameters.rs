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
        if heap.borrow().is_procedure(conv_tagged) {
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

/// `(%parameter-convert param value)` — the value `param` would take on, with
/// its converter applied, without installing anything.
///
/// `parameterize` needs conversion and installation to be separable: R7RS
/// §4.2.6 converts *every* new value before entering the `dynamic-wind`, so a
/// converter that raises cannot leave earlier bindings installed with no
/// after-thunk to undo them — and so a converter with side effects runs once
/// per `parameterize`, not once per re-entry.
pub(super) fn parameter_convert(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    let [param, value] = args[..] else {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    };
    let converter = {
        let heap = ctx.heap();
        let heap = heap.borrow();
        heap.get_parameter(param).and_then(|(_, converter)| converter)
    };
    match converter {
        Some(conv) => ctx.apply_proc(conv, vec![value]),
        // No converter, or not a parameter object at all — the standard ports
        // are parameter-like procedures whose validation only happens on
        // assignment, and `%parameterize-swap!` is what makes that safe.
        None => Ok(value),
    }
}

/// `(%parameterize-swap! params vals)` — install `vals` into `params`,
/// returning the values they held.
///
/// Used for both halves of `parameterize`'s wind: the before-thunk keeps the
/// list it returns, the after-thunk hands that list straight back.
///
/// Two things it does that a `for-each` of the ordinary setter did not.
///
/// It installs *raw*, so restoring does not run the converter a second time on
/// a value that converter already produced — which doubled a `(lambda (x) (* x
/// 2))` on the way out and made a `number->string` raise.
///
/// And it is transactional: if an install fails, the ones already done are
/// undone before the error escapes. That matters because not every
/// parameter-like thing is a parameter *object* — the standard ports are
/// procedures over a thread-local (see `io/ports.rs`), so they validate their
/// argument on assignment and have no converter to run first. Installing them
/// one after another inside a `dynamic-wind`'s before-thunk meant a failure
/// partway left the earlier ones installed with no after-thunk to undo them:
/// `(parameterize ((current-output-port sink) (current-input-port 5)) …)` sent
/// every later write to `sink` forever.
pub(super) fn parameterize_swap(
    ctx: &dyn ApplyContext,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    let [params_tv, vals_tv] = args[..] else {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    };
    let (params, vals) = {
        let heap = ctx.heap();
        let heap = heap.borrow();
        let lists = heap.list_to_vec(params_tv).zip(heap.list_to_vec(vals_tv));
        lists.ok_or_else(|| {
            EvalError::TypeError("%parameterize-swap!: expected two lists".to_string())
        })?
    };
    if params.len() != vals.len() {
        return Err(EvalError::TypeError(
            "%parameterize-swap!: parameter and value lists differ in length".to_string(),
        ));
    }

    let mut olds: Vec<TaggedValue> = Vec::with_capacity(params.len());
    for (i, (&param, &val)) in params.iter().zip(vals.iter()).enumerate() {
        let old = read_parameter(ctx, param)?;
        match install_parameter(ctx, param, val) {
            Ok(()) => olds.push(old),
            Err(e) => {
                // Innermost first, so the undo mirrors the install. A value
                // that was current a moment ago cannot fail its own
                // validation; if putting one back escapes anyway, that escape
                // outranks the error being unwound.
                for (j, &done) in params[..i].iter().enumerate().rev() {
                    install_parameter(ctx, done, olds[j])?;
                }
                return Err(e);
            }
        }
    }
    Ok(ctx.heap().borrow_mut().list_from_iter(olds))
}

/// A parameter's current value — read from the object, or by calling it, for
/// the parameter-like procedures that are not objects.
fn read_parameter(ctx: &dyn ApplyContext, param: TaggedValue) -> Result<TaggedValue, EvalError> {
    let stored = {
        let heap = ctx.heap();
        let heap = heap.borrow();
        heap.get_parameter(param)
            .map(|(values, _)| values.borrow().last().copied())
    };
    match stored {
        Some(value) => Ok(value.unwrap_or(TaggedValue::UNSPECIFIED)),
        None => ctx.apply_proc(param, vec![]),
    }
}

/// Install `value`, bypassing the converter for a real parameter object.
fn install_parameter(
    ctx: &dyn ApplyContext,
    param: TaggedValue,
    value: TaggedValue,
) -> Result<(), EvalError> {
    let values = {
        let heap = ctx.heap();
        let heap = heap.borrow();
        heap.get_parameter(param).map(|(values, _)| values)
    };
    match values {
        Some(values) => {
            if let Some(top) = values.borrow_mut().last_mut() {
                *top = value;
            }
            Ok(())
        }
        // Not a parameter object: the assignment *is* the validation.
        None => ctx.apply_proc(param, vec![value]).map(|_| ()),
    }
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

    // The two halves `parameterize` needs kept apart — see their doc comments.
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "%parameter-convert",
        Arity::Exact(2),
        "Returns the value the parameter would take, with its converter applied, without installing it.",
        parameter_convert,
    ));
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "%parameterize-swap!",
        Arity::Exact(2),
        "Installs a list of values into a list of parameters, returning the values they held.",
        parameterize_swap,
    ));
}
