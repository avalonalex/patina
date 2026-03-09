# VM Testing Framework Design

**Status:** Design Document
**Created:** 2025-12-13
**Related:** [VM_SPECIFICATION.md](./VM_SPECIFICATION.md), [COMPILATION_DESIGN.md](./COMPILATION_DESIGN.md)

---

## Overview

This document describes the testing strategy for the Patina VM. The key principle is **test once, run everywhere**: the same Scheme test suite should pass on both the tree-walker and VM backends.

---

## Table of Contents

1. [Testing Principles](#testing-principles)
2. [Test Architecture](#test-architecture)
3. [Backend-Agnostic Test Harness](#backend-agnostic-test-harness)
4. [Test Categories](#test-categories)
5. [VM-Specific Tests](#vm-specific-tests)
6. [Performance Testing](#performance-testing)
7. [Differential Testing](#differential-testing)
8. [Property-Based Testing](#property-based-testing)
9. [Continuous Integration](#continuous-integration)
10. [Implementation Plan](#implementation-plan)

---

## Testing Principles

### 1. Shared Test Suite

All existing R7RS compliance tests should run on both backends:

```
crates/patina-tests/tests/compliance/
├── lists.rs           # Tests (+ more) run on both backends
├── numbers.rs
├── strings.rs
└── ...
```

### 2. Backend Independence

Tests should not depend on backend implementation details:

```rust
// GOOD: Tests behavior
#[test]
fn test_factorial() {
    assert_eval_to("(define (fact n) ...) (fact 5)", "120");
}

// BAD: Tests implementation detail
#[test]
fn test_register_allocation() {
    let compiler = VmCompiler::new();
    // This is VM-specific
}
```

### 3. Correctness Before Performance

- First, ensure correctness matches tree-walker
- Then, optimize and benchmark
- Never sacrifice correctness for speed

---

## Test Architecture

### Directory Structure

```
crates/
├── patina-tests/
│   └── tests/
│       ├── compliance/          # R7RS compliance (run on all backends)
│       │   ├── lists.rs
│       │   ├── numbers.rs
│       │   └── ...
│       ├── common/
│       │   └── mod.rs           # Test utilities, backend abstraction
│       └── interpreter_api.rs   # API tests
│
├── patina-tree-walker/
│   └── tests/                   # Tree-walker unit tests
│
└── patina-vm/
    └── tests/
        ├── unit/                # VM unit tests
        │   ├── compiler.rs
        │   ├── bytecode.rs
        │   └── vm_execution.rs
        ├── integration/         # VM integration tests
        └── benchmarks/          # Performance tests
```

### Test Harness API

```rust
// crates/patina-tests/tests/common/mod.rs

/// Backend-agnostic test trait.
pub trait TestBackend {
    fn eval_str(&self, code: &str) -> Result<String, String>;
    fn eval_program(&self, code: &str) -> Result<String, String>;
    fn name(&self) -> &str;
}

/// Run a test on all backends.
pub fn test_on_all_backends<F>(test_fn: F)
where
    F: Fn(&dyn TestBackend),
{
    // Tree-walker
    let tree_walker = TreeWalkerTestBackend::new();
    test_fn(&tree_walker);

    // VM (when available)
    #[cfg(feature = "vm")]
    {
        let vm = VmTestBackend::new();
        test_fn(&vm);
    }
}

/// Assert expression evaluates to expected value on all backends.
pub fn assert_eval_to(expr: &str, expected: &str) {
    test_on_all_backends(|backend| {
        let result = backend.eval_str(expr)
            .expect(&format!("[{}] eval failed: {}", backend.name(), expr));
        assert_eq!(
            result.trim(),
            expected,
            "[{}] {} != {}",
            backend.name(),
            result,
            expected
        );
    });
}

/// Assert program evaluates to expected value on all backends.
pub fn assert_program_eval_to(program: &str, expected: &str) {
    test_on_all_backends(|backend| {
        let result = backend.eval_program(program)
            .expect(&format!("[{}] program eval failed", backend.name()));
        assert_eq!(result.trim(), expected, "[{}]", backend.name());
    });
}

/// Assert expression causes an error on all backends.
pub fn assert_eval_error(expr: &str) {
    test_on_all_backends(|backend| {
        let result = backend.eval_str(expr);
        assert!(
            result.is_err(),
            "[{}] expected error for: {}",
            backend.name(),
            expr
        );
    });
}
```

---

## Backend-Agnostic Test Harness

### Tree-Walker Backend

```rust
pub struct TreeWalkerTestBackend {
    interpreter: TreeWalkInterpreter,
}

impl TreeWalkerTestBackend {
    pub fn new() -> Self {
        Self {
            interpreter: TreeWalkInterpreter::new(),
        }
    }
}

impl TestBackend for TreeWalkerTestBackend {
    fn eval_str(&self, code: &str) -> Result<String, String> {
        self.interpreter
            .eval_str(code)
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    fn eval_program(&self, code: &str) -> Result<String, String> {
        self.interpreter
            .eval_program(code)
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    fn name(&self) -> &str {
        "tree-walker"
    }
}
```

### VM Backend

```rust
pub struct VmTestBackend {
    vm: VirtualMachine,
}

impl VmTestBackend {
    pub fn new() -> Self {
        Self {
            vm: VirtualMachine::new(),
        }
    }
}

impl TestBackend for VmTestBackend {
    fn eval_str(&self, code: &str) -> Result<String, String> {
        self.vm
            .eval_str(code)
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    fn eval_program(&self, code: &str) -> Result<String, String> {
        self.vm
            .eval_program(code)
            .map(|v| v.to_string())
            .map_err(|e| e.to_string())
    }

    fn name(&self) -> &str {
        "vm"
    }
}
```

### Conditional Compilation

```toml
# crates/patina-tests/Cargo.toml
[features]
default = ["tree-walker"]
tree-walker = ["patina-tree-walker"]
vm = ["patina-vm"]
all-backends = ["tree-walker", "vm"]

[dependencies]
patina-tree-walker = { path = "../patina-tree-walker", optional = true }
patina-vm = { path = "../patina-vm", optional = true }
```

---

## Test Categories

### 1. R7RS Compliance Tests

These are the core tests that MUST pass on all backends:

```rust
// crates/patina-tests/tests/compliance/numbers.rs

#[test]
fn test_arithmetic() {
    assert_eval_to("(+ 1 2 3)", "6");
    assert_eval_to("(* 2 3 4)", "24");
    assert_eval_to("(- 10 3 2)", "5");
    assert_eval_to("(/ 24 4 2)", "3");
}

#[test]
fn test_numeric_tower() {
    // Fixnum
    assert_eval_to("(+ 1 2)", "3");

    // Bignum promotion
    assert_eval_to("(+ 9223372036854775807 1)", "9223372036854775808");

    // Rational
    assert_eval_to("(+ 1/2 1/3)", "5/6");

    // Float
    assert_eval_to("(exact? (+ 1.0 2.0))", "#f");

    // Complex
    assert_eval_to("(+ 1+2i 3+4i)", "4+6i");
}
```

### 2. Macro Tests

```rust
// crates/patina-tests/tests/compliance/macros.rs

#[test]
fn test_let() {
    assert_eval_to("(let ((x 1) (y 2)) (+ x y))", "3");
}

#[test]
fn test_nested_let() {
    assert_eval_to(
        "(let ((x 1)) (let ((y (+ x 1))) (+ x y)))",
        "3"
    );
}

#[test]
fn test_letrec_mutual_recursion() {
    assert_program_eval_to(
        "(letrec ((even? (lambda (n) (if (= n 0) #t (odd? (- n 1)))))
                 (odd? (lambda (n) (if (= n 0) #f (even? (- n 1))))))
           (even? 10))",
        "#t"
    );
}
```

### 3. Control Flow Tests

```rust
// crates/patina-tests/tests/compliance/control.rs

#[test]
fn test_if() {
    assert_eval_to("(if #t 1 2)", "1");
    assert_eval_to("(if #f 1 2)", "2");
    assert_eval_to("(if 0 1 2)", "1"); // 0 is truthy
}

#[test]
fn test_tail_recursion() {
    // This should not overflow the stack
    assert_program_eval_to(
        "(define (loop n acc)
           (if (= n 0)
               acc
               (loop (- n 1) (+ acc 1))))
         (loop 100000 0)",
        "100000"
    );
}
```

### 4. Continuation Tests

```rust
// crates/patina-tests/tests/compliance/continuations.rs

#[test]
fn test_call_cc_escape() {
    assert_eval_to(
        "(call/cc (lambda (k) (+ 1 (k 10) 100)))",
        "10"
    );
}

#[test]
fn test_call_cc_return() {
    assert_eval_to(
        "(call/cc (lambda (k) (+ 1 2 3)))",
        "6"
    );
}

#[test]
fn test_dynamic_wind() {
    assert_program_eval_to(
        "(let ((log '()))
           (dynamic-wind
             (lambda () (set! log (cons 'before log)))
             (lambda () (set! log (cons 'during log)))
             (lambda () (set! log (cons 'after log))))
           (reverse log))",
        "(before during after)"
    );
}
```

---

## VM-Specific Tests

These tests are specific to the VM implementation:

### Compiler Unit Tests

```rust
// crates/patina-vm/tests/unit/compiler.rs

#[test]
fn test_compile_literal() {
    let expr = VmCoreExpr::Literal(TaggedValue::fixnum(42));
    let code = compile_expr(&expr).unwrap();

    assert_eq!(code.instructions.len(), 1);
    assert!(matches!(
        code.instructions[0],
        Opcode::LoadImmediate { value, .. } if value == TaggedValue::fixnum(42)
    ));
}

#[test]
fn test_compile_if_generates_correct_jumps() {
    let expr = VmCoreExpr::If {
        test: Box::new(VmCoreExpr::Var { name: "x".into(), scopes: ScopeSet::new() }),
        then: Box::new(VmCoreExpr::Literal(TaggedValue::fixnum(1))),
        else_: Box::new(VmCoreExpr::Literal(TaggedValue::fixnum(2))),
    };
    let code = compile_expr(&expr).unwrap();

    // Should have: JumpUnless, LoadImmediate(1), Jump, LoadImmediate(2)
    assert!(matches!(code.instructions[0], Opcode::JumpUnless { .. }));
}

#[test]
fn test_free_variable_analysis() {
    let expr = parse_expr("(lambda (x) (+ x y))");
    let free_vars = analyze_free_vars(&expr);

    assert!(free_vars.contains(&"y".into()));
    assert!(!free_vars.contains(&"x".into()));
}
```

### Bytecode Unit Tests

```rust
// crates/patina-vm/tests/unit/bytecode.rs

#[test]
fn test_opcode_encoding() {
    let op = Opcode::LoadConst { dst: Register(5), constant_index: ConstantIndex(42) };
    let encoded = op.encode();
    let decoded = Opcode::decode(&encoded).unwrap();

    assert_eq!(op, decoded);
}

#[test]
fn test_code_object_serialization() {
    let code = CodeObject {
        instructions: vec![
            Opcode::LoadImmediate { dst: Register(0), value: TaggedValue::fixnum(1) },
            Opcode::Return { value: Register(0) },
        ],
        constants: ConstantPool::new(),
        num_registers: 1,
        // ...
    };

    let bytes = code.serialize();
    let deserialized = CodeObject::deserialize(&bytes).unwrap();

    assert_eq!(code.instructions, deserialized.instructions);
}
```

### VM Execution Tests

```rust
// crates/patina-vm/tests/unit/vm_execution.rs

#[test]
fn test_vm_execute_simple() {
    let code = CodeObject {
        instructions: vec![
            Opcode::LoadImmediate { dst: Register(0), value: TaggedValue::fixnum(42) },
            Opcode::Return { value: Register(0) },
        ],
        // ...
    };

    let mut vm = VirtualMachine::new();
    let result = vm.execute(&code).unwrap();

    assert_eq!(result, TaggedValue::fixnum(42));
}

#[test]
fn test_vm_stack_operations() {
    let mut vm = VirtualMachine::new();

    // Test call stack behavior
    let outer = compile("(define (f x) (g x)) (f 1)");
    let inner = compile("(define (g y) y)");

    vm.load(&outer);
    vm.load(&inner);
    let result = vm.run().unwrap();

    assert_eq!(result, TaggedValue::fixnum(1));
}

#[test]
fn test_closure_capture() {
    let code = compile("
        (define (make-adder n)
          (lambda (x) (+ x n)))
        (define add5 (make-adder 5))
        (add5 10)
    ");

    let mut vm = VirtualMachine::new();
    let result = vm.eval(&code).unwrap();

    assert_eq!(result, TaggedValue::fixnum(15));
}
```

---

## Performance Testing

### Benchmark Suite

```rust
// crates/patina-vm/tests/benchmarks/mod.rs

use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn benchmark_fibonacci(c: &mut Criterion) {
    let code = "
        (define (fib n)
          (if (< n 2)
              n
              (+ (fib (- n 1)) (fib (- n 2)))))
        (fib 30)
    ";

    c.bench_function("fib_30_tree_walker", |b| {
        let interp = TreeWalkInterpreter::new();
        b.iter(|| {
            black_box(interp.eval_program(code).unwrap())
        })
    });

    c.bench_function("fib_30_vm", |b| {
        let vm = VirtualMachine::new();
        b.iter(|| {
            black_box(vm.eval_program(code).unwrap())
        })
    });
}

fn benchmark_tail_recursion(c: &mut Criterion) {
    let code = "
        (define (sum-to n acc)
          (if (= n 0)
              acc
              (sum-to (- n 1) (+ acc n))))
        (sum-to 100000 0)
    ";

    c.bench_function("sum_100000_tree_walker", |b| {
        let interp = TreeWalkInterpreter::new();
        b.iter(|| {
            black_box(interp.eval_program(code).unwrap())
        })
    });

    c.bench_function("sum_100000_vm", |b| {
        let vm = VirtualMachine::new();
        b.iter(|| {
            black_box(vm.eval_program(code).unwrap())
        })
    });
}

fn benchmark_list_operations(c: &mut Criterion) {
    let code = "
        (define (make-list n)
          (if (= n 0)
              '()
              (cons n (make-list (- n 1)))))
        (length (make-list 10000))
    ";

    c.bench_function("list_10000", |b| {
        let vm = VirtualMachine::new();
        b.iter(|| {
            black_box(vm.eval_program(code).unwrap())
        })
    });
}

criterion_group!(
    benches,
    benchmark_fibonacci,
    benchmark_tail_recursion,
    benchmark_list_operations
);
criterion_main!(benches);
```

### Performance Targets

| Benchmark | Tree-Walker | VM Target | Speedup |
|-----------|-------------|-----------|---------|
| fib(30) | ~2000ms | ~200ms | 10x |
| sum(100000) | ~500ms | ~50ms | 10x |
| list(10000) | ~100ms | ~20ms | 5x |

---

## Differential Testing

### Tree-Walker vs VM

```rust
// crates/patina-tests/tests/differential.rs

use proptest::prelude::*;

/// Generate random Scheme expressions.
fn arb_expr() -> impl Strategy<Value = String> {
    prop_oneof![
        // Literals
        any::<i64>().prop_map(|n| n.to_string()),
        prop::bool::ANY.prop_map(|b| if b { "#t".to_string() } else { "#f".to_string() }),

        // Arithmetic
        (any::<i64>(), any::<i64>()).prop_map(|(a, b)| format!("(+ {} {})", a, b)),
        (any::<i64>(), any::<i64>()).prop_map(|(a, b)| format!("(* {} {})", a, b)),

        // Conditionals
        (arb_simple_expr(), arb_simple_expr(), arb_simple_expr())
            .prop_map(|(test, then, els)| format!("(if {} {} {})", test, then, els)),
    ]
}

proptest! {
    #[test]
    fn differential_test_backends(expr in arb_expr()) {
        let tree_walker = TreeWalkInterpreter::new();
        let vm = VirtualMachine::new();

        let tw_result = tree_walker.eval_str(&expr);
        let vm_result = vm.eval_str(&expr);

        match (tw_result, vm_result) {
            (Ok(tw), Ok(vm)) => {
                prop_assert_eq!(tw.to_string(), vm.to_string());
            }
            (Err(_), Err(_)) => {
                // Both error - OK
            }
            (Ok(tw), Err(e)) => {
                prop_assert!(false, "VM failed but tree-walker succeeded: {} vs {:?}", tw, e);
            }
            (Err(e), Ok(vm)) => {
                prop_assert!(false, "Tree-walker failed but VM succeeded: {:?} vs {}", e, vm);
            }
        }
    }
}
```

### Chibi Comparison

```rust
// Compare against chibi-scheme for extra validation.

#[test]
#[ignore] // Only run in CI with chibi available
fn differential_test_chibi() {
    let exprs = vec![
        "(+ 1 2 3)",
        "(* 4 5 6)",
        "(let ((x 10)) (+ x 1))",
        "(map (lambda (x) (* x x)) '(1 2 3 4 5))",
    ];

    for expr in exprs {
        let patina_result = eval_with_patina(expr);
        let chibi_result = eval_with_chibi(expr);

        assert_eq!(
            patina_result, chibi_result,
            "Mismatch for expr: {}", expr
        );
    }
}

fn eval_with_chibi(expr: &str) -> String {
    use std::process::Command;

    let output = Command::new("chibi-scheme")
        .args(["-e", &format!("(write {})", expr)])
        .output()
        .expect("chibi-scheme not found");

    String::from_utf8(output.stdout).unwrap()
}
```

---

## Property-Based Testing

### Tagged Value Properties

```rust
// crates/patina-vm/tests/property/tagged_value.rs

use proptest::prelude::*;

proptest! {
    #[test]
    fn tagged_fixnum_roundtrip(n in -(1i64 << 60)..(1i64 << 60)) {
        let tagged = TaggedValue::fixnum(n);
        prop_assert!(tagged.is_fixnum());
        prop_assert_eq!(tagged.as_fixnum_unchecked(), n);
    }

    #[test]
    fn tagged_boolean_identity(b: bool) {
        let tagged = if b { TaggedValue::TRUE } else { TaggedValue::FALSE };
        prop_assert!(tagged.is_special());
        prop_assert_eq!(tagged.is_true(), b);
    }

    #[test]
    fn tagged_char_roundtrip(c: char) {
        let tagged = TaggedValue::character(c);
        prop_assert!(tagged.is_character());
        prop_assert_eq!(tagged.as_character_unchecked(), c);
    }
}
```

### Compiler Properties

```rust
proptest! {
    #[test]
    fn compile_preserves_semantics(expr in arb_simple_expr()) {
        // Parse and desugar
        let parsed = parse(&expr).unwrap();
        let desugared = desugar(&parsed).unwrap();

        // Interpret directly
        let direct_result = interpret_core_expr(&desugared);

        // Compile and execute
        let compiled = compile(&desugared).unwrap();
        let vm = VirtualMachine::new();
        let vm_result = vm.execute(&compiled);

        prop_assert_eq!(
            direct_result.map(|v| v.to_string()),
            vm_result.map(|v| v.to_string())
        );
    }
}
```

---

## Continuous Integration

### GitHub Actions Workflow

```yaml
# .github/workflows/vm-tests.yml
name: VM Tests

on: [push, pull_request]

jobs:
  test-all-backends:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-action@stable

      - name: Run tree-walker tests
        run: cargo test --package patina-tests --features tree-walker

      - name: Run VM tests
        run: cargo test --package patina-tests --features vm

      - name: Run all-backend tests
        run: cargo test --package patina-tests --features all-backends

      - name: Run VM unit tests
        run: cargo test --package patina-vm

  benchmarks:
    runs-on: ubuntu-latest
    if: github.ref == 'refs/heads/main'
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-action@stable

      - name: Run benchmarks
        run: cargo bench --package patina-vm

      - name: Upload benchmark results
        uses: actions/upload-artifact@v3
        with:
          name: benchmarks
          path: target/criterion

  differential:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-action@stable

      - name: Install chibi-scheme
        run: |
          sudo apt-get update
          sudo apt-get install -y chibi-scheme

      - name: Run differential tests
        run: cargo test --package patina-tests differential -- --ignored
```

### Test Matrix

| Test Category | Tree-Walker | VM | Chibi Diff |
|--------------|-------------|-----|------------|
| R7RS Compliance | Always | Always | Weekly |
| Unit Tests | Per-crate | Per-crate | N/A |
| Property Tests | Always | Always | N/A |
| Benchmarks | Main only | Main only | N/A |
| Differential | Always | Always | Weekly |

---

## Implementation Plan

### Phase 1: Test Infrastructure (1 week)

1. **Backend trait**
   - Define `TestBackend` trait
   - Implement for tree-walker

2. **Test helpers**
   - `assert_eval_to`
   - `assert_program_eval_to`
   - `assert_eval_error`

3. **Feature flags**
   - Set up conditional compilation
   - Test with tree-walker only

### Phase 2: VM Backend Tests (1 week)

1. **Implement `VmTestBackend`**
   - Connect to VM execution

2. **Migrate existing tests**
   - Ensure all tests use `test_on_all_backends`
   - Fix any backend-specific issues

3. **Add VM unit tests**
   - Compiler tests
   - Bytecode tests
   - VM execution tests

### Phase 3: Advanced Testing (1 week)

1. **Property-based tests**
   - TaggedValue properties
   - Compiler correctness

2. **Differential testing**
   - Tree-walker vs VM
   - Patina vs Chibi

3. **Benchmarks**
   - Set up Criterion
   - Establish baselines

### Phase 4: CI Integration (1 week)

1. **GitHub Actions**
   - Test workflows
   - Benchmark tracking

2. **Documentation**
   - Test guide
   - Contributing guide for tests

---

## Test Coverage Goals

| Component | Target Coverage |
|-----------|-----------------|
| Compiler | 90% |
| VM Execution | 85% |
| Tagged Value | 95% |
| Bytecode | 90% |
| Integration | 80% |

---

## References

- [VM_SPECIFICATION.md](./VM_SPECIFICATION.md) - VM design
- [COMPILATION_DESIGN.md](./COMPILATION_DESIGN.md) - Compiler design
- Existing tests: `crates/patina-tests/tests/`
- Chibi test suite: `~/Project/reference/chibi-scheme/tests/r7rs-tests.scm`
