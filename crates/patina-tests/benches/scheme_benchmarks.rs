//! Correctness-checked, backend-qualified measurements. See docs/VM_TESTING.md
//! for timing boundaries and scripts/run_benchmarks.sh for reproducible reports.

// The bare-value `eval_*` forms (#605), deprecated until stage 5e removes them.
#![allow(deprecated)]

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use patina_core::{CoreExpr, Environment, TaggedValue};
use patina_frontend::{Desugarer, Parser};
use patina_interpreter::{Interpreter, TreeWalkInterpreter};
use patina_ir::CpsTransformer;
use patina_runtime::Backend;
use patina_vm::{VmBackend, compiler::compile_with_qq_resolving};
use serde::Deserialize;
use serde_json::json;
use std::{hint::black_box, io::Write, path::PathBuf, rc::Rc, time::Duration};

#[derive(Deserialize)]
struct Workload {
    id: String,
    file: Option<String>,
    setup: String,
    expression: String,
    expected: String,
    check: String,
    criterion: bool,
}

fn backend() -> String {
    match std::env::var("PATINA_BENCH_BACKEND").as_deref() {
        Err(std::env::VarError::NotPresent) | Ok("vm") => "vm".into(),
        Ok("tree-walker") => "tree-walker".into(),
        other => panic!("invalid PATINA_BENCH_BACKEND: {other:?}; use vm or tree-walker"),
    }
}

#[allow(clippy::large_enum_variant)]
enum BenchInterp {
    Vm(Interpreter<VmBackend>),
    TreeWalker(TreeWalkInterpreter),
}

impl BenchInterp {
    fn eval(&self, code: &str) -> TaggedValue {
        match self {
            Self::Vm(i) => i
                .eval_program(code)
                .unwrap_or_else(|e| panic!("{code}: {e}")),
            Self::TreeWalker(i) => i
                .eval_program(code)
                .unwrap_or_else(|e| panic!("{code}: {e}")),
        }
    }

    fn env(&self) -> &Rc<Environment> {
        match self {
            Self::Vm(i) => i.backend().global_env(),
            Self::TreeWalker(i) => i.backend().global_env(),
        }
    }

    fn check(&self, expression: &str, expected: &str) {
        assert_eq!(
            self.eval(&format!("(equal? {expression} {expected})")),
            TaggedValue::TRUE,
            "incorrect benchmark answer: {expression}, expected {expected}"
        );
    }
}

fn make_interpreter() -> BenchInterp {
    if backend() == "tree-walker" {
        let i = TreeWalkInterpreter::new_tree_walker();
        assert!(i.backend().bootstrap_error().is_none(), "bootstrap failed");
        BenchInterp::TreeWalker(i)
    } else {
        let i = Interpreter::new(VmBackend::new());
        assert!(i.backend().bootstrap_error().is_none(), "bootstrap failed");
        BenchInterp::Vm(i)
    }
}

// The runner matches this ledger against fresh Criterion samples. Write only
// after a selected benchmark completes; a failed/panicking run is never a report.
fn checked(id: &str, parameters: serde_json::Value) {
    if let Some(path) = std::env::var_os("PATINA_BENCH_CHECKS") {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        writeln!(
            file,
            "{}",
            json!({"id": id, "correctness": "passed", "parameters": parameters})
        )
        .unwrap();
    }
}

fn end_to_end(c: &mut Criterion) {
    let cases: Vec<Workload> =
        serde_json::from_str(include_str!("../bench_programs/workloads.json")).unwrap();
    for case in cases.into_iter().filter(|case| case.criterion) {
        let id = format!("{}/end_to_end/{}", backend(), case.id);
        let mut prepared = None;
        c.bench_function(&id, |b| {
            let interp = prepared.get_or_insert_with(|| {
                let interp = make_interpreter();
                if let Some(file) = &case.file {
                    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join("bench_programs")
                        .join(file);
                    interp.eval(&std::fs::read_to_string(path).unwrap());
                }
                interp.eval(&case.setup);
                interp.check(&case.expression, &case.expected);
                interp.check(&case.check, "#t");
                interp
            });
            // Source already exists; eval_program includes reading, expansion,
            // lowering, execution and normal GC. No Debug/String conversion.
            b.iter(|| black_box(interp.eval(black_box(&case.expression))));
        });
        if prepared.is_some() {
            checked(
                &id,
                json!({"expression": case.expression, "expected": case.expected, "extra_check": case.check}),
            );
        }
    }
}

const SUM: &str = "(let loop ((n 100) (acc 0)) (if (= n 0) acc (loop (- n 1) (+ acc n))))";
const ALLOC: &str =
    "(let loop ((n 256) (xs '())) (if (= n 0) (length xs) (loop (- n 1) (cons n xs))))";

fn expand(interp: &BenchInterp) -> CoreExpr {
    let heap = interp.env().heap();
    let mut parser = Parser::new_with_heap(black_box(SUM), heap.clone()).unwrap();
    let datum = parser.parse().unwrap();
    Desugarer::with_env(interp.env().clone())
        .desugar_tagged(datum, heap)
        .unwrap()
}

fn heap_counters(interp: &BenchInterp) -> serde_json::Value {
    let s = interp.env().heap().borrow().stats();
    json!({
        "arena_slots": {"pairs": s.pairs, "vectors": s.vectors, "strings": s.strings, "objects": s.objects},
        "free_slots": {"pairs": s.free_pairs, "vectors": s.free_vectors, "strings": s.free_strings, "objects": s.free_objects},
        "symbols": s.symbols, "allocations_since_gc": s.allocs_since_gc,
        "collections": s.gc_collections, "last_swept": s.gc_last_swept
    })
}

// Scheme's monotonic clock encloses only calls into an already loaded procedure.
// The driver is compiled before its first clock read; verification follows the
// second. This intentionally includes the loop, procedure calls and automatic GC.
fn execution_time(interp: &BenchInterp, iterations: u64, expected: &str, jps: f64) -> Duration {
    if iterations == 0 {
        return Duration::ZERO;
    }
    let program = format!(
        "(let ((start (current-jiffy)))
           (let loop ((n {iterations}) (answer #f))
             (if (= n 0)
                 (let ((elapsed (- (current-jiffy) start)))
                   (if (equal? answer {expected}) elapsed (error \"incorrect timed answer\")))
                 (loop (- n 1) (bench-work)))))"
    );
    let ticks = interp
        .eval(&program)
        .as_fixnum()
        .expect("elapsed jiffies must be a fixnum");
    assert!(ticks > 0, "empty or invalid timer measurement");
    Duration::from_secs_f64(ticks as f64 / jps)
}

fn phases(c: &mut Criterion) {
    let prefix = format!("{}/phases", backend());
    let id = format!("{prefix}/startup/bootstrap_and_drop");
    let mut selected = false;
    c.bench_function(&id, |b| {
        if !selected {
            make_interpreter().check(SUM, "5050");
            selected = true;
        }
        b.iter(|| drop(black_box(make_interpreter())));
    });
    if selected {
        checked(
            &id,
            json!({"fresh_interpreter": true, "includes_drop": true}),
        );
    }

    let id = format!("{prefix}/frontend/parse_expand_lower");
    let mut selected = false;
    c.bench_function(&id, |b| {
        if !selected {
            make_interpreter().check(SUM, "5050");
            selected = true;
        }
        if backend() == "vm" {
            let mut registry = patina_primitives::PrimitiveRegistry::new();
            patina_primitives::register_all(&mut registry);
            b.iter_batched_ref(
                make_interpreter,
                |interp| {
                    compile_with_qq_resolving(
                        &expand(interp),
                        interp.env().heap(),
                        interp.env(),
                        &registry,
                    )
                    .unwrap()
                },
                BatchSize::PerIteration,
            );
        } else {
            b.iter_batched_ref(
                make_interpreter,
                |interp| CpsTransformer::new().transform_toplevel(&expand(interp)),
                BatchSize::PerIteration,
            );
        }
    });
    if selected {
        checked(
            &id,
            json!({"expression": SUM, "expected": "5050", "fresh_heap_per_iteration": true}),
        );
    }

    for (name, code, expected) in [
        ("execution/sum_100", SUM, "5050"),
        ("allocation_gc/list_256", ALLOC, "256"),
    ] {
        let id = format!("{prefix}/{name}");
        let mut prepared = None;
        c.bench_function(&id, |b| {
            let (interp, jps) = prepared.get_or_insert_with(|| {
                let interp = make_interpreter();
                interp.eval("(import (scheme time))");
                interp.eval(&format!("(define (bench-work) {code})"));
                interp.check("(bench-work)", expected);
                let jps = interp.eval("(jiffies-per-second)").as_fixnum().unwrap() as f64;
                assert!(jps > 0.0);
                (interp, jps)
            });
            b.iter_custom(|iterations| execution_time(interp, iterations, expected, *jps));
        });
        if let Some((_, jps)) = prepared {
            let mut parameters =
                json!({"expression": code, "expected": expected, "jiffies_per_second": jps});
            if name.starts_with("allocation_gc/") {
                // A separate, fixed-size probe: these are not per-sample counts.
                let probe = make_interpreter();
                probe.eval(&format!("(define (bench-work) {code})"));
                probe.eval("(gc)");
                let before = heap_counters(&probe);
                probe.check("(let loop ((n 1000) (answer 0)) (if (= n 0) answer (loop (- n 1) (bench-work))))", expected);
                let after = heap_counters(&probe);
                probe.eval("(gc)");
                parameters["gc_probe"] = json!({"calls": 1000, "pairs_per_call": 256, "before": before, "after": after, "after_explicit_gc": heap_counters(&probe)});
            }
            checked(&id, parameters);
        }
    }
}

criterion_group!(benches, end_to_end, phases);
criterion_main!(benches);
