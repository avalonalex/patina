use super::super::Evaluator;
use super::super::debug::DebugStage;
use super::super::error::EvalError;
use super::registry::PrimitiveRegistry;
use patina_runtime::value::Value;

/// Register all debug primitives in the registry
pub(super) fn register(registry: &mut PrimitiveRegistry) {
    use super::super::EvalResult;
    use super::registry::PrimitiveFn;
    use patina_runtime::value::Arity;

    // Evaluation debugging primitives
    registry.register(PrimitiveFn::new(
        "patina.debug",
        "debug-enable",
        Arity::Exact(1),
        "Enable debugging for a specific stage (lex, parse, eval, apply, env, expand)",
        |eval, args, _| debug_enable(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "patina.debug",
        "debug-disable",
        Arity::Exact(1),
        "Disable debugging for a specific stage",
        |eval, args, _| debug_disable(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "patina.debug",
        "debug-clear",
        Arity::Exact(0),
        "Disable all debugging stages",
        |eval, args, _| debug_clear(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "patina.debug",
        "debug-status",
        Arity::Exact(0),
        "Return a list of currently enabled debug stages",
        |eval, args, _| debug_status(eval, args).map(EvalResult::Value),
    ));

    registry.register(PrimitiveFn::new(
        "patina.debug",
        "debug-mode",
        Arity::Exact(1),
        "Enable ('on/'all) or disable ('off) all debugging stages",
        |eval, args, _| debug_mode(eval, args).map(EvalResult::Value),
    ));

    // Macro expansion debugging
    registry.register(PrimitiveFn::new(
        "patina.debug",
        "macro-debug-mode",
        Arity::Exact(1),
        "Control macro expansion debugging ('on, 'off, 'status)",
        |eval, args, _| macro_debug_mode(eval, args).map(EvalResult::Value),
    ));
}

pub(super) fn debug_enable(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "debug-enable")?;

    match &args[0] {
        Value::Symbol(stage_name) => {
            let stage = match stage_name.as_ref() {
                "lex" => DebugStage::Lex,
                "parse" => DebugStage::Parse,
                "eval" => DebugStage::Eval,
                "apply" => DebugStage::Apply,
                "env" => DebugStage::Env,
                "expand" => DebugStage::Expand,
                _ => {
                    return Err(EvalError::TypeError(format!(
                        "Unknown debug stage: {}. Valid: lex, parse, eval, apply, env, expand",
                        stage_name
                    )));
                }
            };

            evaluator.debug.enable(stage);
            Ok(Value::Symbol("enabled".into()))
        }
        _ => Err(EvalError::TypeError(
            "debug-enable expects a symbol (lex, parse, eval, apply, env, expand)".to_string(),
        )),
    }
}

pub(super) fn debug_disable(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "debug-disable")?;

    match &args[0] {
        Value::Symbol(stage_name) => {
            let stage = match stage_name.as_ref() {
                "lex" => DebugStage::Lex,
                "parse" => DebugStage::Parse,
                "eval" => DebugStage::Eval,
                "apply" => DebugStage::Apply,
                "env" => DebugStage::Env,
                "expand" => DebugStage::Expand,
                _ => {
                    return Err(EvalError::TypeError(format!(
                        "Unknown debug stage: {}",
                        stage_name
                    )));
                }
            };

            evaluator.debug.disable(stage);
            Ok(Value::Symbol("disabled".into()))
        }
        _ => Err(EvalError::TypeError(
            "debug-disable expects a symbol".to_string(),
        )),
    }
}

pub(super) fn debug_clear(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 0, "debug-clear")?;
    evaluator.debug.clear();
    Ok(Value::Symbol("cleared".into()))
}

pub(super) fn debug_status(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 0, "debug-status")?;

    let stages = vec!["lex", "parse", "eval", "apply", "env", "expand"];
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
            enabled.push(Value::Symbol(stage_name.into()));
        }
    }

    Ok(evaluator.list_from_vec(enabled))
}

pub(super) fn debug_mode(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "debug-mode")?;

    match &args[0] {
        Value::Symbol(mode) => match mode.as_ref() {
            "on" | "all" => {
                evaluator.debug.enable_all();
                Ok(Value::Symbol("all-enabled".into()))
            }
            "off" => {
                evaluator.debug.clear();
                Ok(Value::Symbol("disabled".into()))
            }
            _ => Err(EvalError::TypeError(
                "debug-mode expects 'on, 'off, or 'all".to_string(),
            )),
        },
        _ => Err(EvalError::TypeError(
            "debug-mode expects a symbol".to_string(),
        )),
    }
}

pub(super) fn macro_debug_mode(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "macro-debug-mode")?;

    match &args[0] {
        Value::Symbol(s) if s.as_ref() == "on" => {
            patina_runtime::macro_debug::enable();
            Ok(Value::Symbol("macro-debug-enabled".into()))
        }
        Value::Symbol(s) if s.as_ref() == "off" => {
            patina_runtime::macro_debug::disable();
            Ok(Value::Symbol("macro-debug-disabled".into()))
        }
        Value::Symbol(s) if s.as_ref() == "status" => {
            let status = if patina_runtime::macro_debug::is_enabled() {
                "enabled"
            } else {
                "disabled"
            };
            Ok(Value::Symbol(status.into()))
        }
        _ => Err(EvalError::InvalidSyntax(
            "macro-debug-mode expects 'on, 'off, or 'status".to_string(),
        )),
    }
}
