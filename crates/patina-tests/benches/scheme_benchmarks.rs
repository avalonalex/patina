//! Scheme benchmarks for Patina
//!
//! Run with: cargo bench --package patina-tests
//!
//! These benchmarks are ported from ecraven/r7rs-benchmarks
//! (https://github.com/ecraven/r7rs-benchmarks) for compatibility
//! with the standard R7RS benchmark suite.
//!
//! Benchmark programs are in bench_programs/*.scm

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use patina_interpreter::TreeWalkInterpreter;
use std::path::PathBuf;

/// Get the path to bench_programs directory
fn bench_programs_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bench_programs")
}

/// Helper to create a fresh interpreter for each benchmark
fn make_interpreter() -> TreeWalkInterpreter {
    TreeWalkInterpreter::new_tree_walker()
}

/// Load a benchmark program file
fn load_program(interp: &TreeWalkInterpreter, filename: &str) {
    let path = bench_programs_dir().join(filename);
    let code = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));
    interp.eval_program(&code)
        .unwrap_or_else(|e| panic!("Failed to load {}: {:?}", filename, e));
}

/// Helper to evaluate Scheme code and return the result
fn eval(interp: &TreeWalkInterpreter, code: &str) -> String {
    match interp.eval_program(code) {
        Ok(val) => format!("{}", val),
        Err(e) => panic!("Evaluation error: {:?}", e),
    }
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
    group.bench_function("18_12_6", |b| {
        b.iter(|| eval(&interp, "(tak 18 12 6)"))
    });

    // Smaller size for quick iteration
    group.bench_function("12_8_4", |b| {
        b.iter(|| eval(&interp, "(tak 12 8 4)"))
    });

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

    // ack(3, n) - different n values
    for n in [6, 8, 9] {
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
    group.bench_function("single", |b| {
        b.iter(|| eval(&interp, "(deriv test-expr)"))
    });

    // Multiple iterations (like the actual benchmark)
    group.bench_function("1000_iter", |b| {
        b.iter(|| {
            eval(&interp, r#"
                (let loop ((i 1000))
                  (if (= i 0)
                      'done
                      (begin (deriv test-expr) (loop (- i 1)))))
            "#)
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

    // Different board sizes
    for n in [8, 10, 11] {
        group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
            b.iter(|| eval(&interp, &format!("(nqueens {})", n)))
        });
    }

    group.finish();
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
    group.bench_function("12_8_4", |b| {
        b.iter(|| eval(&interp, "(ctak 12 8 4)"))
    });

    // Even smaller for quick runs
    group.bench_function("8_4_2", |b| {
        b.iter(|| eval(&interp, "(ctak 8 4 2)"))
    });

    group.finish();
}

fn bench_dynamic_wind(c: &mut Criterion) {
    let mut group = c.benchmark_group("continuations/dynamic_wind");

    let interp = make_interpreter();

    // Simple dynamic-wind
    group.bench_function("simple", |b| {
        b.iter(|| {
            eval(&interp, r#"
                (dynamic-wind
                  (lambda () #f)
                  (lambda () 42)
                  (lambda () #f))
            "#)
        });
    });

    // Nested dynamic-wind (tests wind record cloning)
    interp.eval_program(r#"
        (define (wind-nest n)
          (if (= n 0)
              'done
              (dynamic-wind
                (lambda () #f)
                (lambda () (wind-nest (- n 1)))
                (lambda () #f))))
    "#).unwrap();

    group.bench_function("nested_10", |b| {
        b.iter(|| eval(&interp, "(wind-nest 10)"))
    });

    group.bench_function("nested_20", |b| {
        b.iter(|| eval(&interp, "(wind-nest 20)"))
    });

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
    interp.eval_program(r#"
        (define (cc-loop n acc)
          (if (= n 0)
              acc
              (call/cc (lambda (k)
                (cc-loop (- n 1) (+ acc 1))))))
    "#).unwrap();

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
    interp.eval_program(r#"
        (define (make-list n)
          (if (= n 0) '() (cons n (make-list (- n 1)))))
    "#).unwrap();

    group.bench_function("make_1000", |b| {
        b.iter(|| eval(&interp, "(length (make-list 1000))"))
    });

    // Prepare a test list
    interp.eval_program(r#"
        (define test-list (make-list 1000))
    "#).unwrap();

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
            eval(&interp, r#"
                (let ((l1 (make-list 500))
                      (l2 (make-list 500)))
                  (length (append l1 l2)))
            "#)
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
    interp.eval_program(r#"
        (define test-vec (make-vector 1000 42))
        (define (sum-vec v n acc)
          (if (= n 0)
              acc
              (sum-vec v (- n 1) (+ acc (vector-ref v (- n 1))))))
    "#).unwrap();

    group.bench_function("sum_1000", |b| {
        b.iter(|| eval(&interp, "(sum-vec test-vec 1000 0)"))
    });

    // Fill vector
    interp.eval_program(r#"
        (define fill-vec (make-vector 1000 0))
        (define (fill! v n)
          (if (= n 0)
              v
              (begin
                (vector-set! v (- n 1) n)
                (fill! v (- n 1)))))
    "#).unwrap();

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
    interp.eval_program(r#"
        (define (sum-to n acc)
          (if (= n 0) acc (sum-to (- n 1) (+ acc n))))
    "#).unwrap();

    group.bench_function("sum_10000", |b| {
        b.iter(|| eval(&interp, "(sum-to 10000 0)"))
    });

    // Factorial (tests bignum promotion)
    interp.eval_program(r#"
        (define (fact n acc)
          (if (= n 0) acc (fact (- n 1) (* acc n))))
    "#).unwrap();

    for n in [20, 50, 100] {
        group.bench_with_input(BenchmarkId::new("factorial", n), &n, |b, &n| {
            b.iter(|| eval(&interp, &format!("(fact {} 1)", n)))
        });
    }

    // Float arithmetic
    interp.eval_program(r#"
        (define (float-sum n acc)
          (if (= n 0) acc (float-sum (- n 1) (+ acc 1.5))))
    "#).unwrap();

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
    targets = bench_tak, bench_fib, bench_ack, bench_deriv, bench_primes, bench_nqueens, bench_sum
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
