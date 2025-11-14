//! Import special form implementation
//!
//! The `import` special form imports library bindings into the current environment.
//!
//! # Syntax
//!
//! ```scheme
//! (import import-set ...)
//! ```
//!
//! # Import Sets
//!
//! ## Basic Library Import
//! ```scheme
//! (import (scheme base))
//! (import (scheme write))
//! ```
//!
//! ## Only (Import Specific Identifiers)
//! ```scheme
//! (import (only (scheme base) + - *))
//! ```
//!
//! ## Except (Import All Except)
//! ```scheme
//! (import (except (scheme base) map))
//! ```
//!
//! ## Prefix
//! ```scheme
//! (import (prefix (scheme base) scm:))
//! ; Now use scm:+ instead of +
//! ```
//!
//! ## Rename
//! ```scheme
//! (import (rename (scheme base) (+ plus) (- minus)))
//! ```
//!
//! # Semantics
//!
//! - Loads the specified libraries
//! - Imports bindings into the current environment
//! - Returns an unspecified value
//! - Import sets can be combined and nested

use super::super::{EvalError, EvalResult, Evaluator};
use super::SpecialForm;
use patina_frontend::LibraryDefinition;
use patina_runtime::{Environment, Value};
use std::rc::Rc;

/// Import special form
///
/// Imports library bindings into the current environment.
pub struct ImportForm;

impl SpecialForm for ImportForm {
    fn name(&self) -> &'static str {
        "import"
    }

    fn help(&self) -> &'static str {
        "(import import-set ...) imports library bindings.\n\
         Import sets:\n\
         - (library name) - import all\n\
         - (only set id ...) - import specific\n\
         - (except set id ...) - import all except\n\
         - (prefix set prefix) - add prefix\n\
         - (rename set (old new) ...) - rename\n\
         Example: (import (scheme base))"
    }

    fn eval(
        &self,
        evaluator: &Evaluator,
        args: &Value,
        env: &Rc<Environment>,
        _in_tail_position: bool,
    ) -> Result<EvalResult, EvalError> {
        // Parse all import sets
        let import_sets = evaluator.collect_list_items(args)?;

        for import_set_expr in import_sets {
            // Parse the import set
            let import_set = LibraryDefinition::parse_import_set(&import_set_expr)
                .map_err(|e| EvalError::InvalidSyntax(format!("Invalid import set: {}", e)))?;

            // Process the import set: this will import the identifiers into env
            evaluator.process_import_for_eval(&import_set, env)?;
        }

        Ok(EvalResult::Value(Value::Unspecified))
    }

    fn validate_syntax(&self, args: &Value) -> Result<(), EvalError> {
        // Check that we have at least one import set
        match args {
            Value::Null => Err(EvalError::InvalidSyntax(
                "import expects at least one import set".to_string(),
            )),
            Value::Pair(_) => Ok(()),
            _ => Err(EvalError::InvalidSyntax(
                "import expects a proper list of import sets".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_import_form_name() {
        let form = ImportForm;
        assert_eq!(form.name(), "import");
    }

    #[test]
    fn test_import_form_help() {
        let form = ImportForm;
        assert!(form.help().contains("import"));
        assert!(form.help().contains("library"));
        assert!(!form.help().is_empty());
    }
}
