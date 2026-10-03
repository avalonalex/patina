//! `PATINA_GC_COUNT_DIR` (#626): a process writes how many collections it
//! ran, which the stress lanes read to fail a run that did not collect
//! (`scripts/run_gc_stress_tests.sh`, `scripts/run_larceny_gc_stress.sh`).
//!
//! The lanes trust three things about the record, pinned here on both
//! backends: its count is the collections the process ran, the count the
//! heap reports through `gc-stats`, although the CLI leaves through
//! `process::exit` and so the record cannot be written at the end; it names
//! the mode the environment selected; and a process that never collected
//! still leaves one, with a count of zero, since a lane reads a missing
//! record as a process the variable never reached.

mod common;

use common::{BOTH_BACKENDS, patina_command};
use tempfile::TempDir;

/// Allocates enough to collect many times at stress 16, then writes the
/// collections the heap has run.
const PROGRAM: &str = "(import (scheme base) (scheme write) (patina debug))
(define (build n acc) (if (= n 0) acc (build (- n 1) (cons n acc))))
(define kept (build 2000 '()))
(write (cdr (assq 'collections (gc-stats))))
(newline)
";

/// Run `PROGRAM` on `backend` under `mode` with a record asked for, and
/// return what it printed and the records it left.
fn run(backend: &[&str], mode: (&str, &str)) -> (u64, Vec<String>) {
    let dir = TempDir::new().unwrap();
    let records = dir.path().join("records");
    std::fs::create_dir(&records).unwrap();
    std::fs::write(dir.path().join("program.scm"), PROGRAM).unwrap();
    let mut args = backend.to_vec();
    args.push("program.scm");
    let output = patina_command(dir.path(), &args, &[])
        .env_remove("PATINA_GC")
        .env_remove("PATINA_GC_STRESS")
        .env_remove("PATINA_GC_ZEAL")
        .env(mode.0, mode.1)
        .env("PATINA_GC_COUNT_DIR", &records)
        .output()
        .expect("spawn patina");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "patina {args:?} under {}={} failed\nstdout: {stdout}\nstderr: {}",
        mode.0,
        mode.1,
        String::from_utf8_lossy(&output.stderr)
    );
    let printed = stdout
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("not a count: {stdout:?}"));
    let records = std::fs::read_dir(&records)
        .unwrap()
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .collect();
    (printed, records)
}

/// The one record's mode and count.
fn the_record(backend: &[&str], records: &[String]) -> (String, u64) {
    let [record] = records else {
        panic!("{backend:?}: want one record, got {records:?}");
    };
    let field = |name: &str| {
        record
            .split_whitespace()
            .find_map(|f| f.strip_prefix(name))
            .unwrap_or_else(|| panic!("{backend:?}: no {name} in {record:?}"))
            .to_string()
    };
    assert!(record.ends_with('\n'), "{backend:?}: {record:?}");
    let collections = field("collections=")
        .parse()
        .unwrap_or_else(|_| panic!("{backend:?}: not a count in {record:?}"));
    (field("env="), collections)
}

#[test]
fn the_record_counts_the_collections_the_heap_ran() {
    for backend in BOTH_BACKENDS {
        let (printed, records) = run(backend, ("PATINA_GC_STRESS", "16"));
        let (mode, collections) = the_record(backend, &records);
        assert_eq!(mode, "PATINA_GC_STRESS=16", "{backend:?}");
        // The program writes the count before its last form returns, and the
        // runs between that and exit may collect again.
        assert!(
            collections >= printed && printed >= 100,
            "{backend:?}: the record says {collections}, gc-stats said {printed}"
        );
    }
}

#[test]
fn a_process_that_never_collects_still_leaves_a_record() {
    for backend in BOTH_BACKENDS {
        let (printed, records) = run(backend, ("PATINA_GC", "0"));
        assert_eq!(printed, 0, "{backend:?}: collected with PATINA_GC=0");
        assert_eq!(
            the_record(backend, &records),
            ("PATINA_GC=0".to_string(), 0),
            "{backend:?}"
        );
    }
}
