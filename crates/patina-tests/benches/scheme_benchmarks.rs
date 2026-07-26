//! Scheme benchmarks for Patina
//!
//! Run with: cargo bench --package patina-tests
//!
//! Benchmarks the **VM backend** (the default backend) unless
//! `PATINA_BENCH_BACKEND=tree-walker` is set, which switches every group to
//! the tree-walker for cross-checking. Criterion IDs are the same for both,
//! so compare backends across separate runs, not within one report.
//!
//! Benchmark programs are in bench_programs/*.scm; the classic workloads
//! follow the shapes of the ecraven/r7rs-benchmarks suite
//! (https://github.com/ecraven/r7rs-benchmarks). nboyer/sboyer are vendored
//! from that suite verbatim (Public Domain per their headers).
//!
//! # VM baseline (2026-07-26, main @ 020fe70, Apple Silicon macOS, medians)
//!
//! Reference numbers for regression checking; re-run with
//! `cargo bench -p patina-tests` and compare against Criterion's own
//! stored baseline in target/criterion/.
//!
//! | Benchmark                        | Median     |
//! |----------------------------------|------------|
//! | r7rs/tak/18_12_6                 | 23.3 ms    |
//! | r7rs/fib/25                      | 75.2 ms    |
//! | r7rs/ack/3/6                     | 55.4 ms    |
//! | r7rs/deriv/1000_iter             | 36.7 ms    |
//! | r7rs/primes/1000                 | 9.34 ms    |
//! | r7rs/nqueens/10                  | 494 ms     |
//! | r7rs/sum/10000                   | 3.54 ms    |
//! | r7rs/nboyer/0                    | 423 ms     |
//! | r7rs/sboyer/0                    | 491 ms     |
//! | r7rs/ctak/12_8_4                 | 2.51 ms    |
//! | continuations/dynamic_wind/simple| 6.62 µs    |
//! | continuations/callcc/loop/200    | 139 µs     |
//! | data/lists/map_1000              | 2.19 ms    |
//! | data/vectors/sum_1000            | 457 µs     |
//! | numeric/sum_10000                | 2.87 ms    |
//! | numeric/float_sum_1000           | 334 µs     |

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use patina_interpreter::{Interpreter, TreeWalkInterpreter};
use patina_vm::VmBackend;
use std::path::PathBuf;

/// Get the path to bench_programs directory
fn bench_programs_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bench_programs")
}

/// Interpreter under benchmark: VM by default, tree-walker via
/// `PATINA_BENCH_BACKEND=tree-walker`
enum BenchInterp {
    Vm(Interpreter<VmBackend>),
    TreeWalker(TreeWalkInterpreter),
}

impl BenchInterp {
    /// Evaluate and return the raw result value. Formatting is deliberately
    /// not part of the benchmarked work; `Debug` on the `Copy` TaggedValue is
    /// enough to keep the optimizer from discarding the result.
    fn eval_program_or_panic(&self, code: &str, context: &str) -> String {
        match self {
            BenchInterp::Vm(i) => match i.eval_program(code) {
                Ok(val) => format!("{:?}", val),
                Err(e) => panic!("{}: {:?}", context, e),
            },
            BenchInterp::TreeWalker(i) => match i.eval_program(code) {
                Ok(val) => format!("{:?}", val),
                Err(e) => panic!("{}: {:?}", context, e),
            },
        }
    }
}

/// Helper to create a fresh interpreter for each benchmark
fn make_interpreter() -> BenchInterp {
    match std::env::var("PATINA_BENCH_BACKEND").as_deref() {
        Ok("tree-walker") => BenchInterp::TreeWalker(TreeWalkInterpreter::new_tree_walker()),
        _ => BenchInterp::Vm(Interpreter::new(VmBackend::new())),
    }
}

/// Load a benchmark program file
fn load_program(interp: &BenchInterp, filename: &str) {
    let path = bench_programs_dir().join(filename);
    let code = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));
    interp.eval_program_or_panic(&code, &format!("Failed to load {}", filename));
}

/// Helper to evaluate Scheme code and return the result
fn eval(interp: &BenchInterp, code: &str) -> String {
    interp.eval_program_or_panic(code, "Evaluation error")
}

// ============================================================================
// R7RS Benchmarks - Classic (Gabriel/Larceny)
// ============================================================================

fn bench_tak(c: &mut Criterion) {
    let mut group = c.benchmark_group("r7rs/tak");

    // Load the tak program once
    let interp = make_interpreter();
    load_program(&interp, "tak.scm");

    // Standard r7rs-benchmark size (but fewer iterations than the suite)
    group.bench_function("18_12_6", |b| b.iter(|| eval(&interp, "(tak 18 12 6)")));

    // Smaller size for quick iteration
    group.bench_function("12_8_4", |b| b.iter(|| eval(&interp, "(tak 12 8 4)")));

    group.finish();
}

fn bench_fib(c: &mut Criterion) {
    let mut group = c.benchmark_group("r7rs/fib");

    let interp = make_interpreter();
    load_program(&interp, "fib.scm");

    // Various sizes to see scaling
    for n in [15, 20, 25] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter(|| eval(&interp, &format!("(fib {})", n)))
        });
    }

    group.finish();
}

fn bench_ack(c: &mut Criterion) {
    let mut group = c.benchmark_group("r7rs/ack");

    let interp = make_interpreter();
    load_program(&interp, "ack.scm");

    // ack(3, n) - ack(3,8)=19s/sample and ack(3,9)=85s/sample are too slow for a baseline run
    for n in [4, 6] {
        group.bench_with_input(BenchmarkId::new("3", n), &n, |b, &n| {
            b.iter(|| eval(&interp, &format!("(ack 3 {})", n)))
        });
    }

    group.finish();
}

fn bench_deriv(c: &mut Criterion) {
    let mut group = c.benchmark_group("r7rs/deriv");

    let interp = make_interpreter();
    load_program(&interp, "deriv.scm");

    // Single derivation
    group.bench_function("single", |b| b.iter(|| eval(&interp, "(deriv test-expr)")));

    // Multiple iterations (like the actual benchmark)
    group.bench_function("1000_iter", |b| {
        b.iter(|| {
            eval(
                &interp,
                r#"
                (let loop ((i 1000))
                  (if (= i 0)
                      'done
                      (begin (deriv test-expr) (loop (- i 1)))))
            "#,
            )
        })
    });

    group.finish();
}

fn bench_primes(c: &mut Criterion) {
    let mut group = c.benchmark_group("r7rs/primes");

    let interp = make_interpreter();
    load_program(&interp, "primes.scm");

    // Different sieve sizes
    for n in [100, 500, 1000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter(|| eval(&interp, &format!("(length (primes<= {}))", n)))
        });
    }

    group.finish();
}

fn bench_nqueens(c: &mut Criterion) {
    let mut group = c.benchmark_group("r7rs/nqueens");

    let interp = make_interpreter();
    load_program(&interp, "nqueens.scm");

    // n=11 takes several seconds per sample; keep to 8 and 10 for a manageable baseline run
    for n in [8, 10] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter(|| eval(&interp, &format!("(nqueens {})", n)))
        });
    }

    group.finish();
}

/// Shared body for the two Boyer variants (vendored from
/// ecraven/r7rs-benchmarks, Public Domain — see the .scm headers).
/// CONS-intensive logic-programming workload; the classic GC stressor.
/// n=0 must produce exactly 95024 rewrites — asserted before benchmarking
/// so a semantic regression can't masquerade as a speedup.
fn bench_boyer(c: &mut Criterion, name: &str, file: &str, entry: &str) {
    let mut group = c.benchmark_group(format!("r7rs/{}", name));

    let interp = make_interpreter();
    load_program(&interp, file);

    assert_eq!(
        eval(&interp, &format!("(= ({} 0) 95024)", entry)),
        eval(&interp, "#t"),
        "{} n=0 must rewrite exactly 95024 times",
        name
    );

    // n=0 is the standard smallest scaling parameter; larger sizes grow
    // ~6x per step and are too slow for a Criterion baseline.
    group.bench_function("0", |b| b.iter(|| eval(&interp, &format!("({} 0)", entry))));

    group.finish();
}

fn bench_nboyer(c: &mut Criterion) {
    bench_boyer(c, "nboyer", "nboyer.scm", "nboyer-run");
}

fn bench_sboyer(c: &mut Criterion) {
    bench_boyer(c, "sboyer", "sboyer.scm", "sboyer-run");
}

fn bench_sum(c: &mut Criterion) {
    let mut group = c.benchmark_group("r7rs/sum");

    let interp = make_interpreter();
    load_program(&interp, "sum.scm");

    // Different sizes
    for n in [1000, 5000, 10000] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter(|| eval(&interp, &format!("(run {})", n)))
        });
    }

    group.finish();
}

// ============================================================================
// Continuation Benchmarks (CPS evaluator stress tests)
// ============================================================================

fn bench_ctak(c: &mut Criterion) {
    let mut group = c.benchmark_group("r7rs/ctak");

    let interp = make_interpreter();
    load_program(&interp, "ctak.scm");

    // ctak is MUCH slower than tak due to continuation capture
    // Use smaller parameters
    group.bench_function("12_8_4", |b| b.iter(|| eval(&interp, "(ctak 12 8 4)")));

    // Even smaller for quick runs
    group.bench_function("8_4_2", |b| b.iter(|| eval(&interp, "(ctak 8 4 2)")));

    group.finish();
}

fn bench_dynamic_wind(c: &mut Criterion) {
    let mut group = c.benchmark_group("continuations/dynamic_wind");

    let interp = make_interpreter();

    // Simple dynamic-wind
    group.bench_function("simple", |b| {
        b.iter(|| {
            eval(
                &interp,
                r#"
                (dynamic-wind
                  (lambda () #f)
                  (lambda () 42)
                  (lambda () #f))
            "#,
            )
        });
    });

    // Nested dynamic-wind (tests wind record cloning)
    eval(
        &interp,
        r#"
        (define (wind-nest n)
          (if (= n 0)
              'done
              (dynamic-wind
                (lambda () #f)
                (lambda () (wind-nest (- n 1)))
                (lambda () #f))))
    "#,
    );

    group.bench_function("nested_10", |b| b.iter(|| eval(&interp, "(wind-nest 10)")));

    group.bench_function("nested_20", |b| b.iter(|| eval(&interp, "(wind-nest 20)")));

    group.finish();
}

fn bench_callcc(c: &mut Criterion) {
    let mut group = c.benchmark_group("continuations/callcc");

    let interp = make_interpreter();

    // Simple call/cc - baseline
    group.bench_function("simple", |b| {
        b.iter(|| eval(&interp, "(call/cc (lambda (k) (k 42)))"))
    });

    // call/cc in a loop - tests repeated capture/invoke
    eval(
        &interp,
        r#"
        (define (cc-loop n acc)
          (if (= n 0)
              acc
              (call/cc (lambda (k)
                (cc-loop (- n 1) (+ acc 1))))))
    "#,
    );

    for n in [50, 100, 200] {
        group.bench_with_input(BenchmarkId::new("loop", n), &n, |b, &n| {
            b.iter(|| eval(&interp, &format!("(cc-loop {} 0)", n)))
        });
    }

    group.finish();
}

// ============================================================================
// Data Structure Benchmarks
// ============================================================================

fn bench_lists(c: &mut Criterion) {
    let mut group = c.benchmark_group("data/lists");

    let interp = make_interpreter();

    // Build a list recursively
    eval(
        &interp,
        r#"
        (define (make-list n)
          (if (= n 0) '() (cons n (make-list (- n 1)))))
    "#,
    );

    group.bench_function("make_1000", |b| {
        b.iter(|| eval(&interp, "(length (make-list 1000))"))
    });

    // Prepare a test list
    eval(
        &interp,
        r#"
        (define test-list (make-list 1000))
    "#,
    );

    // Reverse
    group.bench_function("reverse_1000", |b| {
        b.iter(|| eval(&interp, "(length (reverse test-list))"))
    });

    // Map
    group.bench_function("map_1000", |b| {
        b.iter(|| eval(&interp, "(length (map (lambda (x) (* x 2)) test-list))"))
    });

    // Append
    group.bench_function("append_500_500", |b| {
        b.iter(|| {
            eval(
                &interp,
                r#"
                (let ((l1 (make-list 500))
                      (l2 (make-list 500)))
                  (length (append l1 l2)))
            "#,
            )
        })
    });

    group.finish();
}

fn bench_vectors(c: &mut Criterion) {
    let mut group = c.benchmark_group("data/vectors");

    let interp = make_interpreter();

    // Create vector
    group.bench_function("make_1000", |b| {
        b.iter(|| eval(&interp, "(vector-length (make-vector 1000 0))"))
    });

    // Sum vector elements
    eval(
        &interp,
        r#"
        (define test-vec (make-vector 1000 42))
        (define (sum-vec v n acc)
          (if (= n 0)
              acc
              (sum-vec v (- n 1) (+ acc (vector-ref v (- n 1))))))
    "#,
    );

    group.bench_function("sum_1000", |b| {
        b.iter(|| eval(&interp, "(sum-vec test-vec 1000 0)"))
    });

    // Fill vector
    eval(
        &interp,
        r#"
        (define fill-vec (make-vector 1000 0))
        (define (fill! v n)
          (if (= n 0)
              v
              (begin
                (vector-set! v (- n 1) n)
                (fill! v (- n 1)))))
    "#,
    );

    group.bench_function("fill_1000", |b| {
        b.iter(|| eval(&interp, "(begin (fill! fill-vec 1000) 'done)"))
    });

    group.finish();
}

// ============================================================================
// Numeric Benchmarks
// ============================================================================

fn bench_numeric(c: &mut Criterion) {
    let mut group = c.benchmark_group("numeric");

    let interp = make_interpreter();

    // Tail-recursive sum
    eval(
        &interp,
        r#"
        (define (sum-to n acc)
          (if (= n 0) acc (sum-to (- n 1) (+ acc n))))
    "#,
    );

    group.bench_function("sum_10000", |b| {
        b.iter(|| eval(&interp, "(sum-to 10000 0)"))
    });

    // Factorial (tests bignum promotion)
    eval(
        &interp,
        r#"
        (define (fact n acc)
          (if (= n 0) acc (fact (- n 1) (* acc n))))
    "#,
    );

    for n in [20, 50, 100] {
        group.bench_with_input(BenchmarkId::new("factorial", n), &n, |b, &n| {
            b.iter(|| eval(&interp, &format!("(fact {} 1)", n)))
        });
    }

    // Float arithmetic
    eval(
        &interp,
        r#"
        (define (float-sum n acc)
          (if (= n 0) acc (float-sum (- n 1) (+ acc 1.5))))
    "#,
    );

    group.bench_function("float_sum_1000", |b| {
        b.iter(|| eval(&interp, "(float-sum 1000 0.0)"))
    });

    group.finish();
}

// ============================================================================
// Criterion Setup
// ============================================================================

criterion_group!(
    name = r7rs_benches;
    config = Criterion::default();
    targets = bench_tak, bench_fib, bench_ack, bench_deriv, bench_primes, bench_nqueens, bench_sum,
        bench_nboyer, bench_sboyer
);

criterion_group!(
    name = continuation_benches;
    config = Criterion::default();
    targets = bench_ctak, bench_dynamic_wind, bench_callcc
);

criterion_group!(
    name = data_benches;
    config = Criterion::default();
    targets = bench_lists, bench_vectors
);

criterion_group!(
    name = numeric_benches;
    config = Criterion::default();
    targets = bench_numeric
);

criterion_main!(
    r7rs_benches,
    continuation_benches,
    data_benches,
    numeric_benches,
);
