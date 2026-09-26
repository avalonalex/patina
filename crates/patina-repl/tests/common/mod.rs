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
    let output = patina_command(cwd, args, envs)
        .output()
        .expect("failed to spawn patina binary");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.success(),
    )
}

/// The binary, its working directory, and a scrubbed environment — the part
/// every spawn helper needs, in one place so the reasoning below has one
/// home. Callers add their own stdio and waiting.
pub fn patina_command(cwd: &Path, args: &[&str], envs: &[(&str, &str)]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_patina"));
    command
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
        .env_remove("PATINA_ISOLATED_LIBRARIES")
        // A session saves its history under HOME, and a test's input has no
        // business in the developer's own `~/.patina_history`.
        .env("HOME", cwd)
        .envs(envs.iter().copied())
        .current_dir(cwd);
    command
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
    for extra in BOTH_BACKENDS {
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
    for extra in BOTH_BACKENDS {
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

/// The argument prefixes that select each backend, the VM first. A claim about
/// the binary is made on both.
pub const BOTH_BACKENDS: [&[&str]; 2] = [&[], &["--tree-walker"]];

/// Run the binary and collect its output, killing it if it is still running
/// after ten seconds: a runner looping on a parse error would otherwise hang
/// the suite, and so would one that never finishes reading standard input.
///
/// `input` is written to the child's standard input and the pipe then closed,
/// as a shell redirect does. `None` closes it immediately, which is what the
/// binary sees from `< /dev/null`.
pub fn run_with_deadline(cwd: &Path, args: &[&str], input: Option<&str>) -> (String, String, bool) {
    let (stdout, stderr, status) = run_with_deadline_status(cwd, args, input);
    (stdout, stderr, status.success())
}

/// [`run_with_deadline`], with the exit status itself.
pub fn run_with_deadline_status(
    cwd: &Path,
    args: &[&str],
    input: Option<&str>,
) -> (String, String, std::process::ExitStatus) {
    run_with_deadline_bytes(cwd, args, input.unwrap_or("").as_bytes())
}

/// [`run_with_deadline_status`] with raw input, for invalid or incomplete UTF-8.
pub fn run_with_deadline_bytes(
    cwd: &Path,
    args: &[&str],
    input: &[u8],
) -> (String, String, std::process::ExitStatus) {
    use std::io::Write;

    let mut patina = spawn_patina(cwd, args);
    let mut sink = patina.stdin.take().expect("stdin pipe");
    let input = input.to_owned();
    // On its own thread: a child that exits without reading leaves this
    // write blocked or broken, and neither should fail the run — the
    // child's own stderr is the better report, so a broken pipe is dropped.
    let writer = std::thread::spawn(move || {
        let _ = sink.write_all(&input);
    });
    let result = patina.finish_with_status();
    let _ = writer.join();
    result
}

/// A running patina whose standard input is a pipe the test writes to in
/// stages, for claims about when a program acts on what it has been given.
/// Both output pipes are drained on their own threads while it runs, so a
/// flood of output fails on what it printed rather than filling a pipe buffer
/// and stalling until the deadline.
pub struct RunningPatina {
    child: std::process::Child,
    stdin: Option<std::process::ChildStdin>,
    stdout: std::thread::JoinHandle<Vec<u8>>,
    stderr: std::thread::JoinHandle<Vec<u8>>,
    args: Vec<String>,
}

pub fn spawn_patina(cwd: &Path, args: &[&str]) -> RunningPatina {
    spawn_patina_with_stdin(cwd, args, std::process::Stdio::piped())
}

/// Like [`spawn_patina`], with a redirected file or another stdin source.
pub fn spawn_patina_with_stdin(
    cwd: &Path,
    args: &[&str],
    stdin: std::process::Stdio,
) -> RunningPatina {
    use std::io::Read;
    use std::process::Stdio;

    let mut child = patina_command(cwd, args, &[])
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn patina binary");
    let drain = |mut pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            buf
        })
    };
    RunningPatina {
        stdin: child.stdin.take(),
        stdout: drain(Box::new(child.stdout.take().expect("stdout pipe"))),
        stderr: drain(Box::new(child.stderr.take().expect("stderr pipe"))),
        child,
        args: args.iter().map(|arg| arg.to_string()).collect(),
    }
}

impl RunningPatina {
    /// Write `text` to standard input and flush it, leaving the pipe open. A
    /// child that has already exited is not an error here: its stderr, which
    /// [`RunningPatina::finish`] returns, is the better report.
    pub fn write(&mut self, text: &str) {
        use std::io::Write;

        if let Some(stdin) = &mut self.stdin {
            let _ = stdin.write_all(text.as_bytes());
            let _ = stdin.flush();
        }
    }

    /// Close standard input, wait for the child to exit, and return its
    /// (stdout, stderr, success) — failing the test, after killing the child,
    /// if it is still running ten seconds later.
    pub fn finish(self) -> (String, String, bool) {
        let (stdout, stderr, status) = self.finish_with_status();
        (stdout, stderr, status.success())
    }

    /// [`RunningPatina::finish`], with the exit status itself.
    pub fn finish_with_status(mut self) -> (String, String, std::process::ExitStatus) {
        use std::time::{Duration, Instant};

        drop(self.stdin.take());
        let started = Instant::now();
        let status = loop {
            if let Some(status) = self.child.try_wait().expect("wait on patina") {
                break Some(status);
            }
            if started.elapsed() > Duration::from_secs(10) {
                self.child.kill().ok();
                self.child.wait().ok();
                break None;
            }
            std::thread::sleep(Duration::from_millis(2));
        };
        let stdout =
            String::from_utf8_lossy(&self.stdout.join().expect("stdout reader")).into_owned();
        let stderr =
            String::from_utf8_lossy(&self.stderr.join().expect("stderr reader")).into_owned();
        let status = status.unwrap_or_else(|| {
            panic!(
                "patina {:?} was still running after 10 s; stderr began:\n{}",
                self.args,
                stderr.lines().take(3).collect::<Vec<_>>().join("\n")
            )
        });
        (stdout, stderr, status)
    }
}
