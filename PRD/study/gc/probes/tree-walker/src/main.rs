use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::mem::size_of;
use std::time::Instant;

use patina_core::cont_value::{ContEnv, ContValue, ExceptionHandler, PromptFrame};
use patina_core::continuation::{CpsContinuation, DynamicWindRecord};
use patina_core::cps_expr::{CpsExpr, CpsExprKind};
use patina_core::environment::Environment;
use patina_core::heap::HeapObjectData;
use patina_core::procedure::Procedure;
use patina_core::tagged_value::TaggedValue;

struct Counting;
static COUNT: AtomicUsize = AtomicUsize::new(0);
static BYTES: AtomicUsize = AtomicUsize::new(0);
static TRACK: AtomicBool = AtomicBool::new(false);
static HIST: Mutex<Option<BTreeMap<usize, usize>>> = Mutex::new(None);
thread_local! { static IN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if TRACK.load(Ordering::Relaxed) {
            COUNT.fetch_add(1, Ordering::Relaxed);
            BYTES.fetch_add(l.size(), Ordering::Relaxed);
            IN.with(|f| {
                if !f.get() {
                    f.set(true);
                    if let Ok(mut h) = HIST.try_lock() {
                        if let Some(m) = h.as_mut() { *m.entry(l.size()).or_insert(0) += 1; }
                    }
                    f.set(false);
                }
            });
        }
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) { unsafe { System.dealloc(p, l) } }
}
#[global_allocator]
static A: Counting = Counting;

fn sizes() {
    println!("== sizes (bytes) ==");
    println!("TaggedValue        {}", size_of::<TaggedValue>());
    println!("ContValue          {}", size_of::<ContValue>());
    println!("ContEnv            {}", size_of::<ContEnv>());
    println!("CpsContinuation    {}", size_of::<CpsContinuation>());
    println!("PromptFrame        {}", size_of::<PromptFrame>());
    println!("DynamicWindRecord  {}", size_of::<DynamicWindRecord>());
    println!("ExceptionHandler   {}", size_of::<ExceptionHandler>());
    println!("Environment        {}", size_of::<Environment>());
    println!("CpsExpr            {}", size_of::<CpsExpr>());
    println!("CpsExprKind        {}", size_of::<CpsExprKind>());
    println!("Procedure          {}", size_of::<Procedure>());
    println!("HeapObjectData     {}", size_of::<HeapObjectData>());
    println!("Option<SourceLoc>  {}", size_of::<Option<patina_core::SourceLocation>>());
    // StepResult::ApplyProc payload by hand: proc + Vec args + ContValue + Rc env + ContEnv + 3 Vec
    let apply = 8 + 24 + size_of::<ContValue>() + 8 + 8 + 3 * 24;
    println!("StepResult::ApplyProc payload ~ {}", apply);
}

fn main() {
    sizes();
    let gc_off = std::env::var("PATINA_GC").as_deref() == Ok("0");
    let interp = patina_interpreter::Interpreter::new_tree_walker();
    if let Some(e) = interp.backend().bootstrap_error() { eprintln!("bootstrap error: {e}"); }
    let _ = interp.eval_program("(import (scheme base))");
    let heap = interp.global_env().heap().clone();
    if let Ok(n) = std::env::var("FIBN") {
        let t = Instant::now();
        let r = interp.eval_program(&format!("(define (fib n) (if (< n 2) n (+ (fib (- n 1)) (fib (- n 2))))) (fib {n})"));
        println!("fib {n}: ok={} {:.1} ms", r.is_ok(), t.elapsed().as_secs_f64()*1e3);
        return;
    }
    if std::env::var("BASEGC").is_ok() {
        for imp in ["#t", "(import (scheme char) (scheme cxr) (scheme lazy) (scheme case-lambda))", "(import (srfi 1))", "(import (scheme list) (scheme vector) (scheme sort) (scheme hash-table))"] {
            let r = interp.eval_program(imp);
            if let Err(e) = &r { eprintln!("import error {e}"); }
            let mut best = f64::MAX;
            for _ in 0..5 {
                let t = Instant::now(); let _ = interp.eval_program("(gc)"); best = best.min(t.elapsed().as_secs_f64()*1e3);
            }
            let st = heap.borrow().stats();
            println!("after {imp}: (gc) best-of-5 {best:.2} ms; live-ish objects slots={} pairs={} (free o={} p={})", st.objects, st.pairs, st.free_objects, st.free_pairs);
        }
        return;
    }
    let progs: Vec<(&str, &str)> = vec![
        ("fib25", "(define (fib n) (if (< n 2) n (+ (fib (- n 1)) (fib (- n 2))))) (fib 25)"),
        ("loop1e6", "(let loop ((i 0) (acc 0)) (if (= i 1000000) acc (loop (+ i 1) (+ acc i))))"),
        ("cons1e5x10", "(define (build n acc) (if (= n 0) acc (build (- n 1) (cons n acc)))) (let loop ((k 0)) (if (< k 10) (begin (build 100000 '()) (loop (+ k 1))) 'done))"),
        ("closures1e5", "(define (mk n) (lambda () n)) (let loop ((i 0) (s 0)) (if (= i 100000) s (loop (+ i 1) (+ s ((mk i))))))"),
        ("callcc1e5", "(let loop ((i 0) (s 0)) (if (= i 100000) s (loop (+ i 1) (+ s (call/cc (lambda (k) (k i)))))))"),
        ("dynwind1e5", "(let loop ((i 0)) (if (= i 100000) i (begin (dynamic-wind (lambda () #f) (lambda () i) (lambda () #f)) (loop (+ i 1)))))"),
        ("map1e5x10", "(define l (let b ((n 100000) (a '())) (if (= n 0) a (b (- n 1) (cons n a))))) (let loop ((k 0)) (if (< k 10) (begin (map (lambda (x) (+ x 1)) l) (loop (+ k 1))) 'done))"),
    ];
    let mut hist_total: BTreeMap<usize, usize> = BTreeMap::new();
    for (name, src) in &progs {
        let before = heap.borrow().stats();
        *HIST.lock().unwrap() = Some(BTreeMap::new());
        COUNT.store(0, Ordering::Relaxed);
        BYTES.store(0, Ordering::Relaxed);
        TRACK.store(true, Ordering::Relaxed);
        let t = Instant::now();
        let r = interp.eval_program(src);
        let dt = t.elapsed();
        if let Err(e) = &r { eprintln!("{name}: error {e}"); }
        TRACK.store(false, Ordering::Relaxed);
        let after = heap.borrow().stats();
        let h = HIST.lock().unwrap().take().unwrap();
        let env_allocs = h.get(&(size_of::<Environment>() + 16)).copied().unwrap_or(0);
        let mut top: Vec<_> = h.iter().map(|(s, c)| (*c, *s)).collect();
        top.sort_unstable_by(|a, b| b.cmp(a));
        let top: Vec<String> = top.iter().take(6).map(|(c, s)| format!("{s}B x{c}")).collect();
        for (s, c) in &h { *hist_total.entry(*s).or_insert(0) += c; }
        println!(
            "{name:12} ok={} time={:>8.1}ms rust_allocs={:>9} rust_bytes={:>11} env_rcbox(240B)={:>8} heap_allocs(no-gc only)={:>9} collections={} top={:?}",
            r.is_ok(),
            dt.as_secs_f64() * 1e3,
            COUNT.load(Ordering::Relaxed),
            BYTES.load(Ordering::Relaxed),
            env_allocs,
            if gc_off { (after.allocs_since_gc - before.allocs_since_gc) as i64 } else { -1 },
            after.gc_collections - before.gc_collections,
            top
        );
    }
    // Full-collection pause probe: build a live structure, then time (gc).
    let _ = interp.eval_program("(define big (let b ((n 300000) (a '())) (if (= n 0) a (b (- n 1) (cons (vector n n) a)))))");
    let _ = interp.eval_program("(define (deep n) (if (= n 0) (begin (gc) 0) (+ 1 (deep (- n 1)))))");
    for _ in 0..3 {
        let t = Instant::now(); let _ = interp.eval_program("#t"); let base = t.elapsed();
        let t = Instant::now(); let _ = interp.eval_program("(gc)"); let g = t.elapsed();
        println!("(gc) with 300k live pairs+vectors: {:.2} ms (baseline form {:.3} ms)", g.as_secs_f64()*1e3, base.as_secs_f64()*1e3);
    }
    let _ = interp.eval_program("(define (deep2 n) (if (= n 0) 0 (+ 1 (deep2 (- n 1)))))");
    for depth in [1000usize, 10000, 50000] {
        let t = Instant::now(); let r = interp.eval_program(&format!("(deep {depth})")); let g = t.elapsed();
        let t = Instant::now(); let _ = interp.eval_program(&format!("(deep2 {depth})")); let g2 = t.elapsed();
        println!("(gc) at Scheme call depth {depth}: with gc {:.2} ms, without {:.2} ms ok={}", g.as_secs_f64()*1e3, g2.as_secs_f64()*1e3, r.is_ok());
    }
    let s = heap.borrow().stats();
    println!("final heap stats: {:?}", s);
}
