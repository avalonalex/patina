//! The default collector counts bytes, not objects (#606).
//!
//! Its trigger used to fire after `max(65,536, 2 × live)` allocations, so an
//! object's size did not count: 500 vectors of 100,000 elements peaked at
//! 414 MB with no collection, and 20,000 VM continuations captured 1,000
//! frames deep at 3.2 GB with one. It now fires after `max(8 MiB, 2·L)` bytes,
//! each object charged its slot and its payload, a VM capture its snapshot,
//! and L, the bytes the last collection found live, counts the snapshots it
//! kept. A tree-walker closure is charged its own allocation and an estimate
//! of the frame it captures (#637), so that a loop of closures collects as
//! often as the memory they keep alive calls for. These run the shapes of
//! #606 and #637 through the CLI with no GC variable set, on the backends
//! they concern, and read what the heap reports in `gc-stats`.

mod common;

use common::{BOTH_BACKENDS, patina_command};
use tempfile::TempDir;

/// Before the workload: a collection that takes the garbage the libraries
/// left, which the default mode does not collect while they load, and the
/// counts after it.
const PRELUDE: &str = "(gc)
(define before (gc-stats))
";

/// The byte keys a program writes before and after its workload, in this
/// order.
const REPORT: &str = "(define after (gc-stats))
(define keys '(collections bytes-allocated bytes-reclaimed live-bytes committed-bytes))
(write (map (lambda (key) (cdr (assq key before))) keys))
(write (map (lambda (key) (cdr (assq key after))) keys))
(newline)
";

/// The adaptive interval's floor, `heap::gc::DEFAULT_MIN_BYTES`.
const FLOOR: u64 = 8 << 20;

/// What the workload did to the heap: each key's growth across it, so the
/// bootstrap's allocation, its garbage and its live set count against no
/// bound, however `lib/scheme` grows.
#[derive(Debug)]
struct Report {
    collections: u64,
    allocated: u64,
    reclaimed: u64,
    live: u64,
    committed: u64,
}

/// Run [`PRELUDE`], `workload` and [`REPORT`] on `backend`, in the default
/// GC mode, and parse what it wrote.
fn run(backend: &[&str], workload: &str) -> Report {
    let dir = TempDir::new().unwrap();
    let program = format!(
        "(import (scheme base) (scheme write) (patina debug))\n{PRELUDE}{workload}\n{REPORT}"
    );
    std::fs::write(dir.path().join("program.scm"), &program).unwrap();
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
    let counts: Vec<u64> = stdout
        .replace(['(', ')'], " ")
        .split_whitespace()
        .map(|n| {
            n.parse()
                .unwrap_or_else(|_| panic!("not a count in {stdout:?}"))
        })
        .collect();
    let [c0, a0, r0, l0, m0, c1, a1, r1, l1, m1] = counts[..] else {
        panic!("{backend:?}: want ten counts, got {stdout:?}")
    };
    Report {
        collections: c1 - c0,
        allocated: a1 - a0,
        reclaimed: r1 - r0,
        live: l1.saturating_sub(l0),
        committed: m1.saturating_sub(m0),
    }
}

/// #606's first program, at 200 vectors: 160 MB in 200 allocations, which an
/// object count never collected on. The bytes collect it about every 8 MiB,
/// and the heap grows by a few vectors' worth across it, not all of them.
#[test]
fn large_garbage_vectors_collect_by_bytes() {
    let workload = "(let loop ((i 0))
  (when (< i 200) (make-vector 100000 0) (loop (+ i 1))))";
    for backend in BOTH_BACKENDS {
        let r = run(backend, workload);
        assert!(r.allocated > 160_000_000, "{backend:?}: {r:?}");
        assert!(
            r.collections >= r.allocated / FLOOR / 2,
            "{backend:?}: {r:?}"
        );
        assert!(r.reclaimed > r.allocated * 9 / 10, "{backend:?}: {r:?}");
        assert!(r.committed < 4 * FLOOR, "{backend:?}: {r:?}");
    }
}

/// The deeper a VM capture, the more it copies, and the trigger is charged
/// what it copied: #606's second program, at 1,000 captures, allocates about
/// 150 MB and collects about every 8 MiB of it, where it used to collect once.
#[test]
fn deep_vm_captures_collect_by_bytes() {
    let workload = "(define (at-depth d thunk) (if (= d 0) (thunk) (+ 1 (at-depth (- d 1) thunk))))
(at-depth 1000
  (lambda ()
    (let loop ((i 0))
      (when (< i 1000) (call/cc (lambda (k) k)) (loop (+ i 1))))
    0))";
    let r = run(&[], workload);
    // About 150 KB a capture: 1,000 frames and their registers.
    assert!(r.allocated > 1000 * 100_000, "{r:?}");
    assert!(r.collections >= r.allocated / FLOOR / 2, "{r:?}");
    assert!(r.reclaimed > r.allocated * 9 / 10, "{r:?}");
    assert!(r.committed < 4 * FLOOR, "{r:?}");
}

/// Captures a program keeps are live bytes, which raise the interval: 600
/// kept captures, about 90 MB, collect three times, where a trigger that
/// left them out of L would collect at every 8 MiB of them, eleven times.
#[test]
fn kept_vm_captures_raise_the_interval() {
    let workload = "(define kept '())
(define (at-depth d thunk) (if (= d 0) (thunk) (+ 1 (at-depth (- d 1) thunk))))
(at-depth 1000
  (lambda ()
    (let loop ((i 0))
      (when (< i 600) (set! kept (cons (call/cc (lambda (k) k)) kept)) (loop (+ i 1))))
    0))";
    let r = run(&[], workload);
    assert!(r.allocated > 600 * 100_000, "{r:?}");
    assert!(r.collections >= 1, "{r:?}");
    // L counted what the last collection kept: most of what was captured
    // before it.
    assert!(r.live > r.allocated / 2, "{r:?}");
    assert!(r.collections * 2 < r.allocated / FLOOR, "{r:?}");
}

/// #637: every tree-walker closure keeps the frame it captures alive until a
/// sweep drops it, and is charged for it — an estimate of 612 bytes on a
/// 64-bit target, `CAPTURED_FRAME_BYTES` in `patina-core` — so a loop of
/// garbage closures collects about every 8 MiB of what they hold. Charged
/// their 72-byte slot alone, 100,000 of them allocated about 7 MB and did
/// not collect. The bound below is 500 bytes a closure, under the estimate.
/// The tree-walker only: a VM closure is a `VmClosure`, charged its free
/// variables, and keeps no frame alive.
#[test]
fn garbage_tree_walker_closures_collect_by_what_they_capture() {
    const CLOSURES: u64 = 100_000;
    let workload = "(define (make-adder n) (lambda (x) (+ x n)))
(let loop ((i 0) (sum 0))
  (if (< i 100000) (loop (+ i 1) (+ sum ((make-adder i) 1))) sum))";
    let r = run(&["--tree-walker"], workload);
    assert!(r.allocated > CLOSURES * 500, "{r:?}");
    assert!(r.collections >= CLOSURES * 500 / FLOOR, "{r:?}");
    assert!(r.reclaimed > r.allocated * 9 / 10, "{r:?}");
    assert!(r.committed < 4 * FLOOR, "{r:?}");
}
