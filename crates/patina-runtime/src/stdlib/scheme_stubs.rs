//! Stub implementations for R7RS libraries not yet implemented
//!
//! These provide empty library definitions so that code can import them
//! without errors, even though the procedures aren't implemented yet.

use crate::environment::Environment;
use std::rc::Rc;

/// (scheme time) - Time operations
pub fn build_scheme_time(_name: Vec<String>, _env: Rc<Environment>) -> Vec<String> {
    // TODO: Implement current-second, current-jiffy, jiffies-per-second
    vec![]
}

/// (scheme file) - File operations
pub fn build_scheme_file(_name: Vec<String>, _env: Rc<Environment>) -> Vec<String> {
    // TODO: Implement file operations
    // call-with-input-file, call-with-output-file
    // with-input-from-file, with-output-to-file
    // open-input-file, open-output-file, close-input-port, close-output-port
    // file-exists?, delete-file
    vec![]
}

/// (scheme read) - Read operations
pub fn build_scheme_read(_name: Vec<String>, _env: Rc<Environment>) -> Vec<String> {
    // TODO: Implement read
    vec![]
}

/// (scheme write) - Write operations
pub fn build_scheme_write(_name: Vec<String>, _env: Rc<Environment>) -> Vec<String> {
    // TODO: Currently display and write are in (scheme base)
    // This library should have: display, write, write-shared, write-simple
    vec![]
}

/// (scheme eval) - Evaluation
pub fn build_scheme_eval(_name: Vec<String>, _env: Rc<Environment>) -> Vec<String> {
    // TODO: Implement eval, environment, scheme-report-environment, null-environment
    vec![]
}

/// (scheme process-context) - Process context
pub fn build_scheme_process_context(_name: Vec<String>, _env: Rc<Environment>) -> Vec<String> {
    // TODO: Implement command-line, exit, get-environment-variable, get-environment-variables
    vec![]
}

/// (scheme r5rs) - R5RS compatibility
pub fn build_scheme_r5rs(_name: Vec<String>, _env: Rc<Environment>) -> Vec<String> {
    // TODO: Re-export everything from (scheme base) plus R5RS-specific items
    vec![]
}
