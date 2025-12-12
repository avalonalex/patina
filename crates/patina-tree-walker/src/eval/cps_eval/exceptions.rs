//! Exception routing for CPS evaluation
//!
//! This module contains functions for routing errors through the CPS
//! exception handler stack instead of propagating them as Rust errors.

use super::CpsEvaluator;
use super::types::{ContValue, ExceptionHandler, PromptFrame, StepResult};
use crate::eval::error::EvalError;
use patina_core::value::{DynamicWindRecord, Value};
use std::collections::HashMap;
use std::rc::Rc;

impl<'a> CpsEvaluator<'a> {
    /// Convert certain errors (IOError, InvalidSyntax) to CPS-routed exceptions
    ///
    /// If there are exception handlers installed, convert the error to a Scheme
    /// exception value and route it through the handler stack. Otherwise,
    /// propagate the error as a Rust Err.
    pub(super) fn maybe_route_error_through_cps(
        &self,
        err: EvalError,
        cont: ContValue,
        cont_env: HashMap<Rc<str>, ContValue>,
        prompt_stack: Vec<PromptFrame>,
        dynamic_winds: Vec<DynamicWindRecord>,
        exception_handlers: Vec<ExceptionHandler>,
    ) -> Result<StepResult, EvalError> {
        // Determine if this error should be converted to a CPS exception
        let (exception_kind, message) = match &err {
            EvalError::IOError(msg) => {
                // Check if it's a file error (contains "Cannot open", "Cannot delete", etc.)
                let kind = if msg.contains("Cannot open")
                    || msg.contains("Cannot delete")
                    || msg.contains("Cannot read")
                    || msg.contains("Cannot write")
                    || msg.contains("No such file")
                {
                    patina_core::ExceptionKind::FileError
                } else {
                    patina_core::ExceptionKind::Error
                };
                (kind, msg.clone())
            }
            EvalError::InvalidSyntax(msg) => {
                // Read errors typically contain "read:" in the message
                let kind = if msg.contains("read:") {
                    patina_core::ExceptionKind::ReadError
                } else {
                    patina_core::ExceptionKind::Error
                };
                (kind, msg.clone())
            }
            // Other errors: propagate as-is
            _ => return Err(err),
        };

        // If there are exception handlers, route through them
        if let Some(handler_entry) = exception_handlers.last().cloned() {
            // Create exception object
            let exception = Value::Exception(Rc::new(patina_core::ExceptionObject {
                kind: exception_kind,
                message,
                irritants: vec![],
            }));

            // Pop the handler (it's been invoked)
            let new_handlers = exception_handlers[..exception_handlers.len() - 1].to_vec();

            // Create continuation for when handler returns
            // Since these are system errors, treat as non-continuable
            let raise_return_cont = ContValue::RaiseHandlerReturn {
                continuable: false,
                original_exception: Some(exception.clone()),
                original_cont: Box::new(cont),
            };

            // Call the handler with the exception
            Ok(StepResult::ApplyProc {
                proc: handler_entry.handler,
                args: vec![exception],
                cont: raise_return_cont,
                env: self.evaluator.global_env.clone(),
                cont_env,
                prompt_stack,
                dynamic_winds,
                exception_handlers: new_handlers,
            })
        } else {
            // No handlers - propagate the error as-is
            Err(err)
        }
    }
}
