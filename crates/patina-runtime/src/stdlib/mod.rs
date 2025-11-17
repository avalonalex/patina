//! Standard R7RS Library Implementations
//!
//! This module contains builder functions for R7RS standard libraries.
//! Each builder populates a library environment with the appropriate primitives
//! and returns a list of exported identifiers.
//!
//! These are registered with RustLibraryLoader and used when libraries are imported.
//!
//! Architecture Note:
//! - The stdlib module is in patina-runtime (not patina-tree-walker) so that
//!   all backends (tree-walker, VM, JIT) can share the same standard library definitions
//! - Each backend implements the actual primitive operations, but the export
//!   lists are defined here

mod chibi_test;
mod patina_debug;
mod scheme_base;
mod scheme_char;
mod scheme_complex;
mod scheme_inexact;
mod scheme_stubs;

pub use chibi_test::{
    build_chibi_test, test_begin, test_end, test_increment_failed, test_increment_passed,
};
pub use patina_debug::build_patina_debug;
pub use scheme_base::build_scheme_base;
pub use scheme_char::build_scheme_char;
pub use scheme_complex::build_scheme_complex;
pub use scheme_inexact::build_scheme_inexact;
pub use scheme_stubs::{
    build_scheme_case_lambda, build_scheme_eval, build_scheme_file, build_scheme_lazy,
    build_scheme_process_context, build_scheme_r5rs, build_scheme_read, build_scheme_time,
    build_scheme_write,
};
