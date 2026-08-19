//! Shared helpers for the binary-spawn test suites.

// Each test binary compiles its own copy of this module, so a helper only some
// of them use reads as dead there — the same reason `patina-tests`'s common
// module carries this.
#![allow(dead_code)]

use std::path::Path;
use std::process::Command;

/// Run the patina binary with `cwd` as the working directory.
/// Returns (stdout, stderr, success).
pub fn run_patina(cwd: &Path, args: &[&str]) -> (String, String, bool) {
    run_patina_env(cwd, args, &[])
}

/// Like [`run_patina`], with extra environment variables set on the child.
pub fn run_patina_env(cwd: &Path, args: &[&str], envs: &[(&str, &str)]) -> (String, String, bool) {
    let output = Command::new(env!("CARGO_BIN_EXE_patina"))
        .args(args)
        // A dialect inherited from the developer's shell would decide what the
        // reader accepts, so tests that assert on it could pass or fail for a
        // reason not written down anywhere in them. Callers that want it pass
        // `--allow-r6rs`.
        .env_remove("PATINA_ALLOW_R6RS")
        .envs(envs.iter().copied())
        .current_dir(cwd)
        .output()
        .expect("failed to spawn patina binary");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.success(),
    )
}

/// Run the same argument list on both backends (the default VM and
/// `--tree-walker`), asserting success and exact trimmed stdout.
pub fn run_both_backends(cwd: &Path, args: &[&str], expect_stdout: &str) {
    for extra in [&[][..], &["--tree-walker"][..]] {
        let mut full = extra.to_vec();
        full.extend_from_slice(args);
        let (stdout, stderr, ok) = run_patina(cwd, &full);
        assert!(
            ok,
            "patina {:?} failed\nstdout: {}\nstderr: {}",
            full, stdout, stderr
        );
        assert_eq!(
            stdout.trim(),
            expect_stdout,
            "unexpected output for patina {:?}\nstderr: {}",
            full,
            stderr
        );
    }
}
