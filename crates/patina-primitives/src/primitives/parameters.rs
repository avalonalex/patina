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
        // No converter, or not a parameter object at all: a parameter-like
        // procedure a program wrote validates on assignment, and
        // `%parameterize-each`'s undo is what makes that safe (#784). The
        // standard ports have converters since #608, so they convert here.
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
/// answering the values they held. If one of `params` is not a parameter
/// object, it installs nothing itself, and answers what `parameterize`'s
/// loop answers for `(params vals '())`, a call the machine makes
/// ([`Step`]).
///
/// Used for both halves of `parameterize`'s wind: the before thunk keeps the
/// list it returns, the after thunk hands that list straight back. It installs
/// *raw*, so restoring does not run the converter a second time on a value
/// that converter already produced, which doubled a `(lambda (x) (* x 2))` on
/// the way out and made a `number->string` raise.
///
/// It calls nothing from Rust (#784). Everything is checked before anything
/// is installed: each binding must be a parameter object, and a standard
/// port parameter's value a port facing its way (#608), since a program can
/// call this primitive itself; so an install cannot fail partway, and there
/// is nothing to undo. A procedure a program wrote to act as a parameter is
/// not one: for a list with one, the loop is `(scheme base)`'s
/// `%parameterize-each` (`lib/scheme/base/parameters.scm`), which calls it as
/// the machine calls any procedure, and undoes in Scheme. The heap keeps the
/// loop, which `(scheme base)` hands over as it loads
/// ([`parameterize_set_each`]): a template that tested the swap's answer
/// itself made each expansion of `parameterize` about half as dear again,
/// and one that passed the loop to the swap about 5% (#817).
pub(super) fn parameterize_swap<'gc>(
    cx: &mut Cx<'gc>,
    _ctx: &dyn ApplyContext,
    args: &[Value<'gc>],
) -> Result<Step<'gc>, EvalError> {
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
    for (&param, &value) in params.iter().zip(vals.iter()) {
        if !check_raw_install(cx, param, value)? {
            let each = cx.parameterize_each().ok_or_else(|| {
                EvalError::TypeError(
                    "%parameterize-swap!: not a parameter object, and (scheme base) not loaded"
                        .to_string(),
                )
            })?;
            return Ok(Step::Call {
                callee: each,
                args: smallvec![params_list, vals_list, Value::NULL],
                state: Value::UNSPECIFIED,
            });
        }
    }
    let mut olds: Vec<Value<'gc>> = Vec::with_capacity(params.len());
    for (&param, &value) in params.iter().zip(vals.iter()) {
        olds.push(cx.parameter_value(param).unwrap_or(Value::UNSPECIFIED));
        cx.store(ParamValue(param), value);
    }
    Ok(Step::Done(cx.list_from_iter(olds)))
}

/// `(%parameter-install! p v)` — store `v` into the parameter object `p`
/// raw, past its converter, and answer `#t`; for anything else, install
/// nothing and answer `#f`. `%parameterize-each`'s install of a parameter
/// object in a list with procedures a program wrote, so that no converter
/// runs a second time (#784). Checked as the swap checks: a
/// standard port parameter takes only a port facing its way (#608).
pub(super) fn parameter_install<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    let [param, value] = args[..] else {
        return Err(EvalError::WrongArity {
            expected: "2".to_string(),
            actual: args.len(),
        });
    };
    if !check_raw_install(cx, param, value)? {
        return Ok(Value::FALSE);
    }
    cx.store(ParamValue(param), value);
    Ok(Value::TRUE)
}

/// `(%parameterize-set-each! loop)` — make `loop` what `%parameterize-swap!`
/// has the machine call for bindings that are not all parameter objects:
/// `(scheme base)`'s `%parameterize-each`, which its body hands over as it
/// loads (#817). The heap roots it for its life; a later `(scheme base)`
/// replaces it.
pub(super) fn parameterize_set_each<'gc>(
    cx: &mut Cx<'gc>,
    args: &[Value<'gc>],
) -> Result<Value<'gc>, EvalError> {
    let [procedure] = args[..] else {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    };
    if !cx.is_procedure(procedure) {
        return Err(EvalError::TypeError(format!(
            "%parameterize-set-each!: expected a procedure, got {}",
            cx.type_name(procedure)
        )));
    }
    cx.set_parameterize_each(procedure);
    Ok(Value::UNSPECIFIED)
}

/// Whether `value` may be installed raw into `param`: `false` if `param` is
/// no parameter object, the port check's error if it is a standard port
/// parameter and `value` no port facing its way (#608), `true` otherwise.
/// The standard ports' cached current port relies on the check (the store
/// funnel's `refresh_current_port`).
fn check_raw_install<'gc>(
    cx: &Cx<'gc>,
    param: Value<'gc>,
    value: Value<'gc>,
) -> Result<bool, EvalError> {
    if !cx.is_parameter(param) {
        return Ok(false);
    }
    if let Some(which) = cx.standard_port_parameter(param) {
        super::io::check_current_port(cx, which, value)?;
    }
    Ok(true)
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
    registry.register(PrimitiveFn::new_resumable(
        "scheme.base",
        "%parameterize-swap!",
        Arity::Exact(2),
        "Installs a list of values into a list of parameter objects, returning the values they held; if one is not a parameter object, returns what parameterize's loop returns for the two lists.",
        parameterize_swap,
        done_with_result,
    ));
}

/// Register `%parameter-install!` and `%parameterize-set-each!`, after every
/// other primitive (`register_all`), so that no other primitive's index
/// moves: an index shift of one cost `string-ports` about 1% in cycles at the
/// same instructions (#608).
pub(super) fn register_last(registry: &mut super::PrimitiveRegistry) {
    use crate::registry::PrimitiveFn;
    use patina_runtime::Arity;

    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "%parameter-install!",
        Arity::Exact(2),
        "Stores a value into a parameter object raw, past its converter, answering #t, or #f for anything else.",
        parameter_install,
    ));
    registry.register(PrimitiveFn::new_cx(
        "scheme.base",
        "%parameterize-set-each!",
        Arity::Exact(1),
        "Makes a procedure the loop parameterize runs for bindings that are not all parameter objects.",
        parameterize_set_each,
    ));
}
