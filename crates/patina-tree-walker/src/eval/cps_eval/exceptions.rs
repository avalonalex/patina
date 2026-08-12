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

            // Unwind dynamic-wind after-thunks back to the handler's installation point
            self.run_wind_handlers(&dynamic_winds, &handler_entry.dynamic_winds)?;
            let handler_winds = handler_entry.dynamic_winds.clone();

            // Create continuation for when handler returns
            // Runtime errors are non-continuable (only user-raised exceptions can be continuable)
            let raise_return_cont = ContValue::RaiseHandlerReturn {
                continuable: false,
                original_exception: Some(exception_tagged),
                original_cont: Box::new(cont),
                popped_handler: None,
            };

            // Handler is already TaggedValue - use directly for ApplyProc.proc
            // Call the handler with the dynamic winds restored to handler's installation point
            Ok(StepResult::ApplyProc {
                proc: handler_entry.handler,
                args: vec![exception_tagged],
                cont: raise_return_cont,
                env: self.evaluator.global_env.clone(),
                cont_env,
                prompt_stack,
                dynamic_winds: handler_winds,
                exception_handlers: new_handlers,
            })
        } else {
            // No handlers - propagate the error as-is
            Err(err)
        }
    }
}
