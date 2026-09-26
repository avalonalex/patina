//! End-to-end tests for the `patina` binary's script-running surface
//! (Track L, L0): shebang lines, program-relative library resolution, and
//! the project-local `./.patina/lib/` dependency directory.
//!
//! The search-path tests spawn the real binary because the behaviour lives in
//! the CLI layer (`main.rs`) and in cwd-relative default search paths —
//! neither is reachable through the library API without changing process
//! state test-wide. The shebang test is a one-spawn acceptance smoke: the
//! mechanism is in the shared lexer and unit-tested there.

mod common;

use common::{BOTH_BACKENDS, expect_failure_on_both_backends, run_both_backends, run_patina};
use std::fs;
use tempfile::TempDir;

/// #417 used to abort before touching a small file: the read limit became
/// the allocation size. Keep the large request in a child process so a
/// regression fails this test rather than aborting the entire Rust suite.
#[test]
#[cfg(target_pointer_width = "64")]
fn oversized_read_bytevector_reads_only_the_available_bytes() {
    let temp = TempDir::new().unwrap();
    let bytes: Vec<u8> = (0..10000).map(|i| (i % 250) as u8).collect();
    fs::write(temp.path().join("tenk.bin"), bytes).unwrap();
    fs::write(temp.path().join("empty.bin"), []).unwrap();
    fs::write(
        temp.path().join("read.scm"),
        r#"(import (scheme base) (scheme write) (scheme file))
(define limit (expt 2 46))
(define (check name)
  (let* ((p (open-binary-input-file name))
         (zero (read-bytevector 0 p))
         (bytes (read-bytevector limit p))
         (end (read-bytevector limit p))
         (zero-at-end (read-bytevector 0 p)))
    (write (list zero
                 (if (eof-object? bytes) 'empty
                     (list (bytevector-length bytes)
                           (bytevector-u8-ref bytes 0)
                           (bytevector-u8-ref bytes 9999)))
                 (eof-object? end) zero-at-end))
    (newline)
    (close-port p)))
(check "tenk.bin")
(check "empty.bin")
"#,
    )
    .unwrap();
    for backend in BOTH_BACKENDS {
        let mut args = backend.to_vec();
        args.extend_from_slice(&["--isolated-libraries", "read.scm"]);
        let (stdout, stderr, status) = common::run_with_deadline_status(temp.path(), &args, None);
        assert!(status.success(), "{backend:?}: {status}\n{stderr}");
        assert_eq!(
            stdout.trim(),
            "(#u8() (10000 0 249) #t #u8())\n(#u8() empty #t #u8())",
            "{backend:?}: {stderr}"
        );
    }
}

#[test]
fn shebang_script_runs() {
    let temp = TempDir::new().unwrap();
    let script = temp.path().join("hello.scm");
    fs::write(
        &script,
        "#!/usr/bin/env patina\n(import (scheme base) (scheme write))\n(display (+ 40 2))\n(newline)\n",
    )
    .unwrap();

    // Shebang stripping is lexer-level and backend-independent; one spawn.
    let (stdout, stderr, ok) = run_patina(temp.path(), &[script.to_str().unwrap()]);
    assert!(ok, "shebang script failed\nstderr: {}", stderr);
    assert_eq!(stdout.trim(), "42");
}

#[test]
fn library_beside_script_resolves() {
    let temp = TempDir::new().unwrap();

    // A checked-out package: mylib.sld sits beside the program that uses it,
    // with no install step and no ./lib/ directory.
    fs::write(
        temp.path().join("mylib.sld"),
        r#"
        (define-library (mylib)
          (import (scheme base))
          (export answer)
          (begin (define answer 42)))
    "#,
    )
    .unwrap();
    let script = temp.path().join("prog.scm");
    fs::write(
        &script,
        "(import (scheme base) (scheme write) (mylib))\n(display answer)\n(newline)\n",
    )
    .unwrap();

    // Run from a *different* cwd so resolution must come from the script's
    // own directory, not from ./.
    let other_cwd = TempDir::new().unwrap();
    run_both_backends(other_cwd.path(), &[script.to_str().unwrap()], "42");
}

#[test]
fn project_local_patina_lib_resolves() {
    let temp = TempDir::new().unwrap();

    // A dependency dropped under ./.patina/lib/, the project-local directory
    // the future fetcher will populate.
    let dep_dir = temp.path().join(".patina").join("lib");
    fs::create_dir_all(&dep_dir).unwrap();
    fs::write(
        dep_dir.join("dep.sld"),
        r#"
        (define-library (dep)
          (import (scheme base))
          (export dep-value)
          (begin (define dep-value 7)))
    "#,
    )
    .unwrap();

    let script = temp.path().join("main.scm");
    fs::write(
        &script,
        "(import (scheme base) (scheme write) (dep))\n(display (* 6 dep-value))\n(newline)\n",
    )
    .unwrap();

    // cwd is the project directory; the script's own directory is excluded as
    // the resolution route by placing the dependency only under .patina/lib.
    run_both_backends(temp.path(), &["main.scm"], "42");
}

/// A library whose load fails is not left marked as loading: loading it again
/// reports the same failure, not a cycle through itself (#436). A `guard`
/// makes the second attempt reachable in one program, as fixing a typo and
/// importing again does at a REPL.
#[test]
fn a_library_that_failed_to_load_fails_the_same_way_again() {
    let temp = TempDir::new().unwrap();
    let lib_dir = temp.path().join("m");
    fs::create_dir(&lib_dir).unwrap();
    fs::write(
        lib_dir.join("broken.sld"),
        "(define-library (m broken)\n  (export x)\n  (import (scheme base))\n  (begin (define x (car 5))))\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("e.scm"),
        r#"(import (scheme base) (scheme write) (scheme eval))
(define (try)
  (guard (e (#t (if (error-object? e) (error-object-message e) e)))
    (environment '(m broken))
    'loaded))
(write (try)) (newline)
(write (try)) (newline)
"#,
    )
    .unwrap();

    for extra in BOTH_BACKENDS {
        let mut args = extra.to_vec();
        args.extend_from_slice(&["-A", ".", "e.scm"]);
        let (stdout, stderr, ok) = run_patina(temp.path(), &args);
        assert!(ok, "patina {args:?} failed\nstderr: {stderr}");
        let lines: Vec<&str> = stdout.lines().collect();
        assert_eq!(lines.len(), 2, "patina {args:?}: {stdout}");
        assert!(lines[0].contains("car"), "patina {args:?}: {stdout}");
        assert_eq!(
            lines[0], lines[1],
            "patina {args:?}: the second attempt differs"
        );
    }
}

/// A library that cannot be found is reported with the directories that
/// were searched, the thing to fix (#436).
#[test]
fn a_missing_library_names_the_directories_searched() {
    let temp = TempDir::new().unwrap();
    let extra = temp.path().join("extra-libs");
    fs::create_dir(&extra).unwrap();
    fs::write(
        temp.path().join("main.scm"),
        "(import (scheme base) (no such lib))\n",
    )
    .unwrap();
    let extra_arg = extra.to_str().unwrap();
    expect_failure_on_both_backends(temp.path(), &["-A", extra_arg, "main.scm"], |stderr| {
        assert!(
            stderr.contains("Library (no such lib) not found; searched ")
                && stderr.contains(extra_arg),
            "stderr: {stderr}"
        );
    });
}

/// A binary that cannot find its own library says so before running
/// anything, with the directories searched and what to set — not an
/// unbound `display`, nor its `import` of `(scheme base)` reported as a cycle
/// through itself (#436).
#[test]
fn a_binary_without_its_library_says_so_up_front() {
    let temp = TempDir::new().unwrap();
    let bin_dir = temp.path().join("bin");
    fs::create_dir(&bin_dir).unwrap();
    let binary = bin_dir.join("patina");
    fs::copy(env!("CARGO_BIN_EXE_patina"), &binary).unwrap();
    fs::write(temp.path().join("q.scm"), "(display 1)\n").unwrap();
    fs::write(
        temp.path().join("p.scm"),
        "(import (scheme base) (scheme write))\n(display 1)\n",
    )
    .unwrap();

    for extra in BOTH_BACKENDS {
        for script in ["q.scm", "p.scm"] {
            let output = std::process::Command::new(&binary)
                .args(extra.iter().copied().chain([script]))
                .env_remove("PATINA_LIBRARY_PATH")
                .env_remove("PATINA_HOME")
                .env_remove("PATINA_ISOLATED_LIBRARIES")
                .env("HOME", temp.path())
                .current_dir(temp.path())
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(
                !output.status.success(),
                "{extra:?} {script}: ran\n{stdout}"
            );
            assert!(stdout.is_empty(), "{extra:?} {script}: ran\n{stdout}");
            assert!(
                stderr.contains("cannot load the base library")
                    && stderr.contains("Library (scheme base) not found; searched ")
                    && stderr.contains("PATINA_LIBRARY_PATH"),
                "{extra:?} {script}: {stderr}"
            );
        }
    }
}

/// A program's `only` import naming an identifier the library does not
/// export is an error on both backends, as chibi and Gauche report it; the
/// tree-walker kept what matched and ran on (#485).
#[test]
fn an_only_import_of_an_identifier_not_exported_is_an_error() {
    let temp = TempDir::new().unwrap();
    fs::write(
        temp.path().join("main.scm"),
        "(import (scheme base) (scheme write))\n(import (only (scheme char) nope))\n(write 'after)\n",
    )
    .unwrap();
    expect_failure_on_both_backends(temp.path(), &["main.scm"], |stderr| {
        assert!(
            stderr.contains("Identifier 'nope' not found in import set"),
            "stderr: {stderr}"
        );
    });
}

/// A `rename` of an identifier the set does not provide is an error on both
/// backends, in a program's imports and in a library's, as Gauche and Chez
/// report it and as Patina's `environment` already did (#489). The VM
/// accepted both and the tree-walker the first.
#[test]
fn a_rename_of_an_identifier_not_provided_is_an_error() {
    let temp = TempDir::new().unwrap();
    let lib_dir = temp.path().join("t");
    fs::create_dir(&lib_dir).unwrap();
    fs::write(
        lib_dir.join("renames.sld"),
        "(define-library (t renames) (import (scheme base) (rename (scheme char) (nope yes)))\n  (export v) (begin (define v 'ok)))\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("program.scm"),
        "(import (scheme base) (rename (scheme char) (nope yes)))\n",
    )
    .unwrap();
    fs::write(
        temp.path().join("library.scm"),
        "(import (scheme base) (t renames))\n",
    )
    .unwrap();
    for script in ["program.scm", "library.scm"] {
        expect_failure_on_both_backends(temp.path(), &["-A", ".", script], |stderr| {
            assert!(
                stderr.contains("Identifier 'nope' not found for rename"),
                "{script}: {stderr}"
            );
        });
    }
}
