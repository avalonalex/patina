//! End-to-end tests for the `patina` binary's library-path CLI surface
//! (Track L, L0.5): `-A`, `-I`, `-p`, `--version`, and PATINA_LIBRARY_PATH.

mod common;

use common::{run_both_backends, run_patina, run_patina_env};
use std::fs;
use std::path::Path;
use tempfile::TempDir;

/// Write a `(dup)` library exporting `v` bound to the given symbol.
fn write_dup_lib(dir: &Path, value: &str) {
    fs::write(
        dir.join("dup.sld"),
        format!(
            r#"
            (define-library (dup)
              (import (scheme base))
              (export v)
              (begin (define v '{})))
        "#,
            value
        ),
    )
    .unwrap();
}

/// Write a script that imports `(dup)` and displays `v`.
fn write_dup_script(dir: &Path) -> String {
    let script = dir.join("prog.scm");
    fs::write(
        &script,
        "(import (scheme base) (dup))\n(display v)\n(newline)\n",
    )
    .unwrap();
    script.to_str().unwrap().to_string()
}

#[test]
fn version_flag_prints_version() {
    let temp = TempDir::new().unwrap();
    let (stdout, stderr, ok) = run_patina(temp.path(), &["--version"]);
    assert!(ok, "stderr: {}", stderr);
    assert_eq!(
        stdout.trim(),
        format!("patina {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn eval_print_expression() {
    let temp = TempDir::new().unwrap();
    run_both_backends(temp.path(), &["-p", "(+ 1 2)"], "3");
}

#[test]
fn eval_print_features_lists_r7rs() {
    // The L3 harness's capability probe: `patina -p "(features)"`.
    let temp = TempDir::new().unwrap();
    let (stdout, stderr, ok) = run_patina(temp.path(), &["-p", "(features)"]);
    assert!(ok, "stderr: {}", stderr);
    assert!(stdout.contains("r7rs"), "features output: {}", stdout);
    assert!(stdout.contains("patina"), "features output: {}", stdout);
}

#[test]
fn eval_print_multiple_in_order() {
    let temp = TempDir::new().unwrap();
    let (stdout, stderr, ok) = run_patina(temp.path(), &["-p", "1", "-p", "2"]);
    assert!(ok, "stderr: {}", stderr);
    assert_eq!(stdout.trim(), "1\n2");
}

#[test]
fn append_path_resolves_library() {
    // The library, the script, and the cwd are three different directories,
    // so only -A can be the resolution route.
    let lib_dir = TempDir::new().unwrap();
    write_dup_lib(lib_dir.path(), "from-append");
    let script_dir = TempDir::new().unwrap();
    let script = write_dup_script(script_dir.path());
    let cwd = TempDir::new().unwrap();

    run_both_backends(
        cwd.path(),
        &["-A", lib_dir.path().to_str().unwrap(), &script],
        "from-append",
    );
}

#[test]
fn prepend_beats_append() {
    let first = TempDir::new().unwrap();
    write_dup_lib(first.path(), "first");
    let second = TempDir::new().unwrap();
    write_dup_lib(second.path(), "second");
    let script_dir = TempDir::new().unwrap();
    let script = write_dup_script(script_dir.path());
    let cwd = TempDir::new().unwrap();

    run_both_backends(
        cwd.path(),
        &[
            "-A",
            second.path().to_str().unwrap(),
            "-I",
            first.path().to_str().unwrap(),
            &script,
        ],
        "first",
    );
}

#[test]
fn prepend_beats_cwd_lib() {
    // ./lib is the first default; -I must still come before it.
    let cwd = TempDir::new().unwrap();
    let cwd_lib = cwd.path().join("lib");
    fs::create_dir(&cwd_lib).unwrap();
    write_dup_lib(&cwd_lib, "from-cwd");
    let prepended = TempDir::new().unwrap();
    write_dup_lib(prepended.path(), "from-prepend");
    let script_dir = TempDir::new().unwrap();
    let script = write_dup_script(script_dir.path());

    run_both_backends(
        cwd.path(),
        &["-I", prepended.path().to_str().unwrap(), &script],
        "from-prepend",
    );
}

#[test]
fn patina_library_path_env_resolves() {
    // Resolution must come through the environment variable: the library,
    // script, and cwd are all separate directories.
    let lib_dir = TempDir::new().unwrap();
    write_dup_lib(lib_dir.path(), "from-env");
    let script_dir = TempDir::new().unwrap();
    let script = write_dup_script(script_dir.path());
    let cwd = TempDir::new().unwrap();

    let (stdout, stderr, ok) = run_patina_env(
        cwd.path(),
        &[&script],
        &[("PATINA_LIBRARY_PATH", lib_dir.path().to_str().unwrap())],
    );
    assert!(ok, "stderr: {}", stderr);
    assert_eq!(stdout.trim(), "from-env");
}

#[test]
fn missing_flag_value_errors() {
    let temp = TempDir::new().unwrap();
    let (_, stderr, ok) = run_patina(temp.path(), &["-A"]);
    assert!(!ok);
    assert!(stderr.contains("requires"), "stderr: {}", stderr);
}

#[test]
fn eval_print_rejects_script_file() {
    let temp = TempDir::new().unwrap();
    let script = temp.path().join("prog.scm");
    fs::write(&script, "(display 1)\n").unwrap();

    let (_, stderr, ok) = run_patina(temp.path(), &["-p", "1", script.to_str().unwrap()]);
    assert!(!ok);
    assert!(stderr.contains("cannot be combined"), "stderr: {}", stderr);
}
