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
use crate::registry::{Step, done_with_result};
use patina_core::{ConvertedParamValue, Cx, ParamValue, Value};
use patina_runtime::EvalError;
use smallvec::smallvec;

/// make-parameter primitive
///
/// (make-parameter init [converter])
///
/// Creates a new parameter object with initial value `init`. If `converter` is
/// provided, it will be called to validate/convert any value set on this
/// parameter — `init` included, which is the call this primitive asks the
/// machine to make rather than making it from Rust ([`Step`], #478).
pub(super) fn make_parameter<'gc>(
    cx: &mut Cx<'gc>,
    _ctx: &dyn ApplyContext,
    args: &[Value<'gc>],
) -> Result<Step<'gc>, EvalError> {
    // Check arity: 1 or 2 arguments
    if args.is_empty() || args.len() > 2 {
        return Err(EvalError::WrongArity {
            expected: "1 or 2".to_string(),
            actual: args.len(),
        });
    }
    let Some(&converter) = args.get(1) else {
        return Ok(Step::Done(cx.alloc_parameter(args[0], None)));
    };
    if !cx.is_procedure(converter) {
        return Err(EvalError::TypeError(format!(
            "make-parameter: converter must be a procedure, got {}",
            cx.type_name(converter)
        )));
    }
    Ok(Step::Call {
        callee: converter,
        args: smallvec![args[0]],
        state: converter,
    })
}

/// `make-parameter` once its converter has returned the initial value.
pub(super) fn make_parameter_converted<'gc>(
    cx: &mut Cx<'gc>,
    _ctx: &dyn ApplyContext,
    converter: Value<'gc>,
    init: Value<'gc>,
) -> Result<Step<'gc>, EvalError> {
    Ok(Step::Done(cx.alloc_parameter(init, Some(converter))))
}

/// The converter of `param`, if it is a parameter object that has one.
fn converter_of<'gc>(cx: &Cx<'gc>, param: Value<'gc>) -> Option<Value<'gc>> {
    cx.parameter_converter(param).flatten()
}

/// `(%parameter-convert param value)` — the value `param` would take on, with
/// its converter applied, without installing anything.
///
/// `parameterize` needs conversion and installation to be separable: R7RS
/// §4.2.6 converts *every* new value before entering the `dynamic-wind`, so a
/// converter that raises cannot leave earlier bindings installed with no
/// after-thunk to undo them — and so a converter with side effects runs once
/// per `parameterize`, not once per re-entry.
///
/// The converter is called by the machine ([`Step`]), so a continuation
/// captured inside it and re-entered after `parameterize` used the value
/// runs the rest of the `parameterize` again with the new one (#478).
pub(super) fn parameter_convert<'gc>(
    cx: &mut Cx<'gc>,
    _ctx: &dyn ApplyContext,
    args: &[Value<'gc>],
) -> Result<Step<'gc>, EvalError> {
    let [param, value] = args[..] else {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    };
    Ok(match converter_of(cx, param) {
        Some(conv) => Step::Call {
            callee: conv,
            args: smallvec![value],
            state: Value::UNSPECIFIED,
        },
        // No converter, or not a parameter object at all — the standard ports
        // are parameter-like procedures whose validation only happens on
        // assignment, and `%parameterize-swap!` is what makes that safe.
        None => Step::Done(value),
    })
}

/// `(%parameter-set! param value)` — what calling a parameter object with one
/// argument does: its converter, if it has one, applied to `value`, and the
/// result made the current value. The backends dispatch `(p v)` here, so the
/// converter is a call the machine makes ([`Step`], #478). The value is
/// stored through the funnel ([`ParamValue`], #706), which the GC census
/// counts at `parameter_install`, or after a converter at
/// `parameter_install_converted`.
pub(super) fn parameter_set<'gc>(
    cx: &mut Cx<'gc>,
    _ctx: &dyn ApplyContext,
    args: &[Value<'gc>],
) -> Result<Step<'gc>, EvalError> {
    let [param, value] = args[..] else {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    };
    match parameter_converter(cx, param)? {
        Some(conv) => Ok(Step::Call {
            callee: conv,
            args: smallvec![value],
            state: param,
        }),
        None => {
            cx.store(ParamValue(param), value);
            Ok(Step::Done(Value::UNSPECIFIED))
        }
    }
}

/// `%parameter-set!` once the converter has returned the value to install.
pub(super) fn parameter_set_converted<'gc>(
    cx: &mut Cx<'gc>,
    _ctx: &dyn ApplyContext,
    param: Value<'gc>,
    value: Value<'gc>,
) -> Result<Step<'gc>, EvalError> {
    parameter_converter(cx, param)?;
    cx.store(ConvertedParamValue(param), value);
    Ok(Step::Done(Value::UNSPECIFIED))
}

/// The converter of the parameter object `param`, if it has one, or the
/// error `%parameter-set!` raises for anything that is not one.
fn parameter_converter<'gc>(
    cx: &Cx<'gc>,
    param: Value<'gc>,
) -> Result<Option<Value<'gc>>, EvalError> {
    cx.parameter_converter(param)
        .ok_or_else(|| EvalError::TypeError("%parameter-set!: not a parameter object".to_string()))
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
/// procedures over the interpreter's current-port slots (see `io/ports.rs`),
/// so they validate their argument on assignment and have no converter to run
/// first. Installing them
/// one after another inside a `dynamic-wind`'s before-thunk meant a failure
/// partway left the earlier ones installed with no after-thunk to undo them:
/// `(parameterize ((current-output-port sink) (current-input-port 5)) …)` sent
/// every later write to `sink` forever.
pub(super) fn parameterize_swap<'gc>(
    cx: &mut Cx<'gc>,
    ctx: &dyn ApplyContext,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    let [params_list, vals_list] = args[..] else {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    };
    let (params, vals) = cx
        .list_to_vec(params_list)
        .zip(cx.list_to_vec(vals_list))
        .ok_or_else(|| {
            EvalError::TypeError("%parameterize-swap!: expected two lists".to_string())
        })?;
    if params.len() != vals.len() {
        return Err(EvalError::TypeError(
            "%parameterize-swap!: parameter and value lists differ in length".to_string(),
        ));
    }

    let mut olds: Vec<Value<'gc>> = Vec::with_capacity(params.len());
    for (i, (&param, &val)) in params.iter().zip(vals.iter()).enumerate() {
        let old = read_parameter(cx, ctx, param)?;
        match install_parameter(cx, ctx, param, val) {
            Ok(()) => olds.push(old),
            Err(e) => {
                // Innermost first, so the undo mirrors the install. A value
                // that was current a moment ago cannot fail its own
                // validation; if putting one back escapes anyway, that escape
                // outranks the error being unwound.
                for (j, &done) in params[..i].iter().enumerate().rev() {
                    install_parameter(cx, ctx, done, olds[j])?;
                }
                return Err(e);
            }
        }
    }
    Ok(cx.list_from_iter(olds))
}

/// A parameter's current value — read from the object, or by calling it, for
/// the parameter-like procedures that are not objects.
fn read_parameter<'gc>(
    cx: &mut Cx<'gc>,
    ctx: &dyn ApplyContext,
    param: Value<'gc>,
) -> Result<Value<'gc>, EvalError> {
    match cx.parameter_value(param) {
        Some(value) => Ok(value),
        // A parameter-like procedure that is not a parameter object, read by
        // calling it. Stage 4a ends this call (PRD/GC_PRD.md §11.3).
        None => ctx.apply_in(cx, param, &[]),
    }
}

/// Install `value`, bypassing the converter for a real parameter object:
/// through the funnel ([`ParamValue`], #706), which the GC census counts at
/// `parameter_install`.
fn install_parameter<'gc>(
    cx: &mut Cx<'gc>,
    ctx: &dyn ApplyContext,
    param: Value<'gc>,
    value: Value<'gc>,
) -> Result<(), EvalError> {
    if cx.is_parameter(param) {
        cx.store(ParamValue(param), value);
        return Ok(());
    }
    // Not a parameter object: the assignment *is* the validation. Stage 4a
    // ends this call (PRD/GC_PRD.md §11.3).
    ctx.apply_in(cx, param, &[value]).map(|_| ())
}

/// Register parameter primitives with the registry
pub(super) fn register(registry: &mut super::PrimitiveRegistry) {
    use crate::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // make-parameter - create a parameter object
    registry.register(PrimitiveFn::new_resumable(
        "scheme.base",
        "make-parameter",
        Arity::Range(1, 2),
        "Returns a new parameter initialized to init. If a conversion procedure converter is specified, it is called with init and the result becomes the current value of the parameter.",
        make_parameter,
        make_parameter_converted,
    ));

    // The two halves `parameterize` needs kept apart — see their doc comments.
    registry.register(PrimitiveFn::new_resumable(
        "scheme.base",
        "%parameter-convert",
        Arity::Exact(2),
        "Returns the value the parameter would take, with its converter applied, without installing it.",
        parameter_convert,
        done_with_result,
    ));
    registry.register(PrimitiveFn::new_resumable(
        "scheme.base",
        "%parameter-set!",
        Arity::Exact(2),
        "Sets a parameter object's current value, through its converter.",
        parameter_set,
        parameter_set_converted,
    ));
    registry.register(PrimitiveFn::new_higher_order(
        "scheme.base",
        "%parameterize-swap!",
        Arity::Exact(2),
        "Installs a list of values into a list of parameters, returning the values they held.",
        parameterize_swap,
    ));
}
