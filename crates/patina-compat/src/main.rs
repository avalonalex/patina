//! patina-compat — the self-contained third-party compatibility harness
//! (Track L, work item L3).
//!
//! Runs the vendored corpus in `compat/vendor/` against a patina binary and
//! reports "N of M packages pass", with failure buckets whose histograms are
//! the bundling work queue. Self-containment invariant: nothing here shells
//! out to anything but the patina binary under test.
//!
//!     cargo run -p patina-compat --release -- run
//!     cargo run -p patina-compat --release -- report

mod corpus;
mod exclusions;
mod report;
mod run;
mod sexp;

use run::RunConfig;
use std::path::PathBuf;
use std::process;
use std::time::Duration;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf()
}

fn print_help() {
    eprintln!("Usage: patina-compat <run|report> [OPTIONS]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  run      Execute the corpus, write both artifacts, print the report");
    eprintln!("  report   Re-render the report from an existing results snapshot");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --patina <path>   Binary under test (default: target/release/patina)");
    eprintln!("  --vendor <dir>    Corpus directory (default: compat/vendor)");
    eprintln!("  --results <file>  Snapshot path (default: compat/reports/results.scm)");
    eprintln!(
        "  --report <file>   Rendered matrix path, run only (default: compat/reports/report.md)"
    );
    eprintln!("  --exclusions <f>  Opt-out list (default: compat/EXCLUSIONS.scm; `none` disables)");
    eprintln!("  --filter <substr> Only packages whose slug contains <substr>");
    eprintln!("  --tree-walker     Test the tree-walking backend instead of the VM");
    eprintln!("  --timeout <secs>  Per-package budget (default: 30)");
    eprintln!("  --jobs <n>        Parallel packages (default: available cores, max 8)");
}

enum Command {
    Run,
    Report,
}

struct Options {
    command: Option<Command>,
    patina: PathBuf,
    vendor: PathBuf,
    /// Output paths, `None` meaning "use the canonical committed location".
    /// Being given one is also what authorizes writing that artifact for a
    /// filtered (subset) run — so authorization is per artifact, and a
    /// subset run cannot overwrite the canonical copy of the *other* one.
    results_path: Option<PathBuf>,
    report_path: Option<PathBuf>,
    /// Opt-out list, `None` meaning the canonical committed location and
    /// `Some("none")` meaning "score every package" — the escape hatch that
    /// makes the raw corpus reachable without editing a committed file.
    exclusions_path: Option<PathBuf>,
    filter: Option<String>,
    tree_walker: bool,
    timeout: Duration,
    jobs: usize,
}

impl Options {
    fn results_path(&self) -> PathBuf {
        self.results_path
            .clone()
            .unwrap_or_else(|| workspace_root().join("compat/reports/results.scm"))
    }

    fn report_path(&self) -> PathBuf {
        self.report_path
            .clone()
            .unwrap_or_else(|| workspace_root().join("compat/reports/report.md"))
    }

    /// The opt-out list to apply, `None` when the caller asked for none.
    fn exclusions_path(&self) -> Option<PathBuf> {
        match self.exclusions_path.as_deref() {
            Some(p) if p == std::path::Path::new("none") => None,
            Some(p) => Some(p.to_path_buf()),
            None => Some(workspace_root().join("compat/EXCLUSIONS.scm")),
        }
    }
}

/// Load the opt-out list, or exit: a malformed list must stop the run rather
/// than silently score every package, which would look like a jump in the
/// headline number with nothing in the diff to explain it.
fn load_exclusions(opts: &Options, heap: &patina_core::SharedHeap) -> Vec<exclusions::Exclusion> {
    let Some(path) = opts.exclusions_path() else {
        return Vec::new();
    };
    match exclusions::load(&path, heap) {
        Ok(list) => list,
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(2);
        }
    }
}

/// Take the value following a flag, or exit with a usage error.
fn require_value(iter: &mut std::slice::Iter<'_, String>, flag: &str) -> String {
    iter.next().cloned().unwrap_or_else(|| {
        eprintln!("Error: {} requires a value", flag);
        process::exit(2);
    })
}

fn parse_args() -> Options {
    let root = workspace_root();
    let mut opts = Options {
        command: None,
        patina: root.join("target/release/patina"),
        vendor: root.join("compat/vendor"),
        results_path: None,
        report_path: None,
        exclusions_path: None,
        filter: None,
        tree_walker: false,
        timeout: Duration::from_secs(30),
        jobs: std::thread::available_parallelism()
            .map(|n| n.get().min(8))
            .unwrap_or(4),
    };

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "run" => opts.command = Some(Command::Run),
            "report" => opts.command = Some(Command::Report),
            "--patina" => opts.patina = PathBuf::from(require_value(&mut iter, "--patina")),
            "--vendor" => opts.vendor = PathBuf::from(require_value(&mut iter, "--vendor")),
            "--results" => {
                opts.results_path = Some(PathBuf::from(require_value(&mut iter, "--results")))
            }
            "--report" => {
                opts.report_path = Some(PathBuf::from(require_value(&mut iter, "--report")))
            }
            "--exclusions" => {
                opts.exclusions_path = Some(PathBuf::from(require_value(&mut iter, "--exclusions")))
            }
            "--filter" => opts.filter = Some(require_value(&mut iter, "--filter")),
            "--tree-walker" => opts.tree_walker = true,
            "--timeout" => {
                let secs: u64 = require_value(&mut iter, "--timeout")
                    .parse()
                    .unwrap_or_else(|_| {
                        eprintln!("Error: --timeout expects seconds");
                        process::exit(2);
                    });
                opts.timeout = Duration::from_secs(secs);
            }
            "--jobs" => {
                opts.jobs = require_value(&mut iter, "--jobs")
                    .parse()
                    .unwrap_or_else(|_| {
                        eprintln!("Error: --jobs expects a number");
                        process::exit(2);
                    });
            }
            "--help" | "-h" => {
                print_help();
                process::exit(0);
            }
            other => {
                eprintln!("Unknown argument: {}", other);
                print_help();
                process::exit(2);
            }
        }
    }
    opts
}

fn main() {
    let opts = parse_args();
    match opts.command {
        Some(Command::Run) => run_command(&opts),
        Some(Command::Report) => report_command(&opts),
        None => {
            print_help();
            process::exit(2);
        }
    }
}

fn run_command(opts: &Options) {
    if !opts.patina.is_file() {
        eprintln!(
            "Error: patina binary not found at {} (build with `cargo build --release`)",
            opts.patina.display()
        );
        process::exit(2);
    }

    let heap = patina_core::new_shared_heap();
    let universe = match corpus::discover(&opts.vendor, &heap) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(2);
        }
    };
    // Providers index over the full corpus so a filtered run still resolves
    // cross-package dependencies.
    let providers = corpus::providers(&universe);
    let selected: Vec<&corpus::Package> = universe
        .iter()
        .filter(|p| {
            opts.filter
                .as_ref()
                .is_none_or(|f| p.slug.contains(f.as_str()))
        })
        .collect();
    if selected.is_empty() {
        eprintln!("Error: no packages selected");
        process::exit(2);
    }

    let backend = if opts.tree_walker {
        "tree-walker"
    } else {
        "vm"
    };
    let config = RunConfig {
        patina: opts.patina.clone(),
        tree_walker: opts.tree_walker,
        timeout: opts.timeout,
        jobs: opts.jobs,
    };
    let results = run::run_corpus(&selected, &universe, &providers, &config);

    let exclusions = load_exclusions(opts, &heap);
    let rendered = report::render(&results, backend, &exclusions, opts.filter.is_none());

    // Both artifacts are written, not just the snapshot: the rendered matrix
    // is committed too, and printing it to stdout alone left it stale unless
    // the caller happened to redirect.
    //
    // A filtered run measures a subset, so it must not overwrite a canonical
    // artifact — that would silently shrink the committed baseline to whatever
    // was filtered. Naming a path is what authorizes writing it, per artifact,
    // so `--filter --results <file>` cannot take the report down with it.
    let subset = opts.filter.is_some();
    write_artifact(
        "results",
        opts.results_path.as_deref(),
        &opts.results_path(),
        subset,
        || report::to_sexp(&results, backend),
    );
    write_artifact(
        "report",
        opts.report_path.as_deref(),
        &opts.report_path(),
        subset,
        || rendered.clone(),
    );

    println!("{}", rendered);
}

/// Write one artifact, unless this is a subset run that did not name a path
/// for it.
fn write_artifact(
    what: &str,
    requested: Option<&std::path::Path>,
    path: &std::path::Path,
    subset: bool,
    contents: impl FnOnce() -> String,
) {
    if subset && requested.is_none() {
        eprintln!(
            "filtered run: {} left unchanged (pass --{} <file> to save a subset)",
            path.display(),
            what
        );
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(path, contents()) {
        Ok(()) => eprintln!("written: {}", path.display()),
        Err(e) => eprintln!("warning: could not write {}: {}", path.display(), e),
    }
}

fn report_command(opts: &Options) {
    let results_path = opts.results_path();
    let source = match std::fs::read_to_string(&results_path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {}: {}", results_path.display(), e);
            process::exit(2);
        }
    };
    let heap = patina_core::new_shared_heap();
    // The snapshot records which backend it measured; the CLI flag plays no
    // part in re-rendering.
    let exclusions = load_exclusions(opts, &heap);
    match report::from_sexp(&source, &heap) {
        // A snapshot is whatever it was written from; re-rendering cannot
        // know whether that was the whole corpus, and the committed one
        // always is.
        Ok((results, backend)) => {
            println!("{}", report::render(&results, &backend, &exclusions, true))
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(2);
        }
    }
}
