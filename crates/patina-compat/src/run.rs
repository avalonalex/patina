//! Execute the corpus against a patina binary and classify each package.
//!
//! Three execution modes per package:
//! - **test** — the package ships a test program (`(test "run-tests.scm")` in
//!   its `package.scm`); run it with `-k`, so an error that escapes to top
//!   level is reported and the suite still reaches its tally. Classification
//!   reads that tally first, then the exit status and stderr.
//! - **smoke** — a maintained assertion driver, with a checked completion tally.
//!   Run strictly so an error cannot be hidden by later assertions.
//! - **probe** — no test program; synthesize `(import ...)` of every library
//!   the package provides, and run it without `-k`, so its first error ends
//!   it and the exit status says so.

use crate::corpus::{self, Package};
use crate::evidence::{self, Evidence};
use patina_runtime::{Diagnostic, DiagnosticKind as Kind};
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
    /// export resolution, library-body evaluation. The producer records this
    /// separately from reader/expansion failures in the diagnostic stream.
    LoadError(Vec<String>),
    /// Libraries resolve but an identifier does not.
    UnboundIdentifier(Vec<String>),
    /// Loaded and ran, but its own test suite reports failures.
    WrongResult(Vec<String>),
    /// Errored at runtime in some other way.
    RuntimeError(Vec<String>),
    /// Did not finish within the per-package budget.
    Timeout,
    /// The package needs a foreign-function interface, proved three ways:
    /// every library it is missing is C-backed upstream (`FFI_BOUND`), a
    /// library it imports is a compiled shared object (`include-shared`), or
    /// it reached a bundled library's FFI stub at run time. The payload is
    /// whichever of those the evidence named — library names, shared-object
    /// names or stub procedures — which is why it serializes under the
    /// neutral `needs` key rather than `missing`.
    ///
    /// This classification does **not** discount the package from the score
    /// on its own; `compat/EXCLUSIONS.scm` does that, and the report warns
    /// about a row that lands here without an entry (PRD §6 risk: "the pass
    /// rate has a ceiling").
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
            Status::WrongResult(_) => "wrong-result",
            Status::RuntimeError(_) => "runtime-error",
            Status::Timeout => "timeout",
            Status::OutOfScope(_) => "out-of-scope",
        }
    }
}

/// One package's outcome.
#[derive(Debug)]
pub struct PackageResult {
    pub slug: String,
    pub mode: &'static str, // "test" | "smoke" | "probe"
    pub status: Status,
}

pub struct RunConfig {
    pub patina: PathBuf,
    pub tree_walker: bool,
    pub timeout: Duration,
    pub jobs: usize,
    /// The root holding third-party libraries Patina supplies but does not
    /// bundle (`test-lib/`). Every package gets it, the way every package
    /// used to get them for free from the bundled `lib/`.
    pub supplied_lib_root: PathBuf,
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
    let scratch = std::env::temp_dir().join(format!(
        "patina-compat-{}-{}",
        std::process::id(),
        package.slug
    ));
    let _ = std::fs::create_dir_all(&scratch);

    // A patched package runs its *patched* test program, not the vendored one:
    // the script is usually where the patched import lives, and running the
    // pristine copy against a patched library would measure neither tree.
    let patched_root = stage_patched_copy(&scratch, package);
    let (script, mode) = match &package.test_script {
        Some(path) => (
            redirect_into(path, &package.root, patched_root.as_deref()),
            "test",
        ),
        None => match &package.smoke {
            Some(smoke) => {
                let script = scratch.join("smoke.scm");
                std::fs::write(&script, crate::smoke::source(package, smoke))
                    .expect("write generated smoke driver");
                (script, "smoke")
            }
            None => (write_probe(&scratch, package), "probe"),
        },
    };
    let search_roots = search_roots(
        package,
        universe,
        providers,
        &scratch,
        mode != "probe",
        &config.supplied_lib_root,
        patched_root.as_deref(),
    );

    let mut cmd = Command::new(&config.patina);
    // A user's installed packages must neither shadow the corpus nor fill a
    // missing dependency and turn a failure into a pass. This also isolates
    // bootstrap; clearing environment variables after startup is too late.
    let diagnostics_path = scratch.join("diagnostics.jsonl");
    // Do not accept a previous run's stream if the child cannot start.
    let _ = std::fs::remove_file(&diagnostics_path);
    cmd.arg("--isolated-libraries")
        .arg("--diagnostics-file")
        .arg(&diagnostics_path);
    for root in &search_roots {
        cmd.arg("-A").arg(root);
    }
    if config.tree_walker {
        cmd.arg("--tree-walker");
    }
    // An upstream suite keeps going past a top-level error to print a tally.
    // Probes and maintained smoke drivers stop at their first uncaught error.
    if mode == "test" {
        cmd.arg("-k");
    }
    cmd.arg(&script)
        .current_dir(&scratch)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let outcome = spawn_with_timeout(cmd, config.timeout).map(|mut out| {
        out.diagnostics = std::fs::read_to_string(&diagnostics_path)
            .map_err(|e| format!("cannot read diagnostic stream: {e}"))
            .and_then(|text| patina_runtime::diagnostic::read_stream(&text));
        if let Err(error) = &out.diagnostics {
            eprintln!(
                "{}: {error}; rebuild the Patina binary before scoring",
                package.slug
            );
        }
        if mode == "test" && !out.timed_out && test_suite_failed(&out.stdout) {
            out.test_details =
                evidence::test_output(&out.stdout, Some(&scratch), test_suite_failed);
        }
        out
    });
    let _ = std::fs::remove_dir_all(&scratch);
    let mut status = match outcome {
        Err(e) => {
            eprintln!("warning: {}: spawn failed: {}", package.slug, e);
            Status::RuntimeError(evidence::bounded(&format!(
                "Could not run child process: {e}"
            )))
        }
        Ok(out) => {
            if mode == "smoke" {
                let smoke = package.smoke.as_ref().expect("smoke mode has a driver");
                let status = classify_smoke(&out, smoke.assertions);
                if status != Status::Pass {
                    eprintln!(
                        "{}: smoke failed or did not complete {} assertions\n{}{}",
                        package.slug, smoke.assertions, out.stdout, out.stderr
                    );
                }
                status
            } else {
                classify(&out, mode)
            }
        }
    };

    if let Status::WrongResult(lines) | Status::RuntimeError(lines) = &mut status {
        for line in lines {
            *line = line.replace(scratch.to_string_lossy().as_ref(), "<scratch>");
        }
    }

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
    supplied_lib_root: &Path,
    subject_patched_root: Option<&Path>,
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

    // The supplied root leads, ahead of every package's own. Not a preference
    // between the two — nothing in the corpus provides a library it holds —
    // but the order these libraries already had: they were bundled, and
    // `lib/` is a default search path, which every `-A` follows. Reproducing
    // that means moving them out of `lib/` cannot shift a tally by reordering,
    // and a reordering that does matter later has to be an argued change
    // rather than a side effect. It is placed here rather than at the
    // `Command` so the ordering is part of what this function returns, which
    // is what the tests below inspect.
    let mut roots = Vec::with_capacity(closure.len() + 1);
    roots.push(supplied_lib_root.to_path_buf());
    if is_test_run && let Some(smoke) = &package.smoke {
        roots.push(
            smoke
                .script
                .parent()
                .expect("smoke directory")
                .to_path_buf(),
        );
    }
    for pkg in closure {
        // A patched copy leads its own pristine root: the patch is the thing
        // being measured when one exists, and the vendored tree behind it is
        // never edited.
        //
        // The subject is never re-staged. `run_package` staged it already and
        // computed the test script's path *inside* that directory, and
        // `stage_patched_copy` begins by clearing it -- so staging it a second
        // time would delete the script out from under the path about to be
        // run. When that second copy failed for any reason the package scored
        // `runtime-error`, blaming the package for the harness's own churn;
        // measured by making the second call fail.
        if pkg.slug == package.slug {
            roots.extend(subject_patched_root.map(Path::to_path_buf));
        } else {
            roots.extend(stage_patched_copy(scratch, pkg));
        }
        roots.extend(stage_off_path_libraries(scratch, pkg));
        roots.push(pkg.root.clone());
    }
    roots
}

/// Re-root `path` from `from` into `into`, when a patched copy exists.
///
/// The test script sits inside the package tree, so when that tree has been
/// copied and patched the script must be taken from the copy. Falls back to
/// the original path when there is no patch, which is every package but a
/// handful.
fn redirect_into(path: &Path, from: &Path, into: Option<&Path>) -> PathBuf {
    let Some(into) = into else {
        return path.to_path_buf();
    };
    match path.strip_prefix(from) {
        Ok(rest) => into.join(rest),
        // The script lives outside the package root, so there is no patched
        // copy of it. Running it pristine against a patched library measures
        // neither tree, which is precisely what this function exists to
        // prevent -- so say so rather than doing it quietly.
        Err(_) => {
            eprintln!(
                "warning: test script {} is outside the package root {}; \
                 running it unpatched against a patched library",
                path.display(),
                from.display()
            );
            path.to_path_buf()
        }
    }
}

/// Where a package's patch lives, when it has one: `compat/patches/<slug>.patch`.
///
/// Returns `None` for the overwhelming majority of packages, which have none.
///
/// The path is derived as a *sibling of the corpus directory*
/// (`<corpus>/../patches`) rather than from a repo root, so `--vendor` keeps
/// working: point it elsewhere and the patches for that corpus are looked up
/// beside it. The sibling is only consulted when the package actually sits
/// one level inside a directory, which the `self_check` package -- rooted at
/// the supplied lib root -- deliberately does not rely on.
pub fn patch_for(package: &Package) -> Option<PathBuf> {
    // `<corpus>/<slug>` -> `<corpus>/../patches/<slug>.patch`
    let corpus = package.root.parent()?;
    let path = corpus
        .parent()?
        .join("patches")
        .join(format!("{}.patch", package.slug));
    path.is_file().then_some(path)
}

/// Copy the package into scratch and apply its patch there, returning the
/// search root that exposes the patched copy.
///
/// **`compat/vendor/` is never written to.** That directory's README calls it
/// "unmodified upstream copies kept for testing", and a harness that edited it
/// would be measuring its own edits — so a patch is applied to a throwaway
/// copy instead, and the vendored tree stays byte-identical to what upstream
/// shipped. The patch file is the reviewable record of the difference.
///
/// A patch that fails to apply warns loudly and the package then runs
/// *unpatched*, so the failure it was covering comes back and is scored as
/// such. That is the honest outcome: the alternative, silently keeping the
/// package's old result, would let a stale patch hide a regression. The
/// warning names the patch, and `every_patch_applies_to_its_package` in the
/// tests below fails the build rather than waiting for a corpus run.
fn stage_patched_copy(scratch: &Path, package: &Package) -> Option<PathBuf> {
    let patch = patch_for(package)?;
    let stage = scratch.join("patched").join(&package.slug);
    // The copy sits one level *inside* `stage`, and that nesting is required
    // rather than decorative: the patches are `-p1`, so patch(1) strips one
    // leading component and resolves the rest against this directory. Flatten
    // it and every patched package fails to apply at once.
    let dest = stage.join(&package.slug);
    // Clear first. `copy_tree` merges into whatever is already there, and this
    // path is reached twice for the same package whenever one patched package
    // depends on another: once staging it as a dependency, once for its own
    // run. Re-applying a patch to an already-patched tree makes patch(1)
    // announce "Reversed (or previously applied) patch detected!" and, with no
    // tty, answer its own prompt with -R -- silently *undoing* the patch and
    // exiting 0. That is how this produced a different score under --jobs 8
    // than under --jobs 1.
    let _ = std::fs::remove_dir_all(&dest);
    if let Err(e) = copy_tree(&package.root, &dest) {
        eprintln!(
            "warning: {}: could not stage for patching: {}",
            package.slug, e
        );
        return None;
    }
    let status = Command::new("patch")
        .arg("-p1")
        .arg("--silent")
        // `--fuzz=0`: context must match exactly. patch(1) will otherwise
        // slide a hunk past mismatched context and report success, which for
        // a *stale* patch means editing the wrong place quietly — the one
        // outcome that would make this mechanism worse than no mechanism.
        .arg("--fuzz=0")
        // Never reverse, and never ask: with stdin closed patch(1) would
        // otherwise take its own default and undo the patch.
        .arg("--forward")
        .arg("--batch")
        .arg("--input")
        .arg(&patch)
        .current_dir(&dest)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status();
    match status {
        Ok(s) if s.success() => Some(dest),
        Ok(s) => {
            eprintln!(
                "warning: {}: patch {} did not apply ({}); running unpatched",
                package.slug,
                patch.display(),
                s
            );
            None
        }
        Err(e) => {
            // patch(1) is not a declared build dependency, and without it
            // every patched package silently regresses to the failure its
            // patch removes -- which reads as a Patina regression rather than
            // a missing tool. A corpus scored without the patches is not the
            // corpus this repo reports, so stop rather than publish a number
            // that means something else.
            panic!(
                "{}: cannot run patch(1) for {}: {e}\n\
                 patch(1) is required whenever compat/patches/ is non-empty: \
                 scoring a patched package without it reports the failure the \
                 patch exists to remove, which reads as a Patina regression.",
                package.slug,
                patch.display()
            );
        }
    }
}

/// Give the package's off-path libraries the layout their names imply, in a
/// throwaway directory, and return the search root that exposes it (`None`
/// when the package needs no staging, which is the norm).
///
/// The `.sld`'s whole directory is mirrored, not just the file, so its
/// relative `include`s still resolve from the staged copy. Snow's builder
/// likewise relocates includes before installation; this harness needs its
/// own staging because it runs extracted sources without invoking that builder.
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

/// Write a synthesized import probe into the scratch directory. Probes run
/// without `-k`, so the first error ends one and its exit status says so.
///
/// The probe is the import form and nothing else. It used to end in a
/// `(display "patina-compat probe ok")` that nothing ever read — `classify`
/// decides a probe on exit status and stderr alone — and that decorative call
/// is how #301 (issue #211), which took `display` out of `(scheme base)`,
/// became a harness failure scored as Patina's: 104 of the 114 probe-mode
/// packages filed as `unbound-identifier` on `display`. A probe that calls
/// nothing depends on no library's export list, so no such change can strand
/// it again, and [`self_check`] proves the binary can run it before a corpus
/// run is scored.
fn write_probe(scratch: &std::path::Path, package: &Package) -> PathBuf {
    let path = scratch.join("probe.scm");
    std::fs::write(&path, probe_source(package)).expect("write probe file");
    path
}

/// The probe's text: an import of `(scheme base)` and every library the
/// package provides. `(scheme base)` keeps the form well-formed for a package
/// that provides nothing; it is the one library every corpus program needs
/// loaded anyway.
fn probe_source(package: &Package) -> String {
    let mut source = String::from("(import (scheme base)");
    for lib in &package.provides {
        source.push_str(&format!(" ({})", lib));
    }
    source.push_str(")\n");
    source
}

/// Run the probe for a package that provides nothing, on the binary and
/// backend the corpus will be scored with, and return its classification.
///
/// A probe that cannot pass on an empty package cannot pass on any package,
/// so anything but `Pass` here means the harness or the binary under test —
/// not a third-party library — is what a corpus run would measure. That is
/// how #301 wrote 23 of 161 into the committed snapshot; the caller refuses
/// to score in that case. Only bundled libraries are exercised: the supplied
/// root is covered by the caller's own directory check.
pub fn self_check(config: &RunConfig) -> Status {
    run_package(&self_check_package(config), &[], &BTreeMap::new(), config).status
}

/// The empty package the self-check runs. Its root is the supplied library
/// root, which `search_roots` lists first anyway, so the probe sees exactly
/// the roots every corpus package sees and no directory of its own.
fn self_check_package(config: &RunConfig) -> Package {
    Package {
        slug: "self-check".to_string(),
        root: config.supplied_lib_root.clone(),
        provides: Vec::new(),
        depends: Vec::new(),
        test_depends: Vec::new(),
        test_script: None,
        smoke: None,
        off_path_libraries: Vec::new(),
    }
}

struct Captured {
    diagnostics: Result<Vec<Diagnostic>, String>,
    stdout: String,
    stderr: String,
    test_details: Vec<String>,
    exit_status: Option<String>,
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
    let (exit_ok, timed_out, exit_status) = loop {
        match child.try_wait().map_err(|e| e.to_string())? {
            Some(status) => break (status.success(), false, Some(status.to_string())),
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break (false, true, None);
            }
            None => std::thread::sleep(Duration::from_millis(25)),
        }
    };

    let stdout = String::from_utf8_lossy(&stdout_thread.join().unwrap()).into_owned();
    let stderr = String::from_utf8_lossy(&stderr_thread.join().unwrap()).into_owned();
    Ok(Captured {
        diagnostics: Err("diagnostic stream not read".into()),
        stdout,
        stderr,
        test_details: Vec::new(),
        exit_status,
        exit_ok,
        timed_out,
    })
}

/// A completion tally cannot excuse an interpreter error, failing exit or
/// timeout, even when it was printed before that failure.
fn classify_smoke(out: &Captured, assertions: usize) -> Status {
    match classify(out, "smoke") {
        Status::Pass => crate::smoke::completion(&out.stdout, assertions),
        failure => failure,
    }
}

fn classify(out: &Captured, mode: &str) -> Status {
    if out.timed_out {
        return Status::Timeout;
    }

    let Ok(diagnostics) = &out.diagnostics else {
        return Status::RuntimeError(runtime_evidence(out));
    };
    let payloads = |kind, payload: fn(&Diagnostic) -> Option<String>| {
        let mut values: Vec<_> = diagnostics
            .iter()
            .filter(|d| d.kind == kind)
            .filter_map(payload)
            .collect();
        values.sort();
        values.dedup();
        values
    };
    let missing = payloads(Kind::MissingLibrary, |d| {
        d.library.as_ref().map(|n| n.join(" "))
    });
    if !missing.is_empty() {
        return if missing.iter().all(|l| FFI_BOUND.contains(&l.as_str())) {
            Status::OutOfScope(missing)
        } else {
            Status::MissingLibrary(missing)
        };
    }

    // Reader/expansion failures outrank downstream loading symptoms. The
    // producer names the stage; no human-readable prefix participates.
    let mut parse_errors = payloads(Kind::Parse, |d| Some(d.message.clone()));
    parse_errors.extend(payloads(Kind::Syntax, |d| Some(d.message.clone())));
    parse_errors.sort();
    parse_errors.dedup();
    if !parse_errors.is_empty() {
        return Status::ParseError(parse_errors);
    }
    let load_errors = payloads(Kind::Load, |d| Some(d.message.clone()));
    if !load_errors.is_empty() {
        return Status::LoadError(load_errors);
    }

    // The library's implementation is a compiled shared object, and Patina
    // refused that clause deliberately. Placed with the load-stage failures
    // above rather than at the top of the function, and the two halves of
    // that are separate decisions:
    //
    // *Below* missing libraries and parse errors, because this is an excusing
    // bucket and one must never swallow a real failure (audit E3). A package
    // that needs a `.so` **and** trips a genuine parse error in some other
    // library still reports the parse error, so it reaches the work queue.
    //
    // *Above* unbound identifiers, for the reason the parse/unbound ordering
    // already encodes: a library that never loads leaves its importer unbound
    // and its test program's entry point undefined, so this is the cause and
    // those are its symptoms. `chibi-mecab` classified as
    // `unbound-identifier: run-chibi-mecab-test-tests` until it moved up.
    //
    // Note this is *not* the ordering the FFI-stub check below gets, and the
    // difference is what each proves. A stub marker is raised at run time, so
    // the suite did run and its verdict is real evidence to weigh. This
    // refusal happens before anything runs at all.
    let extensions = payloads(Kind::NativeExtension, |d| d.extension.clone());
    if !extensions.is_empty() {
        return Status::OutOfScope(extensions);
    }

    let unbound = payloads(Kind::UnboundIdentifier, |d| d.identifier.clone());
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
        return Status::WrongResult(if out.test_details.is_empty() {
            evidence::test_output(&out.stdout, None, test_suite_failed)
        } else {
            out.test_details.clone()
        });
    }

    // A supplied library can be honest about its own limits. `(chibi filesystem)`
    // implements its portable half and stubs the POSIX half with this marker
    // (test-lib/chibi/filesystem.sld), so a package that reaches one of those stubs
    // is FFI-bound in exactly the sense FFI_BOUND means — it just proved it by
    // running instead of by failing to import. Without this it would be filed
    // as a runtime-error, i.e. as our defect.
    if let Some(stubs) = extract_ffi_stubs([out.stdout.as_str(), out.stderr.as_str()]) {
        return Status::OutOfScope(stubs);
    }

    // A test run under `-k` and a probe both exit non-zero once an error is
    // reported. The diagnostic check stays beside the status, so the verdict does
    // not rest on one signal: a suite that reached its own `(test-exit)` is
    // judged by what it printed as well as by how it ended.
    if !diagnostics.is_empty() || !out.exit_ok {
        return Status::RuntimeError(runtime_evidence(out));
    }

    Status::Pass
}

fn runtime_evidence(out: &Captured) -> Vec<String> {
    let mut details = Evidence::default();
    match &out.diagnostics {
        Err(error) => details.push(&format!("Diagnostic stream error: {error}")),
        Ok(diagnostics) => {
            for diagnostic in diagnostics {
                details.push(&diagnostic.message);
                if let Some(path) = &diagnostic.path {
                    details.push(&format!("path: {path}"));
                }
            }
        }
    }
    if !out.exit_ok {
        details.push(
            out.exit_status
                .as_deref()
                .unwrap_or("Child process exited unsuccessfully"),
        );
    }
    // Typed diagnostics take precedence over prose. Stderr (or stdout if
    // stderr is empty) is still useful for a crash or an explicit exit.
    if !out.diagnostics.as_ref().is_ok_and(|ds| !ds.is_empty()) {
        details.push(if out.stderr.trim().is_empty() {
            &out.stdout
        } else {
            &out.stderr
        });
    }
    let lines = details.finish();
    if lines.is_empty() {
        evidence::bounded("Runtime diagnostic contained no message")
    } else {
        lines
    }
}

pub(crate) fn smoke_failure_evidence(stdout: &str) -> Vec<String> {
    evidence::test_output(stdout, None, test_suite_failed)
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

    /// Every patch in `compat/patches/` applies cleanly to the package it
    /// names, and changes something.
    ///
    /// A patch goes stale the moment its package is re-vendored, and the
    /// runner's response to that is to warn and run unpatched — correct, but
    /// it surfaces only during a corpus run, as what looks like the package's
    /// own regression. This fails the build instead, which is where a stale
    /// patch is cheap to notice.
    ///
    /// It also rejects a patch that applies but is a no-op, since one that
    /// changes nothing is a patch someone believes is doing something.
    #[test]
    fn every_patch_applies_to_its_package() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("repo root");
        let patches = repo.join("compat").join("patches");
        let vendor = repo.join("compat").join("vendor");
        // Not an early return: a missing directory is the one condition under
        // which the `checked > 0` assertion below cannot fire, so it would
        // turn this into a test that passes by looking at nothing.
        assert!(
            patches.is_dir() && vendor.is_dir(),
            "expected {} and {} — if the corpus moved, this guard stopped \
             checking anything and patch_for in the runner needs the same fix",
            patches.display(),
            vendor.display()
        );
        let temp = tempfile::TempDir::new().expect("temp dir");
        let temp = temp.path();
        let mut checked = 0;
        for entry in std::fs::read_dir(&patches).expect("read compat/patches") {
            let patch = entry.expect("dir entry").path();
            if patch.extension().and_then(|e| e.to_str()) != Some("patch") {
                continue;
            }
            // Strip exactly the `.patch` suffix. `file_stem` splits on the
            // *last* dot, so a slug containing one (`srfi-179.0.1`) would map
            // to the wrong directory — and the runner builds the name the
            // other way, as `format!("{slug}.patch")`, so the two must be
            // inverses or a patch could exist that only one of them sees.
            let slug = patch
                .file_name()
                .and_then(|s| s.to_str())
                .and_then(|s| s.strip_suffix(".patch"))
                .expect("patch file name");
            let source = vendor.join(slug);
            assert!(
                source.is_dir(),
                "{}: names no vendored package at {}",
                patch.display(),
                source.display()
            );
            let dest = temp.join(slug);
            let _ = std::fs::remove_dir_all(&dest);
            copy_tree(&source, &dest).expect("stage package for patch check");
            let out = Command::new("patch")
                // No `--silent` here, unlike the runner: when a patch goes
                // stale this message is the whole diagnosis, and patch(1)'s
                // per-hunk commentary is what says *which* hunk drifted.
                .args(["-p1", "--fuzz=0", "--forward", "--batch", "--input"])
                .arg(&patch)
                .current_dir(&dest)
                .stdin(Stdio::null())
                .output()
                .expect("run patch(1)");
            assert!(
                out.status.success(),
                "{} does not apply to compat/vendor/{}:\n{}{}",
                patch.display(),
                slug,
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            );
            // Compare only the files the patch names. A bare `diff -r` would
            // also see patch(1)'s own `.rej`/`.orig` artifacts and read them
            // as evidence the patch did something, which is the opposite of
            // what this checks.
            let text = std::fs::read_to_string(&patch).expect("read patch");
            let touched: Vec<String> = text
                .lines()
                .filter_map(|line| line.strip_prefix("+++ b/"))
                .map(|rest| rest.split('\t').next().unwrap_or(rest).trim().to_string())
                .collect();
            assert!(
                !touched.is_empty(),
                "{} names no files (no `+++ b/` lines)",
                patch.display()
            );
            let changed = touched.iter().any(|rel| {
                std::fs::read(source.join(rel)).ok() != std::fs::read(dest.join(rel)).ok()
            });
            assert!(
                changed,
                "{} applies but changes none of the files it names: {:?}",
                patch.display(),
                touched
            );
            checked += 1;
        }
        assert!(
            checked > 0,
            "compat/patches/ exists but holds no .patch files — this check would pass vacuously"
        );
    }

    fn package(slug: &str, root: &Path) -> Package {
        Package {
            slug: slug.to_string(),
            root: root.to_path_buf(),
            provides: Vec::new(),
            depends: Vec::new(),
            test_depends: Vec::new(),
            test_script: None,
            smoke: None,
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

    /// Stand-in for `RunConfig::supplied_lib_root` in the tests below. Its
    /// contents are irrelevant here — `search_roots` only places it.
    const SUPPLIED_STR: &str = "/supplied-lib-root";

    fn supplied() -> &'static Path {
        Path::new(SUPPLIED_STR)
    }

    /// The supplied root leads every package's own, which is the order the
    /// libraries had while they were bundled (`lib/` is a default search path,
    /// and every `-A` follows the defaults). Pinned because `run_package`
    /// turns this vector into `-A` flags in order, so the claim is only as
    /// real as this assertion.
    #[test]
    fn the_supplied_library_root_leads_every_package_root() {
        let temp = tempfile::TempDir::new().unwrap();
        let scratch = temp.path().join("scratch");
        let subject = package("lassik-string-inflection", &temp.path().join("subject"));
        let universe = vec![];
        let providers = corpus::providers(&universe);

        let roots = search_roots(
            &subject,
            &universe,
            &providers,
            &scratch,
            false,
            supplied(),
            None,
        );
        assert_eq!(roots.first().map(PathBuf::as_path), Some(supplied()));
        assert!(roots.contains(&subject.root));
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
        let roots = search_roots(
            &subject,
            &universe,
            &providers,
            &scratch,
            true,
            supplied(),
            None,
        );
        assert!(roots.contains(&framework_root));

        // With no test program to run, they are irrelevant and stay off.
        let roots = search_roots(
            &subject,
            &universe,
            &providers,
            &scratch,
            false,
            supplied(),
            None,
        );
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
        let roots = search_roots(
            &subject,
            &universe,
            &providers,
            &scratch,
            false,
            supplied(),
            None,
        );

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
        let roots = search_roots(
            &subject,
            &universe,
            &providers,
            &scratch,
            true,
            supplied(),
            None,
        );
        assert!(roots.contains(&library_root));
        assert!(!roots.contains(&framework_root));
    }

    fn captured(stdout: &str, stderr: &str, exit_ok: bool) -> Captured {
        Captured {
            diagnostics: Ok(Vec::new()),
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            test_details: Vec::new(),
            exit_status: None,
            exit_ok,
            timed_out: false,
        }
    }

    #[cfg(unix)]
    #[test]
    fn suite_log_is_read_before_scratch_is_removed() {
        use std::os::unix::fs::PermissionsExt;
        let temp = tempfile::tempdir().unwrap();
        let patina = temp.path().join("patina");
        std::fs::write(
            &patina,
            r#"#!/bin/sh
previous=
for arg do
    if [ "$previous" = --diagnostics-file ]; then
        printf '%s\n' '{"protocol":"patina-diagnostics","version":1}' > "$arg"
    fi
    previous=$arg
    program=$arg
done
pwd > "$program.scratch"
cat > suite.log <<'LOG'
Test begin:
  test-name: logged failure
  source-form: (+ 1 1)
Test end:
  result-kind: fail
  actual-value: 2
  expected-value: 3
LOG
printf '%s\n' '%%%% Starting test suite (Writing full log to "suite.log")' '# of failures      1'
"#,
        )
        .unwrap();
        std::fs::set_permissions(&patina, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut package = package("failure-evidence", temp.path());
        package.test_script = Some(temp.path().join("test.scm"));
        let config = RunConfig {
            patina,
            tree_walker: false,
            timeout: Duration::from_secs(10),
            jobs: 1,
            supplied_lib_root: temp.path().to_path_buf(),
        };
        let result = run_package(&package, &[], &BTreeMap::new(), &config);
        let Status::WrongResult(lines) = result.status else {
            panic!("{result:?}");
        };
        for expected in [
            "test-name: logged failure",
            "source-form: (+ 1 1)",
            "actual-value: 2",
            "expected-value: 3",
        ] {
            assert!(lines.iter().any(|line| line == expected), "{lines:?}");
        }
        let scratch = std::fs::read_to_string(temp.path().join("test.scm.scratch")).unwrap();
        assert!(!Path::new(scratch.trim()).exists());
    }

    #[test]
    fn runtime_evidence_prefers_diagnostics_and_explains_protocol_and_exit_failures() {
        let mut out = captured("unrelated output", "duplicate prose", false);
        out.exit_status = Some("exit status: 7".into());
        out.diagnostics = Ok(vec![Diagnostic::new(Kind::Runtime, "division by zero: λ")]);
        assert_eq!(
            classify(&out, "probe"),
            Status::RuntimeError(vec!["division by zero: λ".into(), "exit status: 7".into()])
        );
        out.diagnostics = Err("unsupported version".into());
        assert_eq!(
            classify(&out, "probe"),
            Status::RuntimeError(vec![
                "Diagnostic stream error: unsupported version".into(),
                "exit status: 7".into(),
                "duplicate prose".into()
            ])
        );
        out.diagnostics = Ok(vec![]);
        out.stderr.clear();
        assert_eq!(
            classify(&out, "probe"),
            Status::RuntimeError(vec!["exit status: 7".into(), "unrelated output".into()])
        );
        out.diagnostics = Ok(vec![
            Diagnostic::new(Kind::Io, "No such file or directory").at_path("missing test.scm"),
        ]);
        assert_eq!(
            classify(&out, "probe"),
            Status::RuntimeError(vec![
                "No such file or directory".into(),
                "path: missing test.scm".into(),
                "exit status: 7".into(),
            ])
        );
    }

    #[test]
    fn suite_evidence_retains_tallies_and_context_without_durations() {
        for (stdout, expected) in [
            (
                "\u{1b}[31mFAIL: add\u{1b}[0m\n  expected 3 but got 2\n  on line 4: (+ 1 1)\n52 out of 53 tests passed in 0.123 seconds.\n1 failure (1.9%).\n",
                "expected 3 but got 2",
            ),
            (
                "(+ 1 1) => 2\n; *** failed ***\n; expected result: 3\n; *** checks *** : 2 correct, 1 failed.\n",
                "expected result: 3",
            ),
        ] {
            let Status::WrongResult(lines) = classify(&captured(stdout, "", true), "test") else {
                panic!("{stdout}");
            };
            let text = lines.join("\n");
            assert!(text.contains(expected), "{text}");
            assert!(
                !text.contains("0.123") && !text.contains('\u{1b}'),
                "{text}"
            );
        }
    }

    #[test]
    fn smoke_completion_never_hides_an_execution_failure() {
        let complete = "(patina-compat-smoke 3 0)\n";
        assert_eq!(
            classify_smoke(&captured(complete, "", true), 3),
            Status::Pass
        );
        assert!(matches!(
            classify_smoke(&captured(complete, "", false), 3),
            Status::RuntimeError(_)
        ));
        let mut late_error = captured(complete, "wording changed", true);
        late_error.diagnostics = Ok(vec![Diagnostic::new(Kind::Runtime, "late failure")]);
        assert!(matches!(
            classify_smoke(&late_error, 3),
            Status::RuntimeError(_)
        ));
        let mut timeout = captured(complete, "", true);
        timeout.timed_out = true;
        assert_eq!(classify_smoke(&timeout, 3), Status::Timeout);
        assert!(matches!(
            classify_smoke(&captured("", "", true), 3),
            Status::RuntimeError(_)
        ));
        assert!(matches!(
            classify_smoke(&captured("(patina-compat-smoke 2 1)\n", "", true), 3),
            Status::WrongResult(_)
        ));
    }

    fn diagnosed(diagnostics: Vec<Diagnostic>) -> Captured {
        Captured {
            diagnostics: Ok(diagnostics),
            ..captured("", "prose can say anything", false)
        }
    }

    fn missing(name: &[&str]) -> Diagnostic {
        let mut d = Diagnostic::new(Kind::MissingLibrary, "arbitrary library error wording");
        d.library = Some(name.iter().map(|s| s.to_string()).collect());
        d
    }

    fn unbound(name: &str) -> Diagnostic {
        let mut d = Diagnostic::new(Kind::UnboundIdentifier, "arbitrary lookup wording");
        d.identifier = Some(name.into());
        d
    }

    fn native() -> Diagnostic {
        let mut d = Diagnostic::new(Kind::NativeExtension, "arbitrary native extension wording");
        d.extension = Some("mecab".into());
        d
    }

    #[test]
    fn classifications_use_typed_payloads_despite_misleading_prose() {
        for (diagnostic, expected) in [
            (
                missing(&["foo", "bar"]),
                Status::MissingLibrary(vec!["foo bar".into()]),
            ),
            (
                missing(&["chibi", "ast"]),
                Status::OutOfScope(vec!["chibi ast".into()]),
            ),
            (
                missing(&["foreign", "c"]),
                Status::OutOfScope(vec!["foreign c".into()]),
            ),
            (
                unbound("odd name: `λ`"),
                Status::UnboundIdentifier(vec!["odd name: `λ`".into()]),
            ),
            (native(), Status::OutOfScope(vec!["mecab".into()])),
            (
                Diagnostic::new(Kind::Load, "Export missing: x: y"),
                Status::LoadError(vec!["Export missing: x: y".into()]),
            ),
            (
                Diagnostic::new(Kind::Parse, "reader: detail: λ"),
                Status::ParseError(vec!["reader: detail: λ".into()]),
            ),
            (
                Diagnostic::new(Kind::Syntax, "bad macro"),
                Status::ParseError(vec!["bad macro".into()]),
            ),
        ] {
            let mut out = diagnosed(vec![diagnostic]);
            out.stderr =
                "Error: Library (misleading) not found\nError: Undefined variable: wrong".into();
            out.stdout = "Parse error in /fake: wrong".into();
            assert_eq!(classify(&out, "probe"), expected);
        }
    }

    #[test]
    fn causal_failure_precedence_is_preserved() {
        let parse = Diagnostic::new(Kind::Parse, "bad datum");
        let load = Diagnostic::new(Kind::Load, "bad export");
        for (ds, expected) in [
            (
                vec![native(), parse.clone(), load.clone(), unbound("entry")],
                Status::ParseError(vec!["bad datum".into()]),
            ),
            (
                vec![native(), load, unbound("entry")],
                Status::LoadError(vec!["bad export".into()]),
            ),
            (
                vec![native(), unbound("entry")],
                Status::OutOfScope(vec!["mecab".into()]),
            ),
            (
                vec![missing(&["chibi", "ast"]), missing(&["foo"])],
                Status::MissingLibrary(vec!["chibi ast".into(), "foo".into()]),
            ),
            (
                vec![unbound("z"), unbound("a"), unbound("z")],
                Status::UnboundIdentifier(vec!["a".into(), "z".into()]),
            ),
        ] {
            assert_eq!(classify(&diagnosed(ds), "test"), expected);
        }
    }

    #[test]
    fn ordinary_output_cannot_impersonate_an_interpreter_error() {
        let text = "Error: Library (foo) not found\nError: Undefined variable: x\nParse error in x: broken";
        assert_eq!(classify(&captured(text, text, true), "probe"), Status::Pass);
    }

    #[test]
    fn invalid_diagnostic_stream_cannot_pass_or_excuse_a_package() {
        let mut out = captured("", "requires FFI, unavailable in Patina: open", true);
        out.diagnostics = Err("unsupported version".into());
        assert!(matches!(classify(&out, "probe"), Status::RuntimeError(_)));
    }

    #[test]
    fn reached_ffi_stub_remains_an_explicit_scheme_protocol() {
        let mut out = captured(
            "",
            "Error: requires FFI, unavailable in Patina: open",
            false,
        );
        assert_eq!(
            classify(&out, "probe"),
            Status::OutOfScope(vec!["open".into()])
        );
        out.diagnostics = Ok(vec![unbound("frobnicate")]);
        assert_eq!(
            classify(&out, "probe"),
            Status::UnboundIdentifier(vec!["frobnicate".into()])
        );
    }

    #[test]
    fn classifies_test_failures_despite_exit_zero() {
        let out = captured("52 out of 53 tests passed.\n1 failure (1.9%).\n", "", true);
        assert!(matches!(classify(&out, "test"), Status::WrongResult(_)));
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
        assert!(matches!(classify(&out, "test"), Status::WrongResult(_)));
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
        assert!(matches!(classify(&xpass, "test"), Status::WrongResult(_)));
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
        assert!(matches!(classify(&failing, "test"), Status::WrongResult(_)));

        // Success prints nothing at all, which must still pass.
        let passing = captured("", "", true);
        assert_eq!(classify(&passing, "test"), Status::Pass);
    }

    /// SRFI 78 spells it `failed`, prints the count before the word, and
    /// prints `0 failed.` on success — so the count has to be read.
    #[test]
    fn classifies_srfi_78_failures() {
        let failing = captured("; *** checks *** : 9 correct, 1 failed.\n", "", true);
        assert!(matches!(classify(&failing, "test"), Status::WrongResult(_)));

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
        assert!(matches!(classify(&out, "test"), Status::WrongResult(_)));

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
        assert!(matches!(classify(&out, "probe"), Status::RuntimeError(_)));
    }

    #[test]
    fn timeout_wins() {
        let out = Captured {
            diagnostics: Ok(Vec::new()),
            stdout: String::new(),
            stderr: "Error: Library (foo) not found".to_string(),
            test_details: Vec::new(),
            exit_status: None,
            exit_ok: false,
            timed_out: true,
        };
        assert_eq!(classify(&out, "probe"), Status::Timeout);
    }

    /// The probe is the import form alone. Pinned because the body it used to
    /// carry — a `display` of a marker nothing read — is what #301 stranded:
    /// a probe that calls nothing depends on no library's export list, and
    /// re-adding a call would quietly re-create that dependency.
    #[test]
    fn the_probe_is_an_import_form_and_nothing_else() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(
            probe_source(&package("provides-nothing", temp.path())),
            "(import (scheme base))\n"
        );
        let mut pkg = package("provider", temp.path());
        pkg.provides = vec!["foo bar".to_string(), "srfi 1".to_string()];
        assert_eq!(
            probe_source(&pkg),
            "(import (scheme base) (foo bar) (srfi 1))\n"
        );
        // Written verbatim, under a fixed basename.
        let path = write_probe(temp.path(), &pkg);
        assert_eq!(path.file_name().unwrap(), "probe.scm");
        assert_eq!(std::fs::read_to_string(path).unwrap(), probe_source(&pkg));
    }

    /// The self-check probes an empty package through the same roots every
    /// corpus package gets, and nothing else: no directory of its own.
    #[test]
    fn the_self_check_package_sees_only_the_shared_roots() {
        let temp = tempfile::tempdir().unwrap();
        let config = RunConfig {
            patina: PathBuf::new(),
            tree_walker: false,
            timeout: Duration::from_secs(1),
            jobs: 1,
            supplied_lib_root: temp.path().join("supplied"),
        };
        let pkg = self_check_package(&config);
        assert!(pkg.provides.is_empty() && pkg.test_script.is_none());
        let roots = search_roots(
            &pkg,
            &[],
            &BTreeMap::new(),
            &temp.path().join("scratch"),
            false,
            &config.supplied_lib_root,
            None,
        );
        assert!(
            !roots.is_empty() && roots.iter().all(|r| *r == config.supplied_lib_root),
            "{roots:?}"
        );
    }
}
