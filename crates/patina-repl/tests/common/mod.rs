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
        // Same reasoning, and it bites harder on a *negative* resolution test:
        // both of these are consulted by `LibraryRegistry::with_default_paths`
        // ahead of `./lib` and the workspace walk, so an exported
        // PATINA_LIBRARY_PATH (which this repo's own A/B-build notes recommend
        // setting) could make an "this library must NOT resolve" assertion
        // pass or fail for a reason written down nowhere in the test.
        // `patina_library_path_env_resolves` re-adds it below via `envs`,
        // which runs after these.
        .env_remove("PATINA_LIBRARY_PATH")
        .env_remove("PATINA_HOME")
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

/// The workspace root, for tests that need a repo path. Encodes the
/// crates/patina-repl → root distance once, as `patina-tests`'s own
/// `common::repo_root` does for its crate.
pub fn repo_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .to_path_buf()
}

/// Run the same argument list on both backends (the default VM and
/// `--tree-walker`), asserting failure, and hand each run's stderr to
/// `check`. The negative counterpart of [`run_both_backends`]: `-A`/`-I`
/// handling is implemented once per backend (`LibraryPaths` in
/// `patina-repl/src/main.rs`), so a claim about what does *not* resolve is
/// only half-pinned if it runs on one of them.
pub fn expect_failure_on_both_backends(cwd: &Path, args: &[&str], check: impl Fn(&str)) {
    for extra in [&[][..], &["--tree-walker"][..]] {
        let mut full = extra.to_vec();
        full.extend_from_slice(args);
        let (stdout, stderr, ok) = run_patina(cwd, &full);
        assert!(
            !ok,
            "patina {:?} unexpectedly succeeded\nstdout: {}\nstderr: {}",
            full, stdout, stderr
        );
        check(&stderr);
    }
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
