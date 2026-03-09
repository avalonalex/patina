//! `Backend` trait implementation for the VM.
//!
//! `VmBackend` wraps `VmState` and implements the `patina_runtime::Backend`
//! trait so the VM can be used anywhere a `TreeWalker` is accepted, including
//! `Interpreter<VmBackend>` and the REPL.
//!
//! ## Error bridging
//!
//! `Backend::Error` must be `Send + Sync + 'static`, but `VmError` holds
//! `Symbol = Rc<str>` which is not `Send`. We define `VmBackendError` — a
//! thin wrapper that converts all variable names to `String` on construction.

use crate::compiler::compile;
use crate::error::VmError;
use crate::runtime::{VmState, execute};
use patina_core::environment::Environment;
use patina_core::tagged_value::TaggedValue;
use patina_frontend::Desugarer;
use patina_runtime::Backend;
use std::cell::RefCell;
use std::rc::Rc;

// ─────────────────────────────────────────────────────────────────────────────
// VmBackendError — Send + Sync wrapper around VmError
// ─────────────────────────────────────────────────────────────────────────────

/// Backend-visible error: all variable names converted to `String` so the type
/// is `Send + Sync + 'static` as required by the `Backend` trait.
#[derive(Debug, thiserror::Error)]
pub enum VmBackendError {
    #[error("compile error: {0}")]
    Compile(String),

    #[error("runtime error: {0}")]
    Runtime(String),

    #[error("desugar error: {0}")]
    Desugar(String),
}

impl From<VmError> for VmBackendError {
    fn from(e: VmError) -> Self {
        VmBackendError::Runtime(e.to_string())
    }
}

impl From<patina_frontend::DesugarError> for VmBackendError {
    fn from(e: patina_frontend::DesugarError) -> Self {
        VmBackendError::Desugar(e.to_string())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// VmBackend
// ─────────────────────────────────────────────────────────────────────────────

/// VM backend implementing the `Backend` trait.
///
/// Holds a `VmState` and compiles + executes each expression on demand.
/// The heap is shared with the parser so `TaggedValue` indices remain valid.
pub struct VmBackend {
    state: RefCell<VmState>,
    global_env: Rc<Environment>,
}

impl VmBackend {
    /// Create a new VM backend with a fresh environment and primitive registry.
    pub fn new() -> Self {
        let global_env = Rc::new(Environment::new());
        let mut state = VmState::new(Rc::clone(&global_env));
        state.install_primitives();
        VmBackend {
            state: RefCell::new(state),
            global_env,
        }
    }
}

impl Default for VmBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend for VmBackend {
    type Error = VmBackendError;

    fn eval(&self, expr: TaggedValue, _env: &Rc<Environment>) -> Result<TaggedValue, Self::Error> {
        // Desugar: TaggedValue → CoreExpr.
        // We always evaluate in the global environment; the `env` parameter is
        // ignored for now (same as the tree-walker does for top-level defines).
        let desugarer = Desugarer::with_env(Rc::clone(&self.global_env));
        let heap = self.global_env.heap().clone();
        let core_expr = desugarer
            .desugar_tagged(expr, &heap)
            .map_err(|e| VmBackendError::Desugar(e.to_string()))?;

        // Compile: CoreExpr → CodeObject (5-pass pipeline).
        let (top, nested) =
            compile(&core_expr).map_err(|e| VmBackendError::Compile(e.to_string()))?;

        let top_id = top.id;
        let mut state = self.state.borrow_mut();
        state.load(top);
        state.load_all(nested);

        // Execute.
        let result = execute(&mut state, top_id)?;
        Ok(result)
    }

    fn global_env(&self) -> &Rc<Environment> {
        &self.global_env
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use patina_interpreter::Interpreter;

    fn eval(code: &str) -> TaggedValue {
        let backend = VmBackend::new();
        let interp = Interpreter::new(backend);
        // Use eval_program to handle multiple top-level expressions.
        interp.eval_program(code).expect("eval failed")
    }

    #[test]
    fn addition() {
        assert_eq!(eval("(+ 1 2)").as_fixnum(), Some(3));
    }

    #[test]
    fn conditional() {
        assert_eq!(eval("(if #t 42 0)").as_fixnum(), Some(42));
    }

    #[test]
    fn define_and_ref() {
        assert_eq!(eval("(begin (define x 10) x)").as_fixnum(), Some(10));
    }

    #[test]
    fn lambda_call() {
        assert_eq!(eval("((lambda (x) (+ x 1)) 41)").as_fixnum(), Some(42));
    }

    #[test]
    fn closure_capture() {
        assert_eq!(
            eval("((lambda (x) ((lambda () x))) 99)").as_fixnum(),
            Some(99)
        );
    }

    #[test]
    fn tail_recursive_fibonacci() {
        let code = "(define (fib-iter n a b)
                      (if (= n 0)
                          a
                          (fib-iter (- n 1) b (+ a b))))
                    (fib-iter 10 0 1)";
        assert_eq!(eval(code).as_fixnum(), Some(55));
    }
}
