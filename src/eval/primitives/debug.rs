use super::super::debug::DebugStage;
use super::super::error::EvalError;
use super::super::Evaluator;
use crate::value::Value;

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
                    )))
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
                    )))
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
