//! Compatibility wrapper for the original tree-walker embedding API.
//!
//! New hosts should use `VmInterpreter::new_vm()` (recommended) or
//! `TreeWalkInterpreter::new_tree_walker()`. This adapter retains its original
//! constructors, evaluator access and `PipelineError` return type; all program
//! reading and execution are delegated to the common interpreter.
#![allow(deprecated)]

use crate::{Environment, Evaluator, PipelineError, TaggedValue, TreeWalkInterpreter};
use std::rc::Rc;

/// Legacy tree-walker convenience API. It never selects the VM implicitly.
#[deprecated(note = "use VmInterpreter::new_vm() or TreeWalkInterpreter::new_tree_walker()")]
pub struct SimpleInterpreter {
    interpreter: TreeWalkInterpreter,
}

impl SimpleInterpreter {
    pub fn new() -> Self {
        Self {
            interpreter: TreeWalkInterpreter::new_tree_walker(),
        }
    }

    /// Evaluate one expression; reject an unreadable suffix (#329).
    #[expect(
        clippy::disallowed_methods,
        reason = "the deprecated adapter over `Interpreter::eval_str`: holds nothing"
    )]
    pub fn eval_str(&self, code: &str) -> Result<TaggedValue, PipelineError> {
        self.interpreter.eval_str(code).map_err(Into::into)
    }

    /// Evaluate every form; return unspecified for an empty program.
    #[expect(
        clippy::disallowed_methods,
        reason = "the deprecated adapter over `Interpreter::eval_program`: holds nothing"
    )]
    pub fn eval_program(&self, code: &str) -> Result<TaggedValue, PipelineError> {
        self.interpreter.eval_program(code).map_err(Into::into)
    }

    pub fn global_env(&self) -> Rc<Environment> {
        self.interpreter.global_env()
    }

    pub fn evaluator(&self) -> &Evaluator {
        self.interpreter.evaluator()
    }

    pub fn display_tagged(&self, tv: TaggedValue) -> String {
        self.interpreter.display_tagged(tv)
    }
}

impl Default for SimpleInterpreter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[expect(
    clippy::disallowed_methods,
    reason = "unit tests evaluate through the adapter from outside any loop, as an embedder \
              does, and none reads a value from one evaluation after another (#605's shape)"
)]
mod tests {
    use super::*;

    #[test]
    fn test_eval_str() {
        let interp = SimpleInterpreter::new();
        let result = interp.eval_str("(+ 1 2 3)").unwrap();
        assert_eq!(result.as_fixnum(), Some(6));
    }

    #[test]
    fn test_eval_program() {
        let interp = SimpleInterpreter::new();
        let code = r#"
            (define x 10)
            (define y 20)
            (+ x y)
        "#;
        let result = interp.eval_program(code).unwrap();
        assert_eq!(result.as_fixnum(), Some(30));
    }

    #[test]
    fn test_global_env() {
        let interp = SimpleInterpreter::new();
        let env = interp.global_env();

        // Global environment should have primitives loaded
        assert!(env.get("cons").is_some());
        assert!(env.get("+").is_some());
    }

    #[test]
    fn test_macros() {
        let interp = SimpleInterpreter::new();
        let result = interp.eval_str("(or #f #t)").unwrap();
        assert_eq!(result, TaggedValue::TRUE);
    }
}
