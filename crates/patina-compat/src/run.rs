//! Execute the corpus against a patina binary and classify each package.
//!
//! Two execution modes per package:
//! - **test** — the package ships a test program (`(test "run-tests.scm")` in
//!   its `package.scm`); run it. The patina CLI runs files whose name
//!   contains "test" resiliently (exit 0 regardless), so classification
//!   reads the output, not the exit status.
//! - **probe** — no test program; synthesize `(import ...)` of every library
//!   the package provides. Probes run in strict mode, so the exit status is
//!   meaningful.

use crate::corpus::Package;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Classification buckets (PRD Track L §L3). Order here is display order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Pass,
    /// Imports name libraries nothing provides. The histogram of these is
    /// the L1/L2 bundling work queue.
    MissingLibrary(Vec<String>),
    /// A library or program failed to parse — e.g. the bare-`@` identifier
    /// strictness recorded in the Track L PRD §6. Detected before unbound
    /// identifiers because a failed load leaves everything downstream
    /// unbound, which would mask the real cause.
    ParseError(Vec<String>),
    /// Libraries resolve but an identifier does not.
    UnboundIdentifier(Vec<String>),
    /// Loaded and ran, but its own test suite reports failures.
    WrongResult,
    /// Errored at runtime in some other way.
    RuntimeError,
    /// Did not finish within the per-package budget.
    Timeout,
    /// Every missing library is C-backed upstream — unreachable until FFI
    /// exists, so it bounds the achievable score rather than counting as a
    /// defect (PRD §6 risk: "the pass rate has a ceiling").
    OutOfScope(Vec<String>),
}

impl Status {
    pub fn key(&self) -> &'static str {
        match self {
            Status::Pass => "pass",
            Status::MissingLibrary(_) => "missing-library",
            Status::ParseError(_) => "parse-error",
            Status::UnboundIdentifier(_) => "unbound-identifier",
            Status::WrongResult => "wrong-result",
            Status::RuntimeError => "runtime-error",
            Status::Timeout => "timeout",
            Status::OutOfScope(_) => "out-of-scope",
        }
    }
}

/// One package's outcome.
#[derive(Debug)]
pub struct PackageResult {
    pub slug: String,
    pub mode: &'static str, // "test" | "probe"
    pub status: Status,
}

pub struct RunConfig {
    pub patina: PathBuf,
    pub tree_walker: bool,
    pub timeout: Duration,
    pub jobs: usize,
}

/// Libraries that are C-backed in their upstream implementation. A package
/// whose *only* missing imports are these cannot pass until FFI exists.
/// Deliberately conservative — anything not listed counts as a real gap.
const FFI_BOUND: &[&str] = &[
    "chibi ast",
    "chibi disasm",
    "chibi emscripten",
    "chibi filesystem",
    "chibi heap-stats",
    "chibi net",
    "chibi process",
    "chibi stty",
    "chibi system",
    "chibi threads",
    "chibi time",
    "chibi weak",
    "srfi 18",
];

/// Run the selected packages, in parallel, returning results in slug order.
///
/// `universe` is the full corpus regardless of filtering, so cross-package
/// dependencies always resolve.
pub fn run_corpus(
    selected: &[&Package],
    universe: &[Package],
    providers: &BTreeMap<String, usize>,
    config: &RunConfig,
) -> Vec<PackageResult> {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let results = Mutex::new(Vec::with_capacity(selected.len()));

    std::thread::scope(|scope| {
        for _ in 0..config.jobs.max(1) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    if i >= selected.len() {
                        break;
                    }
                    let result = run_package(selected[i], universe, providers, config);
                    let finished = done.fetch_add(1, Ordering::SeqCst) + 1;
                    eprintln!(
                        "[{}/{}] {}: {}",
                        finished,
                        selected.len(),
                        result.slug,
                        result.status.key()
                    );
                    results.lock().unwrap().push(result);
                }
            });
        }
    });

    let mut results = results.into_inner().unwrap();
    results.sort_by(|a, b| a.slug.cmp(&b.slug));
    results
}

fn run_package(
    package: &Package,
    universe: &[Package],
    providers: &BTreeMap<String, usize>,
    config: &RunConfig,
) -> PackageResult {
    let search_roots = dependency_roots(package, universe, providers);

    let (script, mode, _probe_guard) = match &package.test_script {
        Some(path) => (path.clone(), "test", None),
        None => {
            let probe = ProbeFile::create(package);
            (probe.path.clone(), "probe", Some(probe))
        }
    };

    // Run from a scratch directory, not the package root: test suites write
    // log files into their cwd (srfi-64 always does), and the vendored trees
    // must stay byte-identical to upstream. Library includes are unaffected —
    // they resolve against their .sld's own directory.
    let scratch = std::env::temp_dir().join(format!(
        "patina-compat-cwd-{}-{}",
        std::process::id(),
        package.slug
    ));
    let _ = std::fs::create_dir_all(&scratch);

    let mut cmd = Command::new(&config.patina);
    for root in &search_roots {
        cmd.arg("-A").arg(root);
    }
    if config.tree_walker {
        cmd.arg("--tree-walker");
    }
    cmd.arg(&script)
        .current_dir(&scratch)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let outcome = spawn_with_timeout(cmd, config.timeout);
    let _ = std::fs::remove_dir_all(&scratch);
    let status = match outcome {
        Err(e) => {
            eprintln!("warning: {}: spawn failed: {}", package.slug, e);
            Status::RuntimeError
        }
        Ok(out) => classify(&out, mode),
    };

    PackageResult {
        slug: package.slug.clone(),
        mode,
        status,
    }
}

/// The package's own root plus the roots of every vendored package in its
/// transitive dependency closure. Libraries nobody vendors (bundled ones,
/// genuinely missing ones) are simply absent — patina reports the latter.
fn dependency_roots(
    package: &Package,
    all: &[Package],
    providers: &BTreeMap<String, usize>,
) -> Vec<PathBuf> {
    let mut roots = vec![package.root.clone()];
    let mut seen = vec![false; all.len()];
    let mut queue: Vec<&str> = package.depends.iter().map(String::as_str).collect();
    while let Some(lib) = queue.pop() {
        let Some(&idx) = providers.get(lib) else {
            continue;
        };
        if std::mem::replace(&mut seen[idx], true) {
            continue;
        }
        if all[idx].root != package.root {
            roots.push(all[idx].root.clone());
        }
        queue.extend(all[idx].depends.iter().map(String::as_str));
    }
    roots
}

/// A synthesized import probe, deleted on drop. The filename deliberately
/// avoids the substring "test" so the patina CLI runs it in strict mode.
struct ProbeFile {
    path: PathBuf,
}

impl ProbeFile {
    fn create(package: &Package) -> Self {
        let mut source = String::from("(import (scheme base)");
        for lib in &package.provides {
            source.push_str(&format!(" ({})", lib));
        }
        source.push_str(")\n(display \"patina-compat probe ok\")\n(newline)\n");

        let path = std::env::temp_dir().join(format!(
            "patina-compat-{}-{}.scm",
            std::process::id(),
            package.slug
        ));
        std::fs::write(&path, source).expect("write probe file");
        ProbeFile { path }
    }
}

impl Drop for ProbeFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

struct Captured {
    stdout: String,
    stderr: String,
    exit_ok: bool,
    timed_out: bool,
}

fn spawn_with_timeout(mut cmd: Command, timeout: Duration) -> Result<Captured, String> {
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;

    // Drain the pipes on threads so a chatty child can't fill them and block.
    let mut stdout_pipe = child.stdout.take().expect("piped stdout");
    let mut stderr_pipe = child.stderr.take().expect("piped stderr");
    let stdout_thread = std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = Vec::new();
        let _ = stdout_pipe.read_to_end(&mut buf);
        buf
    });
    let stderr_thread = std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = Vec::new();
        let _ = stderr_pipe.read_to_end(&mut buf);
        buf
    });

    let deadline = Instant::now() + timeout;
    let (exit_ok, timed_out) = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => break (status.success(), false),
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break (false, true);
            }
            None => std::thread::sleep(Duration::from_millis(25)),
        }
    };

    let stdout = String::from_utf8_lossy(&stdout_thread.join().unwrap()).into_owned();
    let stderr = String::from_utf8_lossy(&stderr_thread.join().unwrap()).into_owned();
    Ok(Captured {
        stdout,
        stderr,
        exit_ok,
        timed_out,
    })
}

fn classify(out: &Captured, mode: &str) -> Status {
    if out.timed_out {
        return Status::Timeout;
    }

    let combined = format!("{}\n{}", out.stdout, out.stderr);

    let missing = extract_missing_libraries(&combined);
    if !missing.is_empty() {
        return if missing.iter().all(|l| FFI_BOUND.contains(&l.as_str())) {
            Status::OutOfScope(missing)
        } else {
            Status::MissingLibrary(missing)
        };
    }

    let parse_errors = extract_parse_errors(&combined);
    if !parse_errors.is_empty() {
        return Status::ParseError(parse_errors);
    }

    let unbound = extract_unbound_identifiers(&combined);
    if !unbound.is_empty() {
        return Status::UnboundIdentifier(unbound);
    }

    if mode == "test" && test_suite_failed(&out.stdout) {
        return Status::WrongResult;
    }

    // Resilient test runs always exit 0, so "Error:" on stderr is the only
    // runtime-failure signal there; strict probes also surface via exit.
    if out.stderr.contains("Error") || !out.exit_ok {
        return Status::RuntimeError;
    }

    Status::Pass
}

/// Pull `(lib name)` out of every "Library (lib name) not found" message.
fn extract_missing_libraries(output: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = output;
    while let Some(start) = rest.find("Library (") {
        rest = &rest[start + "Library (".len()..];
        if let Some(end) = rest.find(')')
            && rest[end..].starts_with(") not found")
        {
            found.push(rest[..end].to_string());
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Pull the error description out of "Parse error in <path>: <detail>" lines
/// — the detail alone, so the histogram groups by error kind, not by file.
fn extract_parse_errors(output: &str) -> Vec<String> {
    let mut found = Vec::new();
    for line in output.lines() {
        if let Some(pos) = line.find("Parse error in ")
            && let Some(colon) = line[pos..].rfind(": ")
        {
            found.push(line[pos + colon + 2..].trim().to_string());
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Pull the identifier out of "Undefined variable: x" / "unbound variable: x"
/// (the two backends word it differently).
fn extract_unbound_identifiers(output: &str) -> Vec<String> {
    let mut found = Vec::new();
    for marker in ["Undefined variable: ", "unbound variable: "] {
        let mut rest = output;
        while let Some(start) = rest.find(marker) {
            rest = &rest[start + marker.len()..];
            let ident: String = rest.chars().take_while(|c| !c.is_whitespace()).collect();
            if !ident.is_empty() {
                found.push(ident);
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Did a `(chibi test)` run report failures? The summary prints lines like
/// "3 failures (2.1%)." / "1 error." and per-case "FAIL: name" lines.
fn test_suite_failed(stdout: &str) -> bool {
    if stdout.contains("FAIL") {
        return true;
    }
    // "N failure(s)" / "N error(s)" preceded by a digit, tolerant of the
    // ANSI color codes (chibi test) may wrap around the number.
    for keyword in [" failure", " error"] {
        let mut rest = stdout;
        while let Some(pos) = rest.find(keyword) {
            if rest[..pos]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_digit())
            {
                return true;
            }
            rest = &rest[pos + keyword.len()..];
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn captured(stdout: &str, stderr: &str, exit_ok: bool) -> Captured {
        Captured {
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            exit_ok,
            timed_out: false,
        }
    }

    #[test]
    fn classifies_missing_library() {
        let out = captured("", "Error: Library (foo bar) not found", false);
        assert_eq!(
            classify(&out, "probe"),
            Status::MissingLibrary(vec!["foo bar".to_string()])
        );
    }

    #[test]
    fn classifies_ffi_bound_as_out_of_scope() {
        let out = captured("", "Error: Library (chibi ast) not found", false);
        assert_eq!(
            classify(&out, "probe"),
            Status::OutOfScope(vec!["chibi ast".to_string()])
        );
    }

    #[test]
    fn mixed_missing_stays_missing() {
        let out = captured(
            "",
            "Error: Library (chibi ast) not found\nError: Library (foo) not found",
            false,
        );
        assert!(matches!(classify(&out, "probe"), Status::MissingLibrary(_)));
    }

    #[test]
    fn parse_error_beats_downstream_unbound() {
        // A library that fails to parse leaves its importers unbound; the
        // parse error is the cause and must win.
        let out = captured(
            "",
            "Error: Parse error in /x/chibi/match-test.sld: LexError(UnexpectedChar('@'))\n\
             Error: unbound variable: `run-tests`",
            false,
        );
        assert_eq!(
            classify(&out, "test"),
            Status::ParseError(vec!["LexError(UnexpectedChar('@'))".to_string()])
        );
    }

    #[test]
    fn classifies_unbound_identifier() {
        let out = captured("", "Error: Undefined variable: string-index", false);
        assert_eq!(
            classify(&out, "probe"),
            Status::UnboundIdentifier(vec!["string-index".to_string()])
        );
    }

    #[test]
    fn classifies_test_failures_despite_exit_zero() {
        let out = captured("52 out of 53 tests passed.\n1 failure (1.9%).\n", "", true);
        assert_eq!(classify(&out, "test"), Status::WrongResult);
    }

    #[test]
    fn passing_suite_with_error_free_summary_passes() {
        // "0 failures" must not trip the digit check ("0" precedes it), so
        // (chibi test) omits the line entirely on success — and the word
        // "errors" inside e.g. "line-errors-test" alone must not match.
        let out = captured(
            "53 out of 53 (100.0%) tests passed in 0.1 seconds.\n",
            "",
            true,
        );
        assert_eq!(classify(&out, "test"), Status::Pass);
    }

    #[test]
    fn strict_probe_failure_is_runtime_error() {
        let out = captured("", "Error: something else entirely", false);
        assert_eq!(classify(&out, "probe"), Status::RuntimeError);
    }

    #[test]
    fn timeout_wins() {
        let out = Captured {
            stdout: String::new(),
            stderr: "Error: Library (foo) not found".to_string(),
            exit_ok: false,
            timed_out: true,
        };
        assert_eq!(classify(&out, "probe"), Status::Timeout);
    }
}
