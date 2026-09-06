//! Tests for the (chibi test) framework.
//!
//! These were `#[ignore]`d as "requires the module system", which stopped being
//! true long before the ignore was removed. They now exercise the real upstream
//! `(chibi test)`, rather than the hand-written subset that used to stand in
//! for it.
//!
//! Patina supplies that library from `test-lib/` rather than bundling it, so
//! the interpreters here come from `common`, which puts that root on the
//! search path — this lane's spelling of the `-A test-lib` the shell lanes
//! pass.
//!
//! Both backends, per `common`'s convention: loading this library exercises
//! macro expansion and library resolution, which is exactly where the two
//! diverge, and the VM is the default backend the R7RS lane runs on.

mod common;
use common::{tree_walker_interpreter, vm_interpreter};
use patina_interpreter::Interpreter;
use patina_runtime::Backend;

/// Run `program` on both backends and hand each result to `check`, labelled.
fn on_both_backends(program: &str, check: impl Fn(&str, Result<patina_core::TaggedValue, String>)) {
    fn run<B: Backend>(
        interp: &Interpreter<B>,
        program: &str,
    ) -> Result<patina_core::TaggedValue, String> {
        interp.eval_program(program).map_err(|e| e.to_string())
    }
    check("tree-walker", run(&tree_walker_interpreter(), program));
    check("vm", run(&vm_interpreter(), program));
}

#[test]
fn test_chibi_test_framework_loads() {
    on_both_backends(
        r#"
        (import (scheme base) (chibi test))
        (test-begin "test-suite")
        (test 3 (+ 1 2))
        (test-end)
    "#,
        |backend, result| {
            assert!(
                result.is_ok(),
                "[{backend}] failed to run chibi test: {:?}",
                result
            );
        },
    );
}

#[test]
fn test_chibi_test_basic_functionality() {
    on_both_backends(
        r#"
        (import (scheme base) (chibi test))

        (test-begin "arithmetic")
        (test 6 (+ 1 2 3))
        (test 10 (* 2 5))
        (test-end)

        #t
    "#,
        |backend, result| {
            let value = result.unwrap_or_else(|e| panic!("[{backend}] failed: {e}"));
            assert_eq!(value, patina_core::TaggedValue::TRUE, "[{backend}]");
        },
    );
}
