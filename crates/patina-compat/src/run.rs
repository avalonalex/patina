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

use crate::corpus::{self, Package};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
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
    /// A library loaded and parsed but failed while being installed —
    /// export resolution, library-body evaluation. The interpreter wraps
    /// these in the same "Parse error in ..." prefix as real parse errors
    /// (`LibraryError::ParseError` is overloaded — recorded as debt in the
    /// Track L PRD), so the split happens here to keep the parser work
    /// queue honest.
    LoadError(Vec<String>),
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
    /// Every bucket key, in display order — the one place ordering lives.
    pub const KEYS: [&'static str; 9] = [
        "pass",
        "missing-library",
        "parse-error",
        "load-error",
        "unbound-identifier",
        "wrong-result",
        "runtime-error",
        "timeout",
        "out-of-scope",
    ];

    pub fn key(&self) -> &'static str {
        match self {
            Status::Pass => "pass",
            Status::MissingLibrary(_) => "missing-library",
            Status::ParseError(_) => "parse-error",
            Status::LoadError(_) => "load-error",
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
/// Derived by hand from the upstream chibi-scheme tree's `.c`-backed module
/// inventory (see PRD Track L §L2's out-of-scope list); deliberately
/// conservative — anything not listed counts as a real gap.
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
                    let mut guard = results.lock().unwrap();
                    guard.push(result);
                    let just_pushed = guard.last().unwrap();
                    eprintln!(
                        "[{}/{}] {}: {}",
                        guard.len(),
                        selected.len(),
                        just_pushed.slug,
                        just_pushed.status.key()
                    );
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
    let mut search_roots = dependency_roots(package, universe, providers);

    // Run from a scratch directory, not the package root: test suites write
    // log files into their cwd (srfi-64 always does), and the vendored trees
    // must stay byte-identical to upstream. Library includes are unaffected —
    // they resolve against their .sld's own directory.
    //
    // The scratch path must not contain the substring "test" (some slugs
    // do), or the CLI's test-file heuristic would run a probe resiliently
    // and its exit status would stop meaning anything.
    let scratch = std::env::temp_dir().join(format!(
        "patina-compat-{}-{}",
        std::process::id(),
        package.slug.replace("test", "t-st")
    ));
    let _ = std::fs::create_dir_all(&scratch);

    // A package whose `.sld` does not sit where its name says needs the
    // layout an installer would have given it before anything can import it.
    if let Some(stage) = stage_off_path_libraries(&scratch, package) {
        search_roots.insert(0, stage);
    }

    let (script, mode) = match &package.test_script {
        Some(path) => (path.clone(), "test"),
        None => (write_probe(&scratch, package), "probe"),
    };

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
    // `(test-depends ...)` belongs to the package's own test program, so it
    // seeds the closure only when that program is what we are about to run,
    // and is never followed out of a *dependency* — nobody importing this
    // package needs its test framework.
    if package.test_script.is_some() {
        queue.extend(package.test_depends.iter().map(String::as_str));
    }
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

/// Give the package's off-path libraries the layout their names imply, in a
/// throwaway directory, and return the search root that exposes it (`None`
/// when the package needs no staging, which is the norm).
///
/// The `.sld`'s whole directory is mirrored, not just the file, so its
/// relative `include`s still resolve from the staged copy. Snow's installer
/// instead keeps includes at their package-relative paths, which leaves them
/// unreachable from the `.sld`'s new home; mirroring is the same rename with
/// that defect fixed.
fn stage_off_path_libraries(scratch: &Path, package: &Package) -> Option<PathBuf> {
    let stage = scratch.join("stage");
    let mut staged_any = false;
    for lib in &package.off_path_libraries {
        let dest = stage.join(corpus::name_relative_path(&lib.name));
        let (Some(dest_dir), Some(source_dir)) = (dest.parent(), lib.source.parent()) else {
            continue;
        };
        match copy_tree(source_dir, dest_dir).and_then(|()| std::fs::copy(&lib.source, &dest)) {
            Ok(_) => staged_any = true,
            Err(e) => eprintln!(
                "warning: {}: could not stage ({}): {}",
                package.slug, lib.name, e
            ),
        }
    }
    staged_any.then_some(stage)
}

/// Copy `source` into `dest` recursively, creating `dest`. Corpus packages
/// are a few hundred KB at most, and copying keeps the runner free of
/// platform-specific symlink handling.
fn copy_tree(source: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let to = dest.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), &to)?;
        }
    }
    Ok(())
}

/// Write a synthesized import probe into the scratch directory. The fixed
/// basename keeps "test" out of the argument path structurally, so probes
/// always run in strict mode.
fn write_probe(scratch: &std::path::Path, package: &Package) -> PathBuf {
    let mut source = String::from("(import (scheme base)");
    for lib in &package.provides {
        source.push_str(&format!(" ({})", lib));
    }
    source.push_str(")\n(display \"patina-compat probe ok\")\n(newline)\n");

    let path = scratch.join("probe.scm");
    std::fs::write(&path, source).expect("write probe file");
    path
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

    let parts = [out.stdout.as_str(), out.stderr.as_str()];

    let missing = extract_missing_libraries(parts);
    if !missing.is_empty() {
        return if missing.iter().all(|l| FFI_BOUND.contains(&l.as_str())) {
            Status::OutOfScope(missing)
        } else {
            Status::MissingLibrary(missing)
        };
    }

    // The interpreter wraps genuine parse errors and load-stage failures
    // (export resolution, library-body evaluation) in the same "Parse error
    // in ..." message; split them so the parser queue stays honest. A real
    // parse error wins when both appear — it is the deeper cause.
    let (load_errors, parse_errors): (Vec<_>, Vec<_>) =
        extract_parse_errors(parts).into_iter().partition(|d| {
            d.starts_with("Exported identifier") || d.contains("evaluating library body")
        });
    if !parse_errors.is_empty() {
        return Status::ParseError(parse_errors);
    }
    if !load_errors.is_empty() {
        return Status::LoadError(load_errors);
    }

    let unbound = extract_unbound_identifiers(parts);
    if !unbound.is_empty() {
        return Status::UnboundIdentifier(unbound);
    }

    // A bundled library can be honest about its own limits. `(chibi filesystem)`
    // implements its portable half and stubs the POSIX half with this marker
    // (lib/chibi/filesystem.sld), so a package that reaches one of those stubs
    // is FFI-bound in exactly the sense FFI_BOUND means — it just proved it by
    // running instead of by failing to import. Without this it would be filed
    // as a runtime-error, i.e. as our defect.
    if let Some(stubs) = extract_ffi_stubs(parts) {
        return Status::OutOfScope(stubs);
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

/// Names of the FFI-stub procedures a run actually reached, if any.
///
/// The marker is raised by `define-unimplemented` in a bundled library, so it
/// is our own string and not pattern-matching on a third party's prose.
fn extract_ffi_stubs(parts: [&str; 2]) -> Option<Vec<String>> {
    const MARKER: &str = "requires FFI, unavailable in Patina:";
    let mut found: Vec<String> = parts
        .iter()
        .flat_map(|s| s.lines())
        .filter_map(|line| {
            let (_, rest) = line.split_once(MARKER)?;
            Some(rest.trim().trim_matches(['"', '\'']).to_string())
        })
        .filter(|s| !s.is_empty())
        .collect();
    found.sort();
    found.dedup();
    (!found.is_empty()).then_some(found)
}

/// Pull `(lib name)` out of every "Library (lib name) not found" message.
fn extract_missing_libraries(parts: [&str; 2]) -> Vec<String> {
    let mut found: Vec<String> = parts
        .iter()
        .flat_map(|s| s.lines())
        .filter_map(|line| {
            let (_, rest) = line.split_once("Library (")?;
            let (lib, _) = rest.split_once(") not found")?;
            Some(lib.to_string())
        })
        .collect();
    found.sort();
    found.dedup();
    found
}

/// Pull the error description out of a parse failure — the detail alone, so
/// the histogram groups by error kind, not by file.
///
/// Two wordings, because failing to parse an *included* file is reported by
/// `include`/`load` rather than by the library loader: "Parse error in
/// <path>: <detail>" and "include: parse error in '<path>': <detail>". The
/// second is just as much a parse error, and without it a package whose
/// included source cannot be lexed lands in `runtime-error`, i.e. filed as a
/// failure of something that never ran.
fn extract_parse_errors(parts: [&str; 2]) -> Vec<String> {
    fn from_loader(line: &str) -> Option<String> {
        let (_, rest) = line.split_once("Parse error in ")?;
        let (_, detail) = rest.rsplit_once(": ")?;
        Some(detail.trim().to_string())
    }
    fn from_include(line: &str) -> Option<String> {
        let (_, rest) = line.split_once("parse error in '")?;
        let (_, detail) = rest.split_once("': ")?;
        Some(detail.trim().to_string())
    }
    let mut found: Vec<String> = parts
        .iter()
        .flat_map(|s| s.lines())
        .filter_map(|line| from_loader(line).or_else(|| from_include(line)))
        .collect();
    found.sort();
    found.dedup();
    found
}

/// Pull the identifier out of "Undefined variable: x" / "unbound variable: x"
/// (the two backends word it differently).
fn extract_unbound_identifiers(parts: [&str; 2]) -> Vec<String> {
    let mut found: Vec<String> = ["Undefined variable: ", "unbound variable: "]
        .iter()
        .flat_map(|marker| {
            parts.iter().flat_map(move |part| {
                part.split(marker)
                    .skip(1)
                    .filter_map(|rest| rest.split_whitespace().next())
                    .map(str::to_string)
            })
        })
        .collect();
    found.sort();
    found.dedup();
    found
}

/// Did a `(chibi test)` run report failures? The summary prints lines like
/// "3 failures (2.1%)." / "1 error." and per-case "FAIL: name" lines.
fn test_suite_failed(stdout: &str) -> bool {
    stdout.contains("FAIL")
        || [" failure", " error"].iter().any(|keyword| {
            stdout
                .match_indices(keyword)
                .any(|(i, _)| stdout[..i].ends_with(|c: char| c.is_ascii_digit()))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corpus::OffPathLibrary;

    fn package(slug: &str, root: &Path) -> Package {
        Package {
            slug: slug.to_string(),
            root: root.to_path_buf(),
            provides: Vec::new(),
            depends: Vec::new(),
            test_depends: Vec::new(),
            test_script: None,
            off_path_libraries: Vec::new(),
        }
    }

    /// Staging must carry the `.sld`'s neighbours along, or its relative
    /// `include`s break the moment it is read from its new home.
    #[test]
    fn staging_takes_the_includes_with_it() {
        let temp = tempfile::TempDir::new().unwrap();
        let source = temp.path().join("pkg");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(
            source.join("irregex.sld"),
            "(define-library (chibi irregex))",
        )
        .unwrap();
        std::fs::write(source.join("irregex.scm"), "(define x 1)").unwrap();

        let scratch = temp.path().join("scratch");
        let mut pkg = package("chibi-irregex", &source);
        pkg.off_path_libraries.push(OffPathLibrary {
            name: "chibi irregex".to_string(),
            source: source.join("irregex.sld"),
        });

        let stage = stage_off_path_libraries(&scratch, &pkg).expect("staged");
        assert!(stage.join("chibi/irregex.sld").is_file());
        assert!(stage.join("chibi/irregex.scm").is_file());
    }

    #[test]
    fn a_conventional_package_stages_nothing() {
        let temp = tempfile::TempDir::new().unwrap();
        let pkg = package("srfi-1", temp.path());
        assert!(stage_off_path_libraries(&temp.path().join("scratch"), &pkg).is_none());
    }

    /// A test-only dependency has to reach the search path, or the package is
    /// filed as `missing-library` naming something the corpus provides.
    #[test]
    fn test_depends_reach_the_search_path_when_the_test_runs() {
        let temp = tempfile::TempDir::new().unwrap();
        let mut subject = package("lassik-string-inflection", &temp.path().join("subject"));
        subject.test_depends.push("srfi 64".to_string());
        subject.test_script = Some(temp.path().join("subject/t.scm"));
        let framework_root = temp.path().join("srfi-64");
        let mut framework = package("srfi-64", &framework_root);
        framework.provides.push("srfi 64".to_string());

        let universe = vec![framework];
        let providers = corpus::providers(&universe);
        let roots = dependency_roots(&subject, &universe, &providers);
        assert!(roots.contains(&framework_root));

        // With no test program to run, they are irrelevant and stay off.
        subject.test_script = None;
        let roots = dependency_roots(&subject, &universe, &providers);
        assert!(!roots.contains(&framework_root));
    }

    /// A dependency's own test framework is not part of what importing it
    /// requires, so the closure must not pull it in transitively.
    #[test]
    fn test_depends_are_not_followed_out_of_a_dependency() {
        let temp = tempfile::TempDir::new().unwrap();
        let mut subject = package("app", &temp.path().join("app"));
        subject.depends.push("some lib".to_string());
        subject.test_script = Some(temp.path().join("app/t.scm"));

        let mut library = package("some-lib", &temp.path().join("some-lib"));
        library.provides.push("some lib".to_string());
        library.test_depends.push("srfi 64".to_string());
        let framework_root = temp.path().join("srfi-64");
        let mut framework = package("srfi-64", &framework_root);
        framework.provides.push("srfi 64".to_string());

        let universe = vec![library, framework];
        let providers = corpus::providers(&universe);
        let roots = dependency_roots(&subject, &universe, &providers);
        assert!(roots.contains(&temp.path().join("some-lib")));
        assert!(!roots.contains(&framework_root));
    }

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

    /// Reaching a bundled library's FFI stub is out-of-scope, not a defect —
    /// the package got far enough to run, which a missing-import check cannot
    /// see.
    #[test]
    fn classifies_reached_ffi_stub_as_out_of_scope() {
        let out = captured(
            "",
            "Error: requires FFI, unavailable in Patina: duplicate-file-descriptor",
            false,
        );
        assert_eq!(
            classify(&out, "probe"),
            Status::OutOfScope(vec!["duplicate-file-descriptor".to_string()])
        );
    }

    /// The stub check must not outrank a real failure that happened first.
    #[test]
    fn unbound_identifier_beats_a_later_ffi_stub() {
        let out = captured(
            "",
            "Error: Undefined variable: frobnicate\n\
             Error: requires FFI, unavailable in Patina: open",
            false,
        );
        assert_eq!(
            classify(&out, "probe"),
            Status::UnboundIdentifier(vec!["frobnicate".to_string()])
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
    fn export_failure_is_load_error_not_parse_error() {
        // The interpreter wraps export-resolution failures in the same
        // "Parse error in ..." prefix; they must not inflate the parser
        // work queue.
        let out = captured(
            "",
            "Error: Parse error in /x/lib.sld: Exported identifier 'begin' not defined",
            false,
        );
        assert_eq!(
            classify(&out, "probe"),
            Status::LoadError(vec!["Exported identifier 'begin' not defined".to_string()])
        );
    }

    #[test]
    fn real_parse_error_wins_over_load_error() {
        let out = captured(
            "",
            "Error: Parse error in /x/a.sld: LexError(UnexpectedChar('@'))\n\
             Error: Parse error in /x/b.sld: Exported identifier 'f' not defined",
            false,
        );
        assert!(matches!(classify(&out, "probe"), Status::ParseError(_)));
    }

    /// `include` words a parse failure its own way and quotes the path. It is
    /// still a parse error, and belongs in the queue that drives parser work
    /// rather than in `runtime-error`.
    #[test]
    fn an_included_file_that_will_not_lex_is_a_parse_error() {
        let out = captured(
            "",
            "Error: desugar error: Invalid syntax: include: parse error in \
             '/x/srfi-197.scm': Lexer error: Unexpected character: \u{2026}",
            false,
        );
        assert_eq!(
            classify(&out, "test"),
            Status::ParseError(vec![
                "Lexer error: Unexpected character: \u{2026}".to_string()
            ])
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
        // (chibi test) omits the failures/errors lines entirely on success,
        // and the digit guard keeps prose like "line-errors-test" from
        // matching.
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
