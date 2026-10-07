//! `PATINA_GC_LOG` and the pause, MMU and K16 keys of `(gc-stats)` (#648):
//! what each collection cost and why it ran, pinned on both backends.
//!
//! The log is what the GC benchmark mode and the steady-state lane read
//! (#649, #652), so these tests pin what they rely on: one line per
//! collection, the count the heap reports; phases that add up to the pause;
//! a reason that says what brought the collection; and the high-water mark
//! of a deferral window naming the guard that opened it.

mod common;

use common::{BOTH_BACKENDS, patina_command};
use tempfile::TempDir;

/// Run `program` on `backend` with the GC variables cleared, `env` set and a
/// log asked for: what it printed, and the log's lines.
fn run(backend: &[&str], program: &str, env: &[(&str, &str)]) -> (String, Vec<String>) {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("program.scm"), program).unwrap();
    let log = dir.path().join("gc.csv");
    let mut args = backend.to_vec();
    args.push("program.scm");
    let mut command = patina_command(dir.path(), &args, &[]);
    for var in ["PATINA_GC", "PATINA_GC_STRESS", "PATINA_GC_ZEAL"] {
        command.env_remove(var);
    }
    for (name, value) in env {
        command.env(name, value);
    }
    let output = command
        .env("PATINA_GC_LOG", &log)
        .output()
        .expect("spawn patina");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    assert!(
        output.status.success(),
        "patina {args:?} failed\nstdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let lines = std::fs::read_to_string(&log)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect();
    (stdout, lines)
}

/// The value of `(cdr (assq 'key (gc-stats)))` a program wrote, one per
/// line, as `key value`.
fn reading<'a>(stdout: &'a str, key: &str) -> &'a str {
    stdout
        .lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix(' '))
        .unwrap_or_else(|| panic!("no {key} in {stdout:?}"))
}

/// Allocates past the byte trigger many times over, asks for a collection
/// with `(gc)`, then writes what `(gc-stats)` reports.
const CHURN: &str = "(import (scheme base) (scheme write) (patina debug))
(define (churn n acc)
  (cond ((= n 0) (length acc))
        ((= 0 (modulo n 1000)) (churn (- n 1) '()))
        (else (churn (- n 1) (cons (make-vector 8 n) acc)))))
(churn 300000 '())
(gc)
(for-each (lambda (key)
            (write key) (display \" \") (write (cdr (assq key (gc-stats)))) (newline))
          '(collections last-pause-us pause-max-us pause-total-us mmu-10ms
            resident-bytes cpu-us))
";

#[test]
fn the_log_has_a_line_per_collection_whose_phases_make_its_pause() {
    for backend in BOTH_BACKENDS {
        let (stdout, lines) = run(backend, CHURN, &[]);
        let collections: usize = reading(&stdout, "collections").parse().unwrap();
        assert!(collections >= 2, "{backend:?}: {stdout}");
        let header: Vec<&str> = lines[0].split(',').collect();
        assert_eq!(
            &header[..5],
            ["heap", "number", "start_us", "reason", "pause_us"]
        );
        assert_eq!(lines.len() - 1, collections, "{backend:?}: {lines:#?}");
        let column = |name: &str| header.iter().position(|h| *h == name).unwrap();
        let mut start = 0u64;
        let mut reasons = Vec::new();
        for (n, line) in lines[1..].iter().enumerate() {
            let fields: Vec<&str> = line.split(',').collect();
            assert_eq!(fields.len(), header.len(), "{line}");
            let number = |name: &str| fields[column(name)].parse::<u64>().unwrap();
            assert_eq!(number("number"), n as u64 + 1, "{line}");
            assert!(number("start_us") >= start, "{line}");
            start = number("start_us");
            // Each phase is rounded down to a microsecond on its own.
            let phases: u64 = [
                "roots_us", "mark_us", "weak_us", "prune_us", "sweep_us", "after_us",
            ]
            .iter()
            .map(|p| number(p))
            .sum();
            let pause = number("pause_us");
            assert!(phases <= pause && pause <= phases + 6, "{line}");
            reasons.push(fields[column("reason")].to_owned());
        }
        // The trigger brought the churn's collections, and `(gc)` the last.
        assert!(
            reasons.iter().any(|r| r == "bytes"),
            "{backend:?}: {reasons:?}"
        );
        assert_eq!(
            reasons.last().map(String::as_str),
            Some("call"),
            "{backend:?}"
        );
    }
}

#[test]
fn stress_names_its_collections() {
    for backend in BOTH_BACKENDS {
        let (_, lines) = run(backend, CHURN, &[("PATINA_GC_STRESS", "4096")]);
        let reasons: Vec<&str> = lines[1..]
            .iter()
            .map(|line| line.split(',').nth(3).unwrap())
            .collect();
        assert!(reasons.len() > 10, "{backend:?}: {reasons:?}");
        assert!(
            reasons.iter().all(|r| *r == "stress" || *r == "call"),
            "{backend:?}: {reasons:?}"
        );
    }
}

#[test]
fn the_pause_keys_count_every_collection() {
    for backend in BOTH_BACKENDS {
        let (stdout, _) = run(backend, CHURN, &[]);
        let micros = |key| reading(&stdout, key).parse::<u64>().unwrap();
        assert!(
            micros("last-pause-us") <= micros("pause-max-us"),
            "{stdout}"
        );
        assert!(
            micros("pause-max-us") <= micros("pause-total-us"),
            "{stdout}"
        );
        let mmu: f64 = reading(&stdout, "mmu-10ms").parse().unwrap();
        assert!((0.0..=1.0).contains(&mmu), "{stdout}");
        if cfg!(any(target_os = "macos", target_os = "linux")) {
            assert!(micros("resident-bytes") > 0, "{stdout}");
            assert!(micros("cpu-us") > 0, "{stdout}");
        }
    }
}

/// Loading libraries happens under a holder's guard, which defers every
/// collection: K16's second high-water mark is that window's bytes, named
/// by the guard that opened it.
#[test]
fn a_library_load_reports_its_deferral_window() {
    let program = "(import (scheme base) (scheme write) (patina debug) (scheme list) (scheme sort))
(for-each (lambda (key)
            (write key) (display \" \") (write (cdr (assq key (gc-stats)))) (newline))
          '(deferral-max-bytes deferral-site))
";
    for backend in BOTH_BACKENDS {
        let (stdout, _) = run(backend, program, &[]);
        let bytes: u64 = reading(&stdout, "deferral-max-bytes").parse().unwrap();
        assert!(bytes > 100_000, "{backend:?}: {stdout}");
        let site = reading(&stdout, "deferral-site");
        assert!(
            site.starts_with("\"crates/") && site.contains(".rs:"),
            "{backend:?}: {stdout}"
        );
    }
}
