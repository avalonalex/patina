//! `PATINA_GC_ZEAL=entry` collects at every outermost safe point, allocation
//! or not, on both backends (#625).
//!
//! Stress counts allocations: `PATINA_GC_STRESS=1` raises the pending flag
//! after each one, so a stretch of instructions that allocates nothing never
//! collects under it. Zeal's threshold is 0, so the collection that lowers
//! the flag raises it again, and the next safe point collects too. A zeal
//! mode that had decayed into stress, a threshold of 1 rather than 0, would
//! still collect a great deal on any ordinary program; the control here is a
//! loop that allocates nothing, which only zeal collects in.
//!
//! The zeal lane (`scripts/run_gc_zeal.sh`) checks that each of its runs
//! collected, which a decayed mode, or a binary that ignored the variable,
//! would pass. So it runs this loop on its own binary before it starts, with
//! no GC variable as the control in place of stress 1; keep `SPIN` and the
//! script's probe the same program.

mod common;

use common::{BOTH_BACKENDS, patina_command};
use tempfile::TempDir;

/// Iterations of a loop that allocates nothing: fixnum arithmetic and a self
/// tail call. Each iteration runs several instructions, or trampoline steps,
/// and so passes several safe points.
const ITERATIONS: u64 = 1000;

/// Write the collections the loop ran through, measured by `gc-stats` before
/// and after it.
const SPIN: &str = "(import (scheme base) (scheme write) (patina debug))
(define (spin n) (if (> n 0) (spin (- n 1))))
(define (collections) (cdr (assq 'collections (gc-stats))))
(define before (collections))
(spin 1000)
(define after (collections))
(write (- after before))
(newline)
";

/// The collections the loop ran through under `mode`, on the backend that
/// `backend` selects. Every GC variable is cleared first, since zeal wins
/// over stress and stress over `PATINA_GC=0`.
fn collections_during_the_loop(backend: &[&str], mode: (&str, &str)) -> u64 {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("spin.scm"), SPIN).unwrap();
    let mut args = backend.to_vec();
    args.push("spin.scm");
    let output = patina_command(dir.path(), &args, &[])
        .env_remove("PATINA_GC")
        .env_remove("PATINA_GC_STRESS")
        .env_remove("PATINA_GC_ZEAL")
        .env(mode.0, mode.1)
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
    stdout
        .trim()
        .parse()
        .unwrap_or_else(|_| panic!("not a count: {stdout:?}"))
}

#[test]
fn zeal_collects_in_a_loop_that_allocates_nothing() {
    for backend in BOTH_BACKENDS {
        let zeal = collections_during_the_loop(backend, ("PATINA_GC_ZEAL", "entry"));
        assert!(
            zeal >= ITERATIONS,
            "{backend:?}: {zeal} collections under zeal across {ITERATIONS} iterations"
        );
        // The loop really allocates nothing: stress at its densest collects
        // only for the `gc-stats` calls around it.
        let stress = collections_during_the_loop(backend, ("PATINA_GC_STRESS", "1"));
        assert!(
            stress < ITERATIONS / 10,
            "{backend:?}: {stress} collections at stress 1, so the loop allocates"
        );
    }
}

/// A zeal mode today's collector does not have is refused, rather than run
/// as a torture lane that tortures nothing.
#[test]
fn an_unknown_zeal_mode_is_refused() {
    let dir = TempDir::new().unwrap();
    for backend in BOTH_BACKENDS {
        let mut args = backend.to_vec();
        args.extend(["-p", "(+ 1 2)"]);
        let output = patina_command(dir.path(), &args, &[("PATINA_GC_ZEAL", "major")])
            .output()
            .expect("spawn patina");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{backend:?} accepted zeal=major");
        assert!(
            stderr.contains("PATINA_GC_ZEAL=major") && stderr.contains("`entry`"),
            "{backend:?}: {stderr}"
        );
    }
}
