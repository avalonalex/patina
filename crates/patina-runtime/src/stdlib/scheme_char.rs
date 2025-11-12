//! (scheme char) - Character operations
//!
//! Currently re-exports char operations from (scheme base)
//! TODO: Implement full R7RS char library

use crate::environment::Environment;
use std::rc::Rc;

pub fn build_scheme_char(_name: Vec<String>, _env: Rc<Environment>) -> Vec<String> {
    // TODO: Add char-specific operations
    // For now, return empty - char? is already in (scheme base)
    vec![]
}
