use super::super::Evaluator;
use super::super::debug::DebugStage;
use super::super::error::EvalError;
use super::registry::PrimitiveRegistry;
use patina_core::TaggedValue;

/// Register all debug primitives in the registry
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    use super::super::EvalResult;
    use super::registry::PrimitiveFn;
    use patina_runtime::Arity;

    // Evaluation debugging primitives
    registry.register(PrimitiveFn::new_tagged(
        "patina.debug",
        "debug-enable",
        Arity::Exact(1),
        "Enable debugging for a specific stage (lex, parse, eval, apply, env, expand)",
        |eval, args, _| debug_enable(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "patina.debug",
        "debug-disable",
        Arity::Exact(1),
        "Disable debugging for a specific stage",
        |eval, args, _| debug_disable(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "patina.debug",
        "debug-clear",
        Arity::Exact(0),
        "Disable all debugging stages",
        |eval, args, _| debug_clear(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "patina.debug",
        "debug-status",
        Arity::Exact(0),
        "Return a list of currently enabled debug stages",
        |eval, args, _| debug_status(eval, args).map(EvalResult::Tagged),
    ));

    registry.register(PrimitiveFn::new_tagged(
        "patina.debug",
        "debug-mode",
        Arity::Exact(1),
        "Enable ('on/'all) or disable ('off) all debugging stages",
        |eval, args, _| debug_mode(eval, args).map(EvalResult::Tagged),
    ));

    // Macro expansion debugging (includes hygiene tracing)
    registry.register(PrimitiveFn::new_tagged(
        "patina.debug",
        "macro-debug-mode",
        Arity::Exact(1),
        "Control macro expansion and hygiene debugging ('on, 'off, 'status)",
        |eval, args, _| macro_debug_mode(eval, args).map(EvalResult::Tagged),
    ));

    // Type introspection (Patina extension)
    registry.register(PrimitiveFn::new_tagged(
        "patina.debug",
        "library?",
        Arity::Exact(1),
        "Returns #t if obj is a library.",
        |eval, args, _| library_p(eval, args).map(EvalResult::Tagged),
    ));
}

pub(super) fn library_p(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    let heap = evaluator.global_env.heap();
    Ok(TaggedValue::boolean(heap.borrow().is_library(args[0])))
}

pub(super) fn debug_enable(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let stage_name = heap
        .borrow()
        .get_symbol_name(args[0])
        .map(|s| s.to_string());

    match stage_name.as_deref() {
        Some(name) => {
            let stage = match name {
                "lex" => DebugStage::Lex,
                "parse" => DebugStage::Parse,
                "eval" => DebugStage::Eval,
                "apply" => DebugStage::Apply,
                "env" => DebugStage::Env,
                "expand" => DebugStage::Expand,
                _ => {
                    return Err(EvalError::TypeError(format!(
                        "Unknown debug stage: {}. Valid: lex, parse, eval, apply, env, expand",
                        name
                    )));
                }
            };

            evaluator.debug.enable(stage);
            Ok(heap.borrow_mut().intern_symbol("enabled"))
        }
        None => Err(EvalError::TypeError(
            "debug-enable expects a symbol (lex, parse, eval, apply, env, expand)".to_string(),
        )),
    }
}

pub(super) fn debug_disable(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let stage_name = heap
        .borrow()
        .get_symbol_name(args[0])
        .map(|s| s.to_string());

    match stage_name.as_deref() {
        Some(name) => {
            let stage = match name {
                "lex" => DebugStage::Lex,
                "parse" => DebugStage::Parse,
                "eval" => DebugStage::Eval,
                "apply" => DebugStage::Apply,
                "env" => DebugStage::Env,
                "expand" => DebugStage::Expand,
                _ => {
                    return Err(EvalError::TypeError(format!(
                        "Unknown debug stage: {}",
                        name
                    )));
                }
            };

            evaluator.debug.disable(stage);
            Ok(heap.borrow_mut().intern_symbol("disabled"))
        }
        None => Err(EvalError::TypeError(
            "debug-disable expects a symbol".to_string(),
        )),
    }
}

pub(super) fn debug_clear(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "0".to_string(),
            actual: args.len(),
        });
    }
    evaluator.debug.clear();
    let heap = evaluator.global_env.heap();
    Ok(heap.borrow_mut().intern_symbol("cleared"))
}

pub(super) fn debug_status(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if !args.is_empty() {
        return Err(EvalError::WrongArity {
            expected: "0".to_string(),
            actual: args.len(),
        });
    }

    let stages = vec!["lex", "parse", "eval", "apply", "env", "expand"];
    let heap = evaluator.global_env.heap();
    let mut enabled = Vec::new();

    for stage_name in stages {
        let stage = match stage_name {
            "lex" => DebugStage::Lex,
            "parse" => DebugStage::Parse,
            "eval" => DebugStage::Eval,
            "apply" => DebugStage::Apply,
            "env" => DebugStage::Env,
            "expand" => DebugStage::Expand,
            _ => continue,
        };

        if evaluator.debug.is_enabled(stage) {
            enabled.push(heap.borrow_mut().intern_symbol(stage_name));
        }
    }

    Ok(heap.borrow_mut().list_from_iter(enabled))
}

pub(super) fn debug_mode(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let mode = heap
        .borrow()
        .get_symbol_name(args[0])
        .map(|s| s.to_string());

    match mode.as_deref() {
        Some("on" | "all") => {
            evaluator.debug.enable_all();
            Ok(heap.borrow_mut().intern_symbol("all-enabled"))
        }
        Some("off") => {
            evaluator.debug.clear();
            Ok(heap.borrow_mut().intern_symbol("disabled"))
        }
        Some(_) => Err(EvalError::TypeError(
            "debug-mode expects 'on, 'off, or 'all".to_string(),
        )),
        None => Err(EvalError::TypeError(
            "debug-mode expects a symbol".to_string(),
        )),
    }
}

pub(super) fn macro_debug_mode(
    evaluator: &Evaluator,
    args: Vec<TaggedValue>,
) -> Result<TaggedValue, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    let heap = evaluator.global_env.heap();
    let mode = heap
        .borrow()
        .get_symbol_name(args[0])
        .map(|s| s.to_string());

    match mode.as_deref() {
        Some("on") => {
            patina_runtime::macro_debug::enable(); // Also enables hygiene_debug
            Ok(heap.borrow_mut().intern_symbol("macro-debug-enabled"))
        }
        Some("off") => {
            patina_runtime::macro_debug::disable(); // Also disables hygiene_debug
            Ok(heap.borrow_mut().intern_symbol("macro-debug-disabled"))
        }
        Some("status") => {
            let status = if patina_runtime::macro_debug::is_enabled() {
                "enabled"
            } else {
                "disabled"
            };
            Ok(heap.borrow_mut().intern_symbol(status))
        }
        _ => Err(EvalError::InvalidSyntax(
            "macro-debug-mode expects 'on, 'off, or 'status".to_string(),
        )),
    }
}
