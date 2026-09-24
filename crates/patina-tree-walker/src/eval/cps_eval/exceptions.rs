//! Exception routing for CPS evaluation
//!
//! This module contains functions for routing errors through the CPS
//! exception handler stack instead of propagating them as Rust errors.
//!
//! ## Error Routing Strategy
//!
//! All "catchable" errors are routed through Scheme exception handlers when
//! handlers are installed. Only truly non-catchable errors (internal bugs,
//! continuation escapes) bypass the handler stack.
//!
//! This provides R7RS-compliant behavior where user code can catch and handle
//! runtime errors using `guard` or `with-exception-handler`.

use super::CpsEvaluator;
use super::types::{ContEnv, ContValue, ExceptionHandler, PromptFrame, StepResult};
use crate::eval::error::EvalError;
use patina_core::DynamicWindRecord;
use patina_core::ExceptionKind;
use std::collections::HashSet;
use std::rc::Rc;

impl<'a> CpsEvaluator<'a> {
    /// Route catchable errors through CPS exception handlers
    ///
    /// All errors except `InternalError` and `ContinuationEscape` are catchable
    /// and will be routed through any installed exception handlers. This enables
    /// Scheme code to catch runtime errors using `guard` or `with-exception-handler`.
    ///
    /// If no exception handlers are installed, the error propagates as a Rust Err.
    pub(super) fn maybe_route_error_through_cps(
        &self,
        err: EvalError,
        cont: ContValue,
        cont_env: ContEnv,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        // Non-catchable errors always propagate as Rust errors
        if !err.is_catchable() {
            return Err(err);
        }

        // Convert EvalError to exception kind and message
        // Note: irritants are not used for runtime errors but kept for API compatibility
        let (exception_kind, message): (ExceptionKind, String) = match &err {
            // Lookup errors
            EvalError::UndefinedVariable(name) => (
                ExceptionKind::Error,
                format!("Undefined variable: {}", name),
            ),

            // Application errors
            EvalError::NotAProcedure(desc) => {
                (ExceptionKind::Error, format!("Not a procedure: {}", desc))
            }

            // Arity errors
            EvalError::WrongArity { expected, actual } => (
                ExceptionKind::Error,
                format!(
                    "Wrong number of arguments: expected {}, got {}",
                    expected, actual
                ),
            ),

            // Type errors
            EvalError::TypeError(msg) => (ExceptionKind::Error, msg.clone()),

            // Domain errors
            EvalError::DivisionByZero => (ExceptionKind::Error, "Division by zero".to_string()),

            // Bounds errors
            EvalError::IndexOutOfBounds(msg) => (ExceptionKind::Error, msg.clone()),

            // I/O errors - classify as file-error when appropriate
            EvalError::IOError(msg) => {
                let kind = if msg.contains("Cannot open")
                    || msg.contains("Cannot delete")
                    || msg.contains("Cannot read")
                    || msg.contains("Cannot write")
                    || msg.contains("No such file")
                    || msg.contains("file")
                {
                    ExceptionKind::FileError
                } else {
                    ExceptionKind::Error
                };
                (kind, msg.clone())
            }

            // Syntax/read errors
            EvalError::InvalidSyntax(msg) => {
                let kind = if msg.contains("read:") || msg.contains("parse") {
                    ExceptionKind::ReadError
                } else {
                    ExceptionKind::Error
                };
                (kind, msg.clone())
            }

            // Already a Scheme exception - use its kind
            EvalError::SchemeException {
                kind,
                message,
                irritants_display: _,
            } => (kind.clone(), message.clone()),

            // Located error: include location in exception message
            EvalError::WithLocation { error, location } => {
                let detail = error.to_error_detail();
                let msg = format!("{} at {}", detail.message, location);
                (detail.kind.to_exception_kind(), msg)
            }

            // Internal errors, continuation escapes, and desugar rejections
            // are not catchable (handled by is_catchable() check above)
            EvalError::InternalError(_)
            | EvalError::ContinuationEscape
            | EvalError::DesugarError(_) => {
                unreachable!("Non-catchable errors should have been filtered")
            }
        };

        // If there are exception handlers, route through them
        if let Some(handler_entry) = exception_handlers.last().cloned() {
            let exception_tagged = self
                .evaluator
                .global_env
                .heap()
                .borrow_mut()
                .alloc_exception(
                    exception_kind,
                    message,
                    vec![], // No irritants for runtime errors
                );

            // Pop the handler (it's been invoked)
            let new_handlers = exception_handlers[..exception_handlers.len() - 1].to_vec();

            // The wind stack is left as the error found it, for the reason
            // `apply_raise` records: a raise crosses no dynamic extent, so
            // the unwind belongs to `guard`'s escape, not here. Routing a
            // Rust-level error through the handlers has to look exactly like
            // a `raise` of the same object, or the two paths disagree about
            // where the handler runs — which is what made `(error "x")` and
            // `(raise 'x)` order the after-thunk differently.

            // Create continuation for when handler returns
            // Runtime errors are non-continuable (only user-raised exceptions can be continuable)
            let raise_return_cont = ContValue::RaiseHandlerReturn {
                continuable: false,
                original_exception: Some(exception_tagged),
                original_cont: Box::new(cont),
                popped_handler: None,
            };

            // Handler is already TaggedValue - use directly for ApplyProc.proc
            // Call the handler in the error's own dynamic extent
            Ok(StepResult::ApplyProc {
                proc: handler_entry.handler,
                args: vec![exception_tagged],
                cont: raise_return_cont,
                env: self.evaluator.global_env.clone(),
                cont_env,
                prompt_stack,
                dynamic_winds,
                exception_handlers: new_handlers,
            })
        } else {
            // No handlers - propagate the error as-is
            Err(unhandled(err, &cont, &cont_env, &prompt_stack))
        }
    }
}

/// `err`, which no handler takes, on its way out to be reported: noting the
/// `exit` it interrupted, when it was raised by an after thunk that `exit` is
/// running or by something that thunk called, so that the runner reporting it
/// still ends the process (`patina_runtime::exit_status`).
///
/// Called wherever an error leaves the evaluator unhandled, with the
/// continuation it was raised in.
pub(super) fn unhandled(
    err: EvalError,
    cont: &ContValue,
    cont_env: &ContEnv,
    prompt_stack: &[PromptFrame],
) -> EvalError {
    if let Some(status) = exit_in_progress(cont, cont_env, prompt_stack) {
        patina_runtime::exit_status::note_interrupted_exit(status);
    }
    err
}

/// The status of the `exit` whose travel `cont` runs on into, if any.
///
/// An error raised where `cont` is the continuation was raised inside that
/// travel exactly when the chain from `cont` reaches the jump to an exit's
/// landing (`apply_exit`): a travel something abandoned is no longer on any
/// chain, and a continuation captured inside one of its thunks carries the jump
/// back when it is re-entered. It is the tree-walker's answer to the VM's
/// frame scan (`exit_in_progress` in `patina-vm`), for a continuation that is a
/// chain rather than a stack. The rest of a `Local` is named in its
/// continuation environment, the rest of a prompt body waits on the prompt
/// stack, and a jump runs on into its target, so the walk follows all three,
/// and memoises the environments and continuations chains share.
fn exit_in_progress(
    cont: &ContValue,
    cont_env: &ContEnv,
    prompt_stack: &[PromptFrame],
) -> Option<i32> {
    let mut conts: Vec<&ContValue> = vec![cont];
    conts.extend(prompt_stack.iter().map(|frame| &frame.cont));
    let mut envs: Vec<&ContEnv> = vec![cont_env];
    let mut seen_envs = HashSet::new();
    let mut seen_targets = HashSet::new();
    loop {
        if let Some(env) = envs.pop() {
            if seen_envs.insert(env.gc_identity()) {
                conts.extend(env.iter().map(|(_, value)| value));
            }
            continue;
        }
        let mut targets = Vec::new();
        match conts.pop()? {
            ContValue::ExitLanding { status } => return Some(*status),
            ContValue::Local { cont_env, .. } => envs.push(cont_env),
            ContValue::Captured(target) | ContValue::Jump { target, .. } => targets.push(target),
            ContValue::ComposableInvokeStep { target, cont, .. } => {
                conts.push(cont);
                targets.push(target);
            }
            ContValue::CallWithValuesConsumer { original_cont, .. }
            | ContValue::ForceCache { original_cont, .. }
            | ContValue::ResumePrimitive { original_cont, .. }
            | ContValue::DynamicWindCleanup { original_cont, .. }
            | ContValue::DynamicWindAfterDone { original_cont, .. }
            | ContValue::ExceptionHandlerCleanup { original_cont }
            | ContValue::RaiseHandlerReturn { original_cont, .. } => conts.push(original_cont),
            ContValue::DynamicWindSetup { cleanup_cont, .. } => conts.push(cleanup_cont),
            ContValue::AbortLanding { cont, .. } => conts.push(cont),
            ContValue::Halt | ContValue::PromptBoundary { .. } => {}
        }
        for target in targets {
            if seen_targets.insert(Rc::as_ptr(target)) {
                conts.extend(target.resume.as_ref());
                envs.push(&target.captured_cont_env);
            }
        }
    }
}
