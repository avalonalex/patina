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
    eprintln!("  run      Execute the corpus, write the results snapshot, print the report");
    eprintln!("  report   Re-render the report from an existing results snapshot");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --patina <path>   Binary under test (default: target/release/patina)");
    eprintln!("  --vendor <dir>    Corpus directory (default: compat/vendor)");
    eprintln!("  --results <file>  Snapshot path (default: compat/reports/results.scm)");
    eprintln!("  --filter <substr> Only packages whose slug contains <substr>");
    eprintln!("  --tree-walker     Test the tree-walking backend instead of the VM");
    eprintln!("  --timeout <secs>  Per-package budget (default: 30)");
    eprintln!("  --jobs <n>        Parallel packages (default: available cores, max 8)");
}

struct Options {
    command: String,
    patina: PathBuf,
    vendor: PathBuf,
    results_path: PathBuf,
    filter: Option<String>,
    tree_walker: bool,
    timeout: Duration,
    jobs: usize,
}

fn parse_args() -> Options {
    let root = workspace_root();
    let mut opts = Options {
        command: String::new(),
        patina: root.join("target/release/patina"),
        vendor: root.join("compat/vendor"),
        results_path: root.join("compat/reports/results.scm"),
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
        let mut value = |flag: &str| -> String {
            iter.next().cloned().unwrap_or_else(|| {
                eprintln!("Error: {} requires a value", flag);
                process::exit(2);
            })
        };
        match arg.as_str() {
            "run" | "report" => opts.command = arg.clone(),
            "--patina" => opts.patina = PathBuf::from(value("--patina")),
            "--vendor" => opts.vendor = PathBuf::from(value("--vendor")),
            "--results" => opts.results_path = PathBuf::from(value("--results")),
            "--filter" => opts.filter = Some(value("--filter")),
            "--tree-walker" => opts.tree_walker = true,
            "--timeout" => {
                let secs: u64 = value("--timeout").parse().unwrap_or_else(|_| {
                    eprintln!("Error: --timeout expects seconds");
                    process::exit(2);
                });
                opts.timeout = Duration::from_secs(secs);
            }
            "--jobs" => {
                opts.jobs = value("--jobs").parse().unwrap_or_else(|_| {
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
    if opts.command.is_empty() {
        print_help();
        process::exit(2);
    }
    opts
}

fn main() {
    let opts = parse_args();
    let backend = if opts.tree_walker {
        "tree-walker"
    } else {
        "vm"
    };

    match opts.command.as_str() {
        "run" => {
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
            // Providers index over the full corpus so a filtered run still
            // resolves cross-package dependencies.
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

            let config = RunConfig {
                patina: opts.patina.clone(),
                tree_walker: opts.tree_walker,
                timeout: opts.timeout,
                jobs: opts.jobs,
            };
            let results = run::run_corpus(&selected, &universe, &providers, &config);

            let snapshot = report::to_sexp(&results, backend);
            if let Some(parent) = opts.results_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = std::fs::write(&opts.results_path, &snapshot) {
                eprintln!(
                    "warning: could not write {}: {}",
                    opts.results_path.display(),
                    e
                );
            } else {
                eprintln!("results written to {}", opts.results_path.display());
            }

            println!("{}", report::render(&results, backend));
        }
        "report" => {
            let source = match std::fs::read_to_string(&opts.results_path) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Error: {}: {}", opts.results_path.display(), e);
                    process::exit(2);
                }
            };
            let heap = patina_core::new_shared_heap();
            match report::from_sexp(&source, &heap) {
                Ok(results) => println!("{}", report::render(&results, backend)),
                Err(e) => {
                    eprintln!("Error: {}", e);
                    process::exit(2);
                }
            }
        }
        _ => unreachable!(),
    }
}
