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
    // chibi's own FFI interface library. A package importing it is asking for
    // C bindings by name, which is the deferred work item in Track L §3, not a
    // library Patina could bundle.
    "foreign c",
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

    let (script, mode) = match &package.test_script {
        Some(path) => (path.clone(), "test"),
        None => (write_probe(&scratch, package), "probe"),
    };
    let search_roots = search_roots(package, universe, providers, &scratch, mode == "test");

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

/// Every `-A` root the run needs: the package itself plus every vendored
/// package in its transitive dependency closure, each contributing a staged
/// root ahead of its source tree when it has libraries the source tree cannot
/// resolve by name. Libraries nobody vendors (bundled ones, genuinely missing
/// ones) are simply absent — patina reports the latter.
///
/// Staging follows the closure rather than being applied to the subject alone:
/// an off-path library is just as unreachable when it is a *dependency*, and
/// that would misfile the importer as `missing-library` naming a library the
/// corpus provides — the failure this whole mechanism exists to prevent.
fn search_roots(
    package: &Package,
    all: &[Package],
    providers: &BTreeMap<String, usize>,
    scratch: &Path,
    is_test_run: bool,
) -> Vec<PathBuf> {
    let mut closure = vec![package];
    let mut seen = vec![false; all.len()];
    let mut queue: Vec<&str> = package.depends.iter().map(String::as_str).collect();
    // `(test-depends ...)` belongs to the package's own test program, so it
    // seeds the closure only when that program is what we are about to run,
    // and is never followed out of a *dependency* — nobody importing this
    // package needs its test framework.
    if is_test_run {
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
            closure.push(&all[idx]);
        }
        queue.extend(all[idx].depends.iter().map(String::as_str));
    }

    let mut roots = Vec::with_capacity(closure.len());
    for pkg in closure {
        roots.extend(stage_off_path_libraries(scratch, pkg));
        roots.push(pkg.root.clone());
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
    // Per package, since the closure can stage several and two of them could
    // otherwise want the same directory.
    let stage = scratch.join("stage").join(&package.slug);
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

    // The library's implementation is a compiled shared object, and Patina
    // refused that clause deliberately. First of all the buckets, for the
    // reason the parse-error/unbound ordering below already encodes: a
    // library that never loads leaves its importer unbound and its test
    // program's entry point undefined, so every later marker here is a
    // symptom of this one. `chibi-mecab` arrived as
    // `unbound-identifier: run-chibi-mecab-test-tests` until this moved up.
    //
    // It also outranks a missing library, which the stub check below
    // deliberately does not. The difference is what each proves: a missing
    // library says a package needs something we could bundle, while this says
    // the package needs a `.so` and would still need it afterwards. Matched
    // on a constant this crate links against rather than on wording — see
    // `patina_runtime::NATIVE_EXTENSION_MARKER`.
    if let Some(extensions) = extract_native_extensions(parts) {
        return Status::OutOfScope(extensions);
    }

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

    // A suite that reported failures is a `wrong-result` even if it also
    // reached an FFI stub. Checked before the stub classification, not after:
    // both orders lose information, and this one errs by blaming us for
    // something FFI may have caused, while the other excuses a genuine failure
    // as out-of-scope. Overstating our own defects creates work; understating
    // them creates false confidence, which is the thing this harness exists
    // not to produce (audit E3).
    if mode == "test" && test_suite_failed(&out.stdout) {
        return Status::WrongResult;
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

    // Resilient test runs always exit 0, so "Error:" on stderr is the only
    // runtime-failure signal there; strict probes also surface via exit.
    if out.stderr.contains("Error") || !out.exit_ok {
        return Status::RuntimeError;
    }

    Status::Pass
}

/// Every distinct, non-empty name `read_name` recovers from the text after
/// `marker`, over both captured streams.
///
/// The scan-sort-dedup spine is the same for every marker the classifier
/// reads; naming it leaves each extractor as the one line that says how to
/// read its own tail.
fn extract_after_marker(
    parts: [&str; 2],
    marker: &str,
    read_name: impl Fn(&str) -> Option<String>,
) -> Vec<String> {
    let mut found: Vec<String> = parts
        .iter()
        .flat_map(|s| s.lines())
        .filter_map(|line| read_name(line.split_once(marker)?.1))
        .filter(|s| !s.is_empty())
        .collect();
    found.sort();
    found.dedup();
    found
}

/// Names of the shared objects an `include-shared` declaration asked for.
///
/// Patina refuses that declaration deliberately (`LibraryError::
/// NativeExtensionRequired`), so this reads a marker the interpreter emits on
/// purpose rather than wording that could be reworded — the shared constant is
/// the contract, and it is a compile-time one.
fn extract_native_extensions(parts: [&str; 2]) -> Option<Vec<String>> {
    let found = extract_after_marker(parts, patina_runtime::NATIVE_EXTENSION_MARKER, |rest| {
        let (name, _) = rest.trim().strip_prefix('"')?.split_once('"')?;
        Some(name.to_string())
    });
    (!found.is_empty()).then_some(found)
}

/// Names of the FFI-stub procedures a run actually reached, if any.
///
/// The marker is raised by `define-unimplemented` in a bundled library, so it
/// is our own string and not pattern-matching on a third party's prose.
fn extract_ffi_stubs(parts: [&str; 2]) -> Option<Vec<String>> {
    let found = extract_after_marker(parts, "requires FFI, unavailable in Patina:", |rest| {
        Some(rest.trim().trim_matches(['"', '\'']).to_string())
    });
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
        // First ": " ends the path; the detail may itself contain colons —
        // see `a_multi_colon_detail_is_not_truncated`. The constant layer
        // tags are then stripped so the histogram keys stay short and
        // stable against rewording: the section header already says "Parse
        // errors", so they carry no information there.
        let (_, detail) = rest.split_once(": ")?;
        let detail = detail
            .trim_start_matches("desugar error: ")
            .trim_start_matches("runtime error: ");
        let detail = detail.strip_prefix("Invalid syntax: ").unwrap_or(detail);
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

/// Did the suite report failures?
///
/// Four failure shapes reach this — three frameworks plus one bespoke test
/// script — and each says it its own way, so this knows all four. It is the
/// only thing standing between a failing suite and a `pass` row, which makes
/// a shape it does not know a *false green* rather than a missing detail.
///
/// `(chibi test)` prints per-case `FAIL: name` lines and a summary
/// `3 failures (2.1%).` / `1 error.` — the count *before* the word.
///
/// SRFI 64's reference runner (`compat/vendor/srfi-64`) prints
/// `# of failures             3`: the count *after* the phrase, lowercase,
/// with the per-case detail going to a `.log` file rather than to stdout, and
/// exit code 0 either way. Nothing in the shape above matched it, so a
/// deliberately failing one-test suite classified as `Pass` (audit E1). Two
/// of its neighbours must *not* count — `# of expected passes` and
/// `# of expected failures`, the latter being an xfail, which the old shape
/// already got right — while `# of unexpected successes` (an xpass) must.
///
/// SRFI 78's `check` prints `*** failed ***` per case and a summary
/// `; *** checks *** : 9 correct, 1 failed.` — the count before the word
/// again, but spelled `failed`, and printed as `0 failed.` on success. So the
/// count has to be *read* rather than merely found, which is why the
/// before-the-word test rejects a zero.
///
/// srfi-175's bespoke `want` macro prints `Failed: wanted X but got Y` per
/// case and nothing on success — no count anywhere, and `FAIL` does not match
/// it because the check is case-sensitive. Until this shape was added, a
/// deliberately failing srfi-175 run classified as `Pass`.
fn test_suite_failed(stdout: &str) -> bool {
    if stdout.contains("FAIL") || stdout.contains("Failed:") || stdout.contains("*** failed ***") {
        return true;
    }
    let nonzero_count_before = |keyword: &str| {
        stdout.match_indices(keyword).any(|(i, _)| {
            stdout[..i]
                .chars()
                .rev()
                .take_while(char::is_ascii_digit)
                .any(|c| c != '0')
        })
    };
    if [" failure", " error", " failed"]
        .iter()
        .any(|keyword| nonzero_count_before(keyword))
    {
        return true;
    }
    [
        "# of failures",
        "# of unexpected successes",
        "# of unexpected passes",
    ]
    .iter()
    .any(|phrase| {
        stdout.match_indices(phrase).any(|(i, _)| {
            stdout[i + phrase.len()..]
                .split_whitespace()
                .next()
                .and_then(|count| count.parse::<u64>().ok())
                .is_some_and(|count| count > 0)
        })
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

    /// A package rooted under `temp` that provides one library.
    fn provider(slug: &str, temp: &Path, library: &str) -> Package {
        let mut pkg = package(slug, &temp.join(slug));
        pkg.provides.push(library.to_string());
        pkg
    }

    /// A package whose one library sits at `<root>/<file>` rather than where
    /// its name says, with `file` written out so staging has something to copy.
    fn off_path_provider(slug: &str, temp: &Path, library: &str, file: &str) -> Package {
        let mut pkg = provider(slug, temp, library);
        std::fs::create_dir_all(&pkg.root).unwrap();
        std::fs::write(
            pkg.root.join(file),
            format!("(define-library ({}))", library),
        )
        .unwrap();
        pkg.off_path_libraries.push(OffPathLibrary {
            name: library.to_string(),
            source: pkg.root.join(file),
        });
        pkg
    }

    /// Staging must carry the `.sld`'s neighbours along, or its relative
    /// `include`s break the moment it is read from its new home.
    #[test]
    fn staging_takes_the_includes_with_it() {
        let temp = tempfile::TempDir::new().unwrap();
        let pkg = off_path_provider("chibi-irregex", temp.path(), "chibi irregex", "irregex.sld");
        std::fs::write(pkg.root.join("irregex.scm"), "(define x 1)").unwrap();

        let scratch = temp.path().join("scratch");
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
        let scratch = temp.path().join("scratch");
        let mut subject = package("lassik-string-inflection", &temp.path().join("subject"));
        subject.test_depends.push("srfi 64".to_string());
        let framework = provider("srfi-64", temp.path(), "srfi 64");
        let framework_root = framework.root.clone();

        let universe = vec![framework];
        let providers = corpus::providers(&universe);
        let roots = search_roots(&subject, &universe, &providers, &scratch, true);
        assert!(roots.contains(&framework_root));

        // With no test program to run, they are irrelevant and stay off.
        let roots = search_roots(&subject, &universe, &providers, &scratch, false);
        assert!(!roots.contains(&framework_root));
    }

    /// An off-path library is just as unreachable when it is a dependency, so
    /// staging has to follow the closure rather than stop at the subject.
    #[test]
    fn a_dependency_with_an_off_path_library_is_staged_too() {
        let temp = tempfile::TempDir::new().unwrap();
        let scratch = temp.path().join("scratch");
        let mut subject = package("chibi-regexp", &temp.path().join("chibi-regexp"));
        subject.depends.push("chibi irregex".to_string());
        let dependency =
            off_path_provider("chibi-irregex", temp.path(), "chibi irregex", "irregex.sld");

        let universe = vec![dependency];
        let providers = corpus::providers(&universe);
        let roots = search_roots(&subject, &universe, &providers, &scratch, false);

        assert!(
            roots.iter().any(|r| r.join("chibi/irregex.sld").is_file()),
            "no search root resolves the dependency's library by name: {:?}",
            roots
        );
    }

    /// A dependency's own test framework is not part of what importing it
    /// requires, so the closure must not pull it in transitively.
    #[test]
    fn test_depends_are_not_followed_out_of_a_dependency() {
        let temp = tempfile::TempDir::new().unwrap();
        let scratch = temp.path().join("scratch");
        let mut subject = package("app", &temp.path().join("app"));
        subject.depends.push("some lib".to_string());

        let mut library = provider("some-lib", temp.path(), "some lib");
        library.test_depends.push("srfi 64".to_string());
        let library_root = library.root.clone();
        let framework = provider("srfi-64", temp.path(), "srfi 64");
        let framework_root = framework.root.clone();

        let universe = vec![library, framework];
        let providers = corpus::providers(&universe);
        let roots = search_roots(&subject, &universe, &providers, &scratch, true);
        assert!(roots.contains(&library_root));
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

    #[test]
    fn classifies_foreign_c_as_out_of_scope() {
        let out = captured("", "Error: Library (foreign c) not found", false);
        assert_eq!(
            classify(&out, "probe"),
            Status::OutOfScope(vec!["foreign c".to_string()])
        );
    }

    /// A library whose implementation is a shared object is out of scope, and
    /// says so at the cause. Before Patina recognised `include-shared` this
    /// arrived as `Exported identifier 'mecab?' not defined` — a load error
    /// naming a symptom two steps downstream.
    ///
    /// The message is *rendered from the real error* rather than pasted in as
    /// a literal, so the whole shape this extractor depends on — the marker,
    /// and the quoted name after it — is checked against the producing site.
    /// A pasted literal would only have pinned the copy in this file, leaving
    /// a Display reformat free to reclassify chibi-mecab with a green build.
    #[test]
    fn classifies_a_native_extension_as_out_of_scope() {
        let rendered = patina_runtime::LibraryError::NativeExtensionRequired {
            file: "chibi/mecab.sld".to_string(),
            extension: "mecab".to_string(),
        }
        .to_string();
        let out = captured("", &format!("Error: {rendered}"), false);
        assert_eq!(
            classify(&out, "probe"),
            Status::OutOfScope(vec!["mecab".to_string()])
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
            "Error: Parse error in /x/lib.sld: Exported identifier 'my-helper' not defined",
            false,
        );
        assert_eq!(
            classify(&out, "probe"),
            Status::LoadError(vec![
                "Exported identifier 'my-helper' not defined".to_string()
            ])
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

    /// A detail that itself contains colons must survive whole — splitting on
    /// the last one reported edn's duplicate-pattern-variable failure as the
    /// two-letter histogram row `ch` — while the constant layer tags
    /// ("desugar error: ", one "Invalid syntax: ") are stripped from the key.
    #[test]
    fn a_multi_colon_detail_is_not_truncated() {
        let out = captured(
            "",
            "Error: Parse error in /x/edn.sld: desugar error: Invalid syntax: \
             Failed to compile macro new-symbol?: Invalid syntax: \
             Duplicate pattern variable: ch",
            false,
        );
        assert_eq!(
            classify(&out, "test"),
            Status::ParseError(vec![
                "Failed to compile macro new-symbol?: \
                 Invalid syntax: Duplicate pattern variable: ch"
                    .to_string()
            ])
        );
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

    /// The shape that made this detector's own coverage the audit's E1
    /// finding: SRFI 64 puts the count *after* the phrase, lowercase, and
    /// exits 0. A deliberately failing one-test suite, run exactly as the
    /// harness runs it, produced this and classified as `Pass`.
    #[test]
    fn classifies_srfi_64_failures() {
        let out = captured(
            "%%%% Starting test example  (Writing full log to \"example.log\")\n\
             # of expected passes      2\n\
             # of failures             1\n",
            "",
            true,
        );
        assert_eq!(classify(&out, "test"), Status::WrongResult);
    }

    /// The near-miss on either side of it. `# of expected failures` is an
    /// xfail — a test that was *supposed* to fail — and is not a failure;
    /// `# of unexpected successes` is an xpass and is.
    #[test]
    fn srfi_64_expected_failures_pass_and_xpasses_do_not() {
        let xfail = captured(
            "# of expected passes      2\n# of expected failures    1\n",
            "",
            true,
        );
        assert_eq!(classify(&xfail, "test"), Status::Pass);

        let xpass = captured(
            "# of expected passes      2\n# of unexpected successes 1\n",
            "",
            true,
        );
        assert_eq!(classify(&xpass, "test"), Status::WrongResult);
    }

    /// srfi-175's bespoke `want` macro: `Failed: wanted ...` per case, no
    /// summary line, no count, exit 0. The audit shape — before the marker
    /// was added, exactly this output classified as `Pass`.
    #[test]
    fn classifies_bespoke_want_failures() {
        let failing = captured(
            "Failed: wanted #t but got #f from (ascii-char? #\\a)\n",
            "",
            true,
        );
        assert_eq!(classify(&failing, "test"), Status::WrongResult);

        // Success prints nothing at all, which must still pass.
        let passing = captured("", "", true);
        assert_eq!(classify(&passing, "test"), Status::Pass);
    }

    /// SRFI 78 spells it `failed`, prints the count before the word, and
    /// prints `0 failed.` on success — so the count has to be read.
    #[test]
    fn classifies_srfi_78_failures() {
        let failing = captured("; *** checks *** : 9 correct, 1 failed.\n", "", true);
        assert_eq!(classify(&failing, "test"), Status::WrongResult);

        let passing = captured("; *** checks *** : 10 correct, 0 failed.\n", "", true);
        assert_eq!(classify(&passing, "test"), Status::Pass);
    }

    /// A suite that fails *and* reaches an FFI stub is a `wrong-result`, not
    /// `out-of-scope`: the excusing bucket must not swallow a real failure.
    #[test]
    fn a_failing_suite_outranks_an_ffi_stub() {
        let out = captured(
            "FAIL: something\n",
            "Error: requires FFI, unavailable in Patina: file-owner",
            true,
        );
        assert_eq!(classify(&out, "test"), Status::WrongResult);

        // With no failure, the stub still classifies as out-of-scope.
        let stubbed = captured(
            "",
            "Error: requires FFI, unavailable in Patina: file-owner",
            true,
        );
        assert!(matches!(classify(&stubbed, "test"), Status::OutOfScope(_)));
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
