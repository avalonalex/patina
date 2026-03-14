# Patina VM: Testing Discipline

**Status:** Draft v0.1
**Depends on:** [VM_ISA.md](./VM_ISA.md), [VM_COMPILER.md](./VM_COMPILER.md), [VM_RUNTIME.md](./VM_RUNTIME.md)
**See also:** `docs/TEST_ORGANIZATION.md` — existing tree-walker test structure

---

## 1. Guiding Principles

1. **Correctness before performance.** The VM must pass all existing tests before
   any optimization work begins. A faster wrong answer is worse than a slower
   correct one.

2. **The `Backend` trait is the correctness contract.** Any `Interpreter<VmBackend>`
   that implements `Backend` correctly will automatically inherit all existing
   tree-walker tests. This is the primary correctness guarantee.

3. **Test each layer independently.** The compiler, runtime, and their interaction
   are separate concerns. A bug should be locatable to a specific layer without
   running the full stack.

4. **The tree-walker is the oracle.** For any Scheme expression where the
   tree-walker produces a result, the VM must produce the same result. Divergence
   is a VM bug.

---

## 2. Test Layers

```
Layer 4: R7RS compliance (chibi-scheme as external oracle)
            ↑ run on VM backend via Backend trait
Layer 3: Shared integration tests (tree-walker tests reused on VM)
            ↑ run via patina-tests with both backends
Layer 2: VM-specific integration tests (continuation, tail call, GC)
            ↑ test VM behavior that has no tree-walker analog
Layer 1: VM unit tests (compiler passes, runtime primitives in isolation)
            ↑ test individual components without full stack
```

Each layer can be run independently. Failures in lower layers should be fixed
before investigating higher layers.

---

## 3. Layer 1 — VM Unit Tests

Located in `crates/patina-vm/src/` (inline) and `crates/patina-vm/tests/`.

### 3.1 Compiler Pass Tests

Each compiler pass (VM_COMPILER.md §4–8) gets its own test module. Tests operate
on hand-constructed `CoreExpr` inputs and inspect the output IR directly.

**Pass 1 — Analysis:**
```rust
#[test]
fn free_var_analysis_lambda() {
    // (lambda (x) (+ x y)) — x is bound, y is free
    let expr = lambda(["x"], [app(var("+""), [var("x"), var("y")])]);
    let info = Pass1Analysis::run(&expr);
    let lambda_info = info.lambdas.values().next().unwrap();
    assert!(lambda_info.free_vars.contains(&"y".into()));
    assert!(!lambda_info.free_vars.contains(&"x".into()));
}

#[test]
fn mutation_detection_captured_var() {
    // (lambda (x) (lambda () (set! x 1))) — x is mutated after capture
    let inner = lambda([], [set("x", lit(1))]);
    let outer = lambda(["x"], [inner]);
    let info = Pass1Analysis::run(&outer);
    let outer_info = &info.lambdas[&outer_id];
    assert!(outer_info.mutated_locals.contains(&"x".into()));
}
```

**Pass 3 — Tail marking:**
```rust
#[test]
fn tail_position_in_if() {
    // (lambda () (if #t (f) (g))) — both branches are tail
    let expr = lambda([], [if_(lit(true), app(var("f"), []), app(var("g"), []))]);
    let tailed = Pass3Tail::run(&Pass2Closure::run(&expr, &AnalysisInfo::empty()));
    // Both branches of the if should be marked is_tail=true
    assert_tail_marked(&tailed, "f");
    assert_tail_marked(&tailed, "g");
}

#[test]
fn non_tail_in_operator_position() {
    // (lambda () (+ (f) 1)) — (f) is NOT in tail position
    let expr = lambda([], [app(var("+"), [app(var("f"), []), lit(1)])]);
    let tailed = Pass3Tail::run(&Pass2Closure::run(&expr, &AnalysisInfo::empty()));
    assert_not_tail_marked(&tailed, "f");
}
```

**Pass 4 — Register allocation (tail call overlap invariant):**
```rust
#[test]
fn tail_call_no_register_overlap_swap() {
    // (define (f a b) (f b a)) — args swap, must not overlap
    let expr = define("f", lambda(["a", "b"],
        [tail_app(var("f"), [var("b"), var("a")])]));
    let regged = run_passes(&expr);
    assert_no_overlap_in_tail_calls(&regged);
}

#[test]
fn tail_call_no_register_overlap_mutual() {
    // (define (f a b) (g b a)) — mutual tail call
    let expr = define("f", lambda(["a", "b"],
        [tail_app(var("g"), [var("b"), var("a")])]));
    let regged = run_passes(&expr);
    assert_no_overlap_in_tail_calls(&regged);
}
```

### 3.2 Runtime Unit Tests

Test individual execution loop handlers with hand-constructed `CodeObject`s.

```rust
#[test]
fn load_immediate_fixnum() {
    let code = code_object([
        Instruction::LoadImmediate { dst: 0, val: TaggedValue::fixnum(42) },
        Instruction::Return { val: 0 },
    ]);
    assert_eq!(run_code(code), Ok(TaggedValue::fixnum(42)));
}

#[test]
fn call_and_return() {
    // Inner function returns 99; outer calls it and returns the result
    let inner = code_object([
        Instruction::LoadImmediate { dst: 0, val: TaggedValue::fixnum(99) },
        Instruction::Return { val: 0 },
    ]);
    let outer = code_object([
        Instruction::LoadConst { dst: 0, idx: 0 },  // load inner closure
        Instruction::Call { func: 0, args: vec![], dst: 1 },
        Instruction::Return { val: 1 },
    ]).with_constant(inner);
    assert_eq!(run_code(outer), Ok(TaggedValue::fixnum(99)));
}

#[test]
fn tail_call_does_not_grow_stack() {
    // A tail-recursive loop of depth 100_000 should not overflow
    let result = eval("(define (loop n) (if (= n 0) 'done (loop (- n 1)))) (loop 100000)");
    assert_eq!(result, Ok(symbol("done")));
}
```

### 3.3 Continuation Unit Tests

These test the continuation machinery in isolation before running full programs.

```rust
#[test]
fn call_with_prompt_normal_return() {
    // Body returns normally — result reaches dst, no handler invoked
    let result = eval("
        (call-with-continuation-prompt
          (lambda () 42)
          (default-continuation-prompt-tag)
          (lambda (v k) 'should-not-reach))
    ");
    assert_eq!(result, Ok(fixnum(42)));
}

#[test]
fn abort_to_prompt_invokes_handler() {
    let result = eval("
        (call-with-continuation-prompt
          (lambda ()
            (abort-current-continuation
              (default-continuation-prompt-tag)
              99))
          (default-continuation-prompt-tag)
          (lambda (v k) v))
    ");
    assert_eq!(result, Ok(fixnum(99)));
}

#[test]
fn dynamic_wind_fires_on_abort() {
    let result = eval("
        (let ((log '()))
          (call-with-continuation-prompt
            (lambda ()
              (dynamic-wind
                (lambda () (set! log (cons 'before log)))
                (lambda () (abort-current-continuation
                             (default-continuation-prompt-tag) 'done))
                (lambda () (set! log (cons 'after log)))))
            (default-continuation-prompt-tag)
            (lambda (v k) log)))
    ");
    // 'after' thunk must fire even on abort
    assert!(result_contains(result, "after"));
}
```

---

## 4. Layer 2 — VM-Specific Integration Tests

Located in `crates/patina-vm/tests/integration/`. These test VM behaviors that
the tree-walker either handles differently or has no analog for.

### 4.1 Tail Call Correctness

```rust
#[test]
fn mutual_tail_recursion_constant_stack() {
    // even?/odd? mutual tail recursion at large depth
    eval_to("
        (define (even? n) (if (= n 0) #t (odd? (- n 1))))
        (define (odd? n) (if (= n 0) #f (even? (- n 1))))
        (even? 1000000)
    ", "#t");
}

#[test]
fn named_let_tail_loop() {
    eval_to("
        (let loop ((n 1000000) (acc 0))
          (if (= n 0) acc (loop (- n 1) (+ acc 1))))
    ", "1000000");
}
```

### 4.2 Closure Correctness

```rust
#[test]
fn mutable_captured_variable() {
    eval_to("
        (define counter
          (let ((n 0))
            (lambda ()
              (set! n (+ n 1))
              n)))
        (counter) (counter) (counter)
    ", "3");
}

#[test]
fn shared_mutable_cell_across_closures() {
    eval_to("
        (define-values (get set!)
          (let ((x 0))
            (values (lambda () x)
                    (lambda (v) (set! x v)))))
        (set! 42)
        (get)
    ", "42");
}
```

### 4.3 call/cc

```rust
#[test]
fn callcc_escape() {
    eval_to("(call/cc (lambda (k) (+ 1 (k 10) 100)))", "10");
}

#[test]
fn callcc_reentry() {
    eval_to("
        (define k #f)
        (define n (call/cc (lambda (c) (set! k c) 0)))
        (if (< n 3) (k (+ n 1)) n)
    ", "3");
}

#[test]
fn callcc_and_dynamic_wind() {
    eval_to("
        (let ((log '()))
          (let ((k (call/cc (lambda (k) k))))
            (dynamic-wind
              (lambda () (set! log (cons 'in log)))
              (lambda () (if (< (length log) 6) (k k)))
              (lambda () (set! log (cons 'out log)))))
          (length log))
    ", "6");
}
```

---

## 5. Layer 3 — Shared Integration Tests (Tree-Walker Reuse)

The existing `patina-tests` integration tests run against `Interpreter<B: Backend>`.
When `patina-vm` implements `Backend`, **all existing tests automatically apply to
the VM** — no duplication needed.

### 5.1 What gets reused automatically

Everything in `crates/patina-tests/tests/`:

| Test file | What it covers | VM relevance |
|---|---|---|
| `compliance/numbers.rs` | Numeric tower | Full reuse |
| `compliance/strings.rs` | String operations | Full reuse |
| `compliance/lists.rs` | List operations | Full reuse |
| `tail_recursion.rs` | TCO correctness | Critical for VM |
| `hygiene.rs` | Macro hygiene | Full reuse (frontend shared) |
| `cps_features.rs` | CPS/continuation behavior | Critical for VM |
| `record_types.rs` | define-record-type | Full reuse |
| `parameters.rs` | Parameter objects | Full reuse |

### 5.2 How to run existing tests against the VM

```bash
# Run all patina-tests against VM backend
cargo test --package patina-tests --features vm

# Run against both backends (differential)
cargo test --package patina-tests --features all-backends

# Run a specific file against VM
cargo test --package patina-tests --features vm --test tail_recursion
```

### 5.3 Expected initial failures

When the VM is first wired into `patina-tests`, some tests may fail. The triage
order:

1. Fix basic evaluation failures first (literals, arithmetic, `if`)
2. Fix function call / tail call failures
3. Fix closure failures
4. Fix continuation / `call/cc` failures
5. Fix library loading failures (last — depends on all of the above)

Do not move on to performance work until all 1400 tests pass.

---

## 6. Layer 4 — R7RS Compliance (Chibi Tests)

The existing `./scripts/run_chibi_tests.sh` runs 1159 chibi R7RS tests against
the tree-walker. The same script must pass against the VM backend.

```bash
# Current: runs against tree-walker
cargo build --release && ./scripts/run_chibi_tests.sh

# Future: run against VM backend
cargo build --release --features vm && ./scripts/run_chibi_tests.sh --backend vm
```

Chibi tests serve as an external oracle independent of the tree-walker. A test
passing on the tree-walker but failing on chibi indicates a pre-existing bug.
A test passing on the tree-walker but failing on the VM (but passing on chibi)
indicates a VM bug.

---

## 7. Differential Testing

When both backends are available, the same expression can be run on both and
results compared. This is the most powerful correctness check.

```rust
// crates/patina-tests/tests/differential.rs

fn assert_same_on_both_backends(expr: &str) {
    let tw = TreeWalkerBackend::new().eval(expr);
    let vm = VmBackend::new().eval(expr);
    assert_eq!(
        tw.map(|v| v.to_string()),
        vm.map(|v| v.to_string()),
        "Backends disagree on: {expr}"
    );
}

#[test]
fn differential_arithmetic() {
    for expr in &["(+ 1 2)", "(* 3 4)", "(expt 2 32)", "(/ 22 7)"] {
        assert_same_on_both_backends(expr);
    }
}

#[test]
fn differential_tail_calls() {
    assert_same_on_both_backends("
        (define (f n acc) (if (= n 0) acc (f (- n 1) (+ acc 1))))
        (f 100000 0)
    ");
}
```

Differential testing should eventually be run with randomized Scheme expressions
(property-based testing) to catch corner cases not covered by the hand-written
suite.

---

## 8. Benchmarks

Benchmarks live in `crates/patina-vm/benches/` and use `criterion`.

**Baseline targets (Phase 2A acceptance):**

| Benchmark | Tree-walker | VM target | Notes |
|---|---|---|---|
| `fib(25)` recursive | ~832ms | < 200ms | 4× minimum |
| `sum(1_000_000)` tail loop | measure | < 2× tree-walker | tail call efficiency |
| `list-ref` 10k elements | measure | comparable | heap/GC |
| `callcc` escape loop | measure | comparable | continuation overhead |

Benchmarks are run manually, not in CI. Run them:
- During Phase 2B performance work
- Before and after any optimization to measure impact

---

## 9. Test Development Order

When building the VM, write tests in this order — each level gates the next:

1. **Compiler pass unit tests** — before writing any runtime code, verify each
   pass produces correct output on known inputs
2. **Runtime unit tests** — hand-constructed `CodeObject`s for `LoadImmediate`,
   `Call`, `Return`, `TailCall`
3. **Continuation unit tests** — `CallWithPrompt`, `AbortToPrompt`,
   `CaptureComposable` in isolation
4. **VM integration tests** — tail calls, closures, `call/cc` end-to-end
5. **Shared test suite** — wire VM into `patina-tests`, triage failures
6. **Chibi tests** — final R7RS compliance gate
7. **Differential tests** — ongoing, catches regressions
8. **Benchmarks** — Phase 2B only

---

## 10. What Not To Test in VM Tests

- **Frontend behavior** (parsing, macro expansion) — already tested by
  `patina-frontend` and `patina-macros`; the VM receives `CoreExpr` and trusts
  the frontend
- **`patina-primitives` correctness** — already tested by their own unit tests;
  the VM just calls them
- **Tree-walker internals** — not the VM's concern

VM tests should focus on: compilation correctness, execution correctness,
continuation semantics, and tail call behavior.
