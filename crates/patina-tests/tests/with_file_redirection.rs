//! How `with-input-from-file` / `with-output-to-file` give the port back.
//!
//! The happy path is covered in `vfs_file_io.rs`; this file is the exit paths,
//! which is what changed when the two moved from Rust primitives to Scheme
//! (`lib/scheme/file/redirect.scm`). A `call/cc` escape out of the thunk used
//! to crash the VM outright. History in `PRD/ARCHIVE/TRACK_L_SNOW_LIBRARIES_PRD.md` §6.

mod common;
use common::{assert_program_eval_to, scratch_path as scratch};
use tempfile::TempDir;

/// The redirection ends when the thunk does: `display` afterwards must not
/// still be going to the file.
#[test]
fn test_the_previous_port_is_restored_after_a_normal_return() {
    let dir = TempDir::new().expect("temp dir");
    let path = scratch(&dir, "restored.txt");
    assert_program_eval_to(
        &format!(
            r#"(import (scheme base) (scheme file) (scheme write))
               (define sink (open-output-string))
               (parameterize ((current-output-port sink))
                 (with-output-to-file "{path}" (lambda () (display "in-file")))
                 (display "after"))
               (get-output-string sink)"#
        ),
        "\"after\"",
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("file written"),
        "in-file"
    );
}

/// The case that crashed the VM. Escaping past `with-output-to-file` must
/// restore the port rather than take the process down — and the thunk's output
/// must still reach the file, which is why the port is closed on the way out
/// (Patina, unlike chibi, does not flush open ports at exit).
#[test]
fn test_escaping_out_of_the_thunk_restores_the_port() {
    let dir = TempDir::new().expect("temp dir");
    let path = scratch(&dir, "escaped.txt");
    assert_program_eval_to(
        &format!(
            r#"(import (scheme base) (scheme file) (scheme write))
               (define sink (open-output-string))
               (parameterize ((current-output-port sink))
                 (call-with-current-continuation
                   (lambda (escape)
                     (with-output-to-file "{path}"
                       (lambda () (display "in-file") (escape 'gone)))))
                 (display "after"))
               (get-output-string sink)"#
        ),
        "\"after\"",
    );
    assert_eq!(
        std::fs::read_to_string(&path).expect("file written"),
        "in-file"
    );
}

/// The same for the input side, and it doubles as a check that the escape
/// value is the one the continuation was given.
#[test]
fn test_escaping_out_of_an_input_thunk() {
    let dir = TempDir::new().expect("temp dir");
    let path = scratch(&dir, "escaped-in.txt");
    std::fs::write(&path, "42\n").expect("seed file");
    assert_program_eval_to(
        &format!(
            r#"(import (scheme base) (scheme file) (scheme read))
               (call-with-current-continuation
                 (lambda (escape)
                   (with-input-from-file "{path}" (lambda () (escape (read))))))"#
        ),
        "42",
    );
}

/// An error out of the thunk is the other non-local exit. It must stay
/// catchable, and must not leave the world redirected.
///
/// The handler deliberately writes nothing: where a `guard` handler's own
/// output goes was a separate divergence, converged and kept as the "a guard
/// clause runs after the unwind" row of `tests/scheme/control/cps-features.scm`,
/// and asserting it here would test that behaviour instead
/// of this one.
#[test]
fn test_an_error_in_the_thunk_still_restores_the_port() {
    let dir = TempDir::new().expect("temp dir");
    let path = scratch(&dir, "errored.txt");
    assert_program_eval_to(
        &format!(
            r#"(import (scheme base) (scheme file) (scheme write))
               (define sink (open-output-string))
               (parameterize ((current-output-port sink))
                 (guard (e (#t 'caught))
                   (with-output-to-file "{path}" (lambda () (error "boom"))))
                 (display "after"))
               (get-output-string sink)"#
        ),
        "\"after\"",
    );
}

/// One port, one object (#608): every read of `current-output-port` answers
/// the object installed, at the top level, inside `with-output-to-file` and
/// inside a `parameterize` given a file port. #608's first program, whole,
/// as chibi 0.12.0 and Gauche 0.9.15 answer it; a suite file cannot make the
/// files it opens.
#[test]
fn one_port_one_object() {
    let dir = TempDir::new().expect("temp dir");
    let out = scratch(&dir, "eq-out.txt");
    let redirected = scratch(&dir, "eq2.txt");
    assert_program_eval_to(
        &format!(
            r#"(import (scheme base) (scheme write) (scheme file))
               (define p (open-output-file "{out}"))
               (list (eq? (current-output-port) (current-output-port))
                     (let ((q #f))
                       (with-output-to-file "{redirected}"
                         (lambda () (set! q (eq? (current-output-port) (current-output-port)))))
                       q)
                     (eq? p p)
                     (parameterize ((current-output-port p)) (eq? (current-output-port) p)))"#
        ),
        "(#t #t #t #t)",
    );
}
