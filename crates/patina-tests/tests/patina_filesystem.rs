//! Public directory operations use the interpreter's VFS on both backends.
//! No supplied libraries or real directory mutations are needed (#205).

// The bare-value `eval_*` forms (#605), deprecated until stage 5e removes them.
#![allow(deprecated)]

use patina_core::{NativeFs, OverlayFs};
use patina_interpreter::{Backend, Interpreter, TreeWalkInterpreter};
use patina_primitives::primitives::io::datum_writer::format_write_tagged;
use patina_vm::VmBackend;
use std::sync::Arc;

fn check<B: Backend>(interp: Interpreter<B>, code: &str, expected: &str) {
    let value = interp
        .eval_program(code)
        .unwrap_or_else(|e| panic!("{} failed: {e}\n{code}", std::any::type_name::<B>()));
    assert_eq!(
        format_write_tagged(value, interp.backend().global_env().heap()),
        expected,
        "{}",
        std::any::type_name::<B>()
    );
}

fn check_on_overlay(code: &str, expected: &str) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("memory-only");
    let program = format!(
        "(import (scheme base) (scheme file) (scheme write) (patina filesystem))
         (define root {root:?})\n{code}"
    );
    let make_fs = || Arc::new(OverlayFs::new(Arc::new(NativeFs)));
    check(
        TreeWalkInterpreter::new_tree_walker_with_fs(make_fs()),
        &program,
        expected,
    );
    check(
        Interpreter::new(VmBackend::with_fs(make_fs())),
        &program,
        expected,
    );
    assert!(!root.exists(), "directory writes escaped the overlay");
}

#[test]
fn public_directory_lifecycle_uses_the_vfs() {
    check_on_overlay(
        r#"
        (define child (string-append root "/z-dir"))
        (define file (string-append root "/a.txt"))
        (define created (create-directory root))
        (define child-created (create-directory child #o700))
        (call-with-output-file file (lambda (p) (display "hello" p)))
        (define entries (directory-files root))
        (define kinds (list (file-directory? root) (file-directory? file)
                            (file-regular? file) (file-regular? root)
                            (file-directory? (string-append root "/absent"))
                            (file-regular? (string-append root "/absent"))))
        (define before (current-directory))
        (define changed (change-directory root))
        (define inside (equal? root (current-directory)))
        (change-directory before)
        (delete-file file)
        (define child-deleted (delete-directory child))
        (define deleted (delete-directory root))
        (list created child-created entries kinds changed inside
              (equal? before (current-directory)) child-deleted deleted
              (file-directory? root))
        "#,
        r#"(#t #t ("." ".." "a.txt" "z-dir") (#t #f #t #f #f #f) #t #t #t #t #t #f)"#,
    );
}

#[test]
fn public_directory_failures_raise_errors() {
    check_on_overlay(
        r#"
        (define missing (string-append root "/missing"))
        (create-directory root)
        (create-directory (string-append root "/child"))
        (list
          (guard (e (#t (error-object? e))) (create-directory root) #f)
          (guard (e (#t (error-object? e))) (create-directory (string-append missing "/child")) #f)
          (guard (e (#t (error-object? e))) (directory-files missing) #f)
          (guard (e (#t (error-object? e))) (delete-directory root) #f)
          (guard (e (#t (error-object? e))) (delete-directory missing) #f)
          (guard (e (#t (error-object? e))) (change-directory missing) #f))
        "#,
        "(#t #t #t #t #t #t)",
    );
}
