//! A namespace charges the collector for its tables (#615).
//!
//! `(environment '(scheme base))` builds an environment of 267 imports whose
//! tables, about 40 KiB, are Rust allocations; the heap saw one specifier
//! slot. A loop of them reached 2.9 GiB before its first collections, where
//! chibi stays under 9 MiB. A namespace now charges its tables to the heap's
//! external bytes, once when it is made and again as they grow, and gives
//! them back when it drops, so the byte trigger (#606) collects such a loop
//! about every 8 MiB of namespaces, and L, which counts the external bytes
//! held, raises the interval for a program that keeps them.
//!
//! These run the issue's shapes through the CLI in the default GC mode, on
//! both backends, and read what the heap reports in `gc-stats`: counts, not
//! resident memory, which is what the issue measured and what the external
//! bytes stand in for.

mod common;

use common::{BOTH_BACKENDS, patina_command};
use tempfile::TempDir;

/// The adaptive interval's floor, `heap::gc::DEFAULT_MIN_BYTES`.
const FLOOR: u64 = 8 << 20;

const IMPORTS: &str = "(import (scheme base) (scheme write) (scheme eval) (scheme repl)
        (only (scheme r5rs) scheme-report-environment null-environment)
        (patina debug))
(define (stat key) (cdr (assq key (gc-stats))))
";

/// Run `program` after [`IMPORTS`] on `backend`, in the default GC mode, and
/// parse the list of counts it writes.
fn run(backend: &[&str], program: &str) -> Vec<i64> {
    let dir = TempDir::new().unwrap();
    std::fs::write(
        dir.path().join("program.scm"),
        format!("{IMPORTS}{program}"),
    )
    .unwrap();
    let mut args = backend.to_vec();
    args.push("program.scm");
    let output = patina_command(dir.path(), &args, &[])
        .env_remove("PATINA_GC")
        .env_remove("PATINA_GC_STRESS")
        .env_remove("PATINA_GC_ZEAL")
        .output()
        .expect("spawn patina");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "patina {args:?} failed\nstdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    stdout
        .replace(['(', ')'], " ")
        .split_whitespace()
        .map(|n| {
            n.parse()
                .unwrap_or_else(|_| panic!("not a count in {stdout:?}"))
        })
        .collect()
}

/// What a loop of `n` evaluations of `form` did, from a collection before it
/// to one after it.
#[derive(Debug)]
struct Loop {
    collections: u64,
    allocated: u64,
    /// The most external bytes held above the start, sampled every 16
    /// iterations: the dead namespaces waiting for a collection.
    peak: u64,
    /// External bytes held after a collection that followed the loop, less
    /// those held before it.
    left: i64,
}

/// Every global the measurement uses is defined, and the loop expanded,
/// before the first count is read: a definition can grow the global
/// environment's tables, and a namespace charges the names defined since its
/// last growth when a table next grows (`NamespaceCharge`), so a definition
/// between the two reads of `external-bytes` could charge the global
/// environment there and leave `left` above zero with nothing wrong.
fn run_loop(backend: &[&str], form: &str, n: usize) -> Loop {
    let program = format!(
        "(define c0 0) (define a0 0) (define e0 0) (define peak 0) (define c1 0) (define a1 0)
(define (run)
  (let loop ((i 0))
    (when (< i {n})
      {form}
      (when (= 0 (modulo i 16))
        (set! peak (max peak (- (stat 'external-bytes) e0))))
      (loop (+ i 1)))))
(gc)
(set! c0 (stat 'collections))
(set! a0 (stat 'bytes-allocated))
(set! e0 (stat 'external-bytes))
(run)
(set! c1 (stat 'collections))
(set! a1 (stat 'bytes-allocated))
(gc)
(write (list (- c1 c0) (- a1 a0) peak (- (stat 'external-bytes) e0)))
(newline)"
    );
    let [collections, allocated, peak, left] = run(backend, &program)[..] else {
        panic!("{backend:?}: want four counts")
    };
    Loop {
        collections: collections as u64,
        allocated: allocated as u64,
        peak: peak as u64,
        left,
    }
}

/// The issue's loop, and its three neighbours: each discarded namespace is
/// charged, so the loop collects about every 8 MiB of them, and what it holds
/// between collections is bounded by the interval, not by the number of
/// calls. The bound is the same at four times the calls, which is the
/// issue's 80,000-against-320,000 comparison in counts. After the loop and a
/// collection the account is back where it started: every namespace that
/// died gave its bytes back.
#[test]
fn discarded_namespaces_collect_by_their_tables() {
    // Each about 32 MiB of namespaces: 40 KiB, 40 KiB, 20 KiB and 3 KiB a call.
    let loops = [
        ("(environment '(scheme base))", 800),
        (
            "(let ((e (environment '(scheme base)))) (eval '(+ 1 2) e))",
            800,
        ),
        ("(scheme-report-environment 5)", 1_600),
        ("(null-environment 5)", 11_000),
    ];
    for backend in BOTH_BACKENDS {
        for (form, n) in loops {
            for n in [n, 4 * n] {
                let r = run_loop(backend, form, n);
                let what = format!("{backend:?} {n} x {form}: {r:?}");
                assert!(r.allocated > 3 * FLOOR, "{what}");
                assert!(r.collections >= r.allocated / FLOOR / 2, "{what}");
                assert!(r.peak < 2 * FLOOR, "{what}");
                assert_eq!(r.left, 0, "{what}");
            }
        }
    }
}

/// `interaction-environment` answers one specifier for the one global
/// environment, which is charged once: a loop of it allocates nothing, so it
/// collects no more often than a loop of `(cons 1 2)`, and holds no more.
/// The issue's 1,000,000 calls, so that the `cons` loop, 16 MB of pairs,
/// collects and the comparison can fail: a specifier a call, 72 MB of them,
/// collected eight times.
#[test]
fn the_interaction_environment_is_charged_once() {
    let program = "(define (measure thunk)
  (gc)
  (let ((c0 (stat 'collections)) (a0 (stat 'bytes-allocated)) (e0 (stat 'external-bytes)))
    (let loop ((i 0))
      (when (< i 1000000) (thunk) (loop (+ i 1))))
    (list (- (stat 'collections) c0) (- (stat 'bytes-allocated) a0)
          (- (stat 'external-bytes) e0))))
(write (list (if (eq? (interaction-environment) (interaction-environment)) 1 0)
             (measure (lambda () (interaction-environment)))
             (measure (lambda () (cons 1 2)))))
(newline)";
    for backend in BOTH_BACKENDS {
        let counts = run(backend, program);
        let [
            same,
            ie_collections,
            ie_allocated,
            ie_external,
            cons_collections,
            cons_allocated,
            _,
        ] = counts[..]
        else {
            panic!("{backend:?}: want seven counts, got {counts:?}")
        };
        let what = format!("{backend:?}: {counts:?}");
        assert_eq!(same, 1, "{what}");
        assert!(cons_collections >= 1, "{what}");
        assert!(ie_collections <= cons_collections, "{what}");
        assert!(ie_allocated <= cons_allocated, "{what}");
        assert_eq!(ie_external, 0, "{what}");
    }
}

/// Namespaces a program keeps are live bytes, which raise the interval: 2,000
/// kept environments, about 80 MB, collect three times, where a trigger that
/// left their tables out of L would collect at every 8 MiB of them, nine
/// times.
#[test]
fn kept_namespaces_raise_the_interval() {
    let program = "(gc)
(define c0 (stat 'collections))
(define a0 (stat 'bytes-allocated))
(define kept '())
(let loop ((i 0))
  (when (< i 2000)
    (set! kept (cons (environment '(scheme base)) kept))
    (loop (+ i 1))))
(write (list (- (stat 'collections) c0) (- (stat 'bytes-allocated) a0) (stat 'live-bytes)))
(newline)";
    for backend in BOTH_BACKENDS {
        let counts = run(backend, program);
        let [collections, allocated, live] = counts[..] else {
            panic!("{backend:?}: want three counts, got {counts:?}")
        };
        let what = format!("{backend:?}: {counts:?}");
        assert!(allocated as u64 > 2000 * 30_000, "{what}");
        assert!(collections >= 1, "{what}");
        // L counted the namespaces the last collection kept: most of what was
        // made before it.
        assert!(live > allocated / 2, "{what}");
        assert!(
            (collections as u64) * 2 < allocated as u64 / FLOOR,
            "{what}"
        );
    }
}

/// A namespace charges for its tables as they grow after it is made: the
/// global environment, defined into under `eval`, is charged each slot and
/// its name. Frames are not namespaces, and a program that only calls and
/// binds charges nothing: read at the bottom of a recursion, where the
/// tree-walker holds 200 frames, each a call's or a `let`'s, that the
/// pending `(+ n …)` needs.
#[test]
fn a_namespace_charges_its_growth_and_a_frame_nothing() {
    // Each measurement is a procedure, expanded before it runs, so that what
    // expanding a form installs in the global environment is not counted.
    let program = "(define frame-peak 0)
(define (deep n e0)
  (if (= n 0)
      (begin (set! frame-peak (max frame-peak (- (stat 'external-bytes) e0))) 0)
      (+ n (let ((m (- n 1))) (deep m e0)))))
(define (frames)
  (let ((e0 (stat 'external-bytes)))
    (let loop ((i 0)) (when (< i 1000) (deep 100 e0) (loop (+ i 1))))
    frame-peak))
(define (globals)
  (let ((e0 (stat 'external-bytes)))
    (let loop ((i 0))
      (when (< i 2000)
        (eval (list 'define (string->symbol (string-append \"g\" (number->string i))) i)
              (interaction-environment))
        (loop (+ i 1))))
    (- (stat 'external-bytes) e0)))
(define charged (list (frames) (globals)))
(write charged)
(newline)";
    for backend in BOTH_BACKENDS {
        let counts = run(backend, program);
        let [frames, globals] = counts[..] else {
            panic!("{backend:?}: want two counts, got {counts:?}")
        };
        let what = format!("{backend:?}: {counts:?}");
        assert_eq!(frames, 0, "{what}");
        // A slot of 24 bytes and a name of at least 17, each.
        assert!(globals >= 2000 * 41, "{what}");
    }
}
