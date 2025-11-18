//! (scheme case-lambda) library implementation
//!
//! This library provides the `case-lambda` special form for creating procedures
//! that dispatch based on the number of arguments.
//!
//! # Exports
//!
//! - `case-lambda` - Special form for creating multi-arity procedures

use crate::environment::Environment;
use std::rc::Rc;

/// Build the (scheme case-lambda) library
///
/// Returns the list of exported identifiers.
/// The actual implementation is provided as a special form in the evaluator.
pub fn build_scheme_case_lambda(_name: Vec<String>, _env: Rc<Environment>) -> Vec<String> {
    // case-lambda is a special form, so it's already registered globally
    // We just need to export it as part of this library
    vec!["case-lambda".to_string()]
}
