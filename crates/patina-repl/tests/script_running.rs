//! End-to-end tests for the `patina` binary's script-running surface
//! (Track L, L0): shebang lines, program-relative library resolution, and
//! the project-local `./.patina/lib/` dependency directory.
//!
//! The search-path tests spawn the real binary because the behaviour lives in
//! the CLI layer (`main.rs`) and in cwd-relative default search paths —
//! neither is reachable through the library API without changing process
//! state test-wide. The shebang test is a one-spawn acceptance smoke: the
//! mechanism is in the shared lexer and unit-tested there.

use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

/// Run the patina binary on `script`, with `cwd` as the working directory.
/// Returns (stdout, stderr, success).
fn run_patina(cwd: &Path, args: &[&str]) -> (String, String, bool) {
    let output = Command::new(env!("CARGO_BIN_EXE_patina"))
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("failed to spawn patina binary");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.success(),
    )
}

/// Both backends: the default VM and --tree-walker.
fn run_both_backends(cwd: &Path, script: &str, expect_stdout: &str) {
    for extra in [&[][..], &["--tree-walker"][..]] {
        let mut args = extra.to_vec();
        args.push(script);
        let (stdout, stderr, ok) = run_patina(cwd, &args);
        assert!(
            ok,
            "patina {:?} failed\nstdout: {}\nstderr: {}",
            args, stdout, stderr
        );
        assert_eq!(
            stdout.trim(),
            expect_stdout,
            "unexpected output for patina {:?}\nstderr: {}",
            args,
            stderr
        );
    }
}

#[test]
fn shebang_script_runs() {
    let temp = TempDir::new().unwrap();
    let script = temp.path().join("hello.scm");
    fs::write(
        &script,
        "#!/usr/bin/env patina\n(import (scheme base))\n(display (+ 40 2))\n(newline)\n",
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
        "(import (scheme base) (mylib))\n(display answer)\n(newline)\n",
    )
    .unwrap();

    // Run from a *different* cwd so resolution must come from the script's
    // own directory, not from ./.
    let other_cwd = TempDir::new().unwrap();
    run_both_backends(other_cwd.path(), script.to_str().unwrap(), "42");
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
        "(import (scheme base) (dep))\n(display (* 6 dep-value))\n(newline)\n",
    )
    .unwrap();

    // cwd is the project directory; the script's own directory is excluded as
    // the resolution route by placing the dependency only under .patina/lib.
    run_both_backends(temp.path(), "main.scm", "42");
}
