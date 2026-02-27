//! Procedure application logic
//!
//! This module handles the application of procedures to arguments, including:
//! - `apply()` - Apply a procedure to evaluated arguments (TaggedValue-native)
//! - Arity checking is handled by the PrimitiveRegistry (registry.rs)

use patina_core::Procedure;
use patina_core::TaggedValue;

use super::Evaluator;
use super::error::EvalError;

impl Evaluator {
    /// Apply a procedure to a vector of evaluated arguments
    ///
    /// The `in_tail_position` parameter enables tail call optimization for primitives.
    /// When true, primitives like call-with-values can return TailCallPrimitive to
    /// participate in the trampoline loop.
    pub(super) fn apply(
        &self,
        proc: TaggedValue,
        args: Vec<TaggedValue>,
        in_tail_position: bool,
    ) -> Result<super::EvalResult, EvalError> {
        let heap = self.global_env.heap();

        // Debug trace entry
        if self.debug.is_enabled(super::debug::DebugStage::Apply) {
            use crate::eval::primitives::io::datum_writer::format_display_tagged;
            let proc_str = format_display_tagged(proc, heap);
            let args_str = args
                .iter()
                .map(|tv| format_display_tagged(*tv, heap))
                .collect::<Vec<_>>()
                .join(" ");
            tracing::debug!(
                target: "patina_tree_walker::apply",
                indent = %self.debug.current_indent(),
                proc = %proc_str,
                args = %args_str,
                "Applying"
            );
            self.debug.indent();
        }

        // 1. Check for closures (TAG_CLOSURE) — CPS lambdas use this tag
        let result = if proc.is_closure() {
            use crate::eval::cps_eval::CpsEvaluator;
            let cps_eval = CpsEvaluator::new(self);
            let result_tagged = cps_eval.apply_from_direct_tagged(proc, args)?;
            Ok(super::EvalResult::Tagged(result_tagged))
        }
        // 2. Check for procedures
        //    Extract to let binding first to drop heap borrow before calling into primitives
        else {
            let proc_opt = heap.borrow().get_procedure(proc);
            if let Some(proc_box) = proc_opt {
                match proc_box.as_ref() {
                    Procedure::Primitive { .. } => {
                        // Arity is checked inside apply_primitive_tagged → registry.call_tagged
                        self.apply_primitive_tagged(&proc_box, args, in_tail_position)
                    }
                    Procedure::CpsLambda { .. } => {
                        use crate::eval::cps_eval::CpsEvaluator;
                        let cps_eval = CpsEvaluator::new(self);
                        let result_tagged = cps_eval.apply_from_direct_tagged(proc, args)?;
                        Ok(super::EvalResult::Tagged(result_tagged))
                    }
                }
            }
            // 3. Check for continuations — error in direct mode
            else {
                let is_continuation = heap.borrow().get_continuation(proc).is_some();
                if is_continuation {
                    Err(EvalError::InternalError(
                        "Continuations can only be invoked in CPS evaluation mode. \
                         This continuation was captured in CPS mode but is being invoked in direct mode."
                            .to_string(),
                    ))
                }
                // 4. Check for parameters
                else {
                    let param_opt = heap.borrow().get_parameter(proc);
                    if let Some((values, converter)) = param_opt {
                        match args.len() {
                            0 => {
                                // Get current value (top of stack)
                                let stack = values.borrow();
                                let current_tagged = stack.last().ok_or_else(|| {
                                    EvalError::InvalidSyntax("parameter stack is empty".to_string())
                                })?;
                                Ok(super::EvalResult::Tagged(*current_tagged))
                            }
                            1 => {
                                // Set value (replace top of stack after applying converter)
                                let new_val = if let Some(conv) = converter {
                                    // Apply converter — conv and args[0] are already TaggedValue
                                    let result = self.apply(conv, vec![args[0]], false)?;
                                    match result {
                                        super::EvalResult::Tagged(tv) => tv,
                                        super::EvalResult::TailCallPrimitive { .. } => {
                                            return Err(EvalError::InvalidSyntax(
                                                "parameter converter returned non-value"
                                                    .to_string(),
                                            ));
                                        }
                                    }
                                } else {
                                    args[0]
                                };

                                // Set the new value (replace top of stack)
                                let mut stack = values.borrow_mut();
                                if let Some(last) = stack.last_mut() {
                                    *last = new_val;
                                } else {
                                    return Err(EvalError::InvalidSyntax(
                                        "parameter stack is empty".to_string(),
                                    ));
                                }
                                Ok(super::EvalResult::Tagged(TaggedValue::UNSPECIFIED))
                            }
                            _ => Err(EvalError::WrongArity {
                                expected: "0 or 1".to_string(),
                                actual: args.len(),
                            }),
                        }
                    }
                    // 5. Not callable
                    else {
                        let heap_ref = heap.borrow();
                        let type_name = heap_ref.type_name(proc);
                        Err(EvalError::NotAProcedure(type_name.to_string()))
                    }
                }
            }
        };

        // Debug trace exit
        if self.debug.is_enabled(super::debug::DebugStage::Apply) {
            self.debug.dedent();
            match &result {
                Ok(super::EvalResult::TailCallPrimitive { proc, args }) => {
                    use crate::eval::primitives::io::datum_writer::format_display_tagged;
                    let proc_str = format_display_tagged(*proc, heap);
                    let args_str = args
                        .iter()
                        .map(|tv| format_display_tagged(*tv, heap))
                        .collect::<Vec<_>>()
                        .join(" ");
                    tracing::debug!(
                        target: "patina_tree_walker::apply",
                        indent = %self.debug.current_indent(),
                        proc = %proc_str,
                        args = %args_str,
                        "=> tail call primitive"
                    );
                }
                Ok(super::EvalResult::Tagged(tv)) => {
                    tracing::debug!(
                        target: "patina_tree_walker::apply",
                        indent = %self.debug.current_indent(),
                        result = ?tv,
                        "=> tagged value"
                    );
                }
                Err(e) => {
                    tracing::debug!(
                        target: "patina_tree_walker::apply",
                        indent = %self.debug.current_indent(),
                        error = %e,
                        "=> error"
                    );
                }
            }
        }

        result
    }
}
