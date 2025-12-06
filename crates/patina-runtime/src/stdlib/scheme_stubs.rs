//! Stub implementations for R7RS libraries not yet implemented
//!
//! These provide empty library definitions so that code can import them
//! without errors, even though the procedures aren't implemented yet.

use crate::environment::Environment;
use std::rc::Rc;

/// (scheme eval) - Evaluation
pub fn build_scheme_eval(_name: Vec<String>, _env: Rc<Environment>) -> Vec<String> {
    // TODO: Implement eval, environment, scheme-report-environment, null-environment
    vec![]
}

/// (scheme r5rs) - R5RS compatibility
pub fn build_scheme_r5rs(_name: Vec<String>, _env: Rc<Environment>) -> Vec<String> {
    // TODO: Re-export everything from (scheme base) plus R5RS-specific items
    vec![]
}
