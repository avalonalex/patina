# Roadmap: Making Patina Run R7RS Test Suite

This document outlines what needs to be implemented for Patina to run the full R7RS test files in `tests/schemes/`.

## Current Status

✅ **Working Now:**
- Basic arithmetic: `+, -, *, /`
- Comparisons: `=, <`
- Lists: `cons, car, cdr, list`
- Predicates: `null?, pair?`
- Control: `if, begin, define, quote`
- Expression-level comparison with chibi-scheme works!

⚠️ **Can't Run R7RS Test Files Yet Because:**

```scheme
;; tests/schemes/arithmetic/basic.scm
(import (scheme base)          ; ← ERROR: import not supported
        (scheme write))

(display (+ 1 2 3)) (newline)  ; ← ERROR: display not supported
                                ; ← ERROR: newline not supported
```

## Priority 1: I/O Procedures (Required for Test Files)

These are needed to run the test files at all:

### 1. `display` procedure
```scheme
(display "hello")              ; Prints: hello
(display 42)                   ; Prints: 42
(display (list 1 2 3))         ; Prints: (1 2 3)
```

**Implementation:** Add primitive in `src/eval/mod.rs`:
- Convert value to string (without quotes for strings)
- Print to stdout
- Return `Value::Unspecified`

### 2. `write` procedure
```scheme
(write "hello")                ; Prints: "hello" (with quotes)
(write 42)                     ; Prints: 42
(write (list 1 2 3))           ; Prints: (1 2 3)
```

**Implementation:** Similar to display but:
- Strings print with quotes
- Uses Scheme notation

### 3. `newline` procedure
```scheme
(newline)                      ; Prints a newline
```

**Implementation:** Print `\n`, return `Value::Unspecified`

**Estimated effort:** 1-2 hours

## Priority 2: Handle Import Statements

R7RS test files start with:
```scheme
(import (scheme base)
        (scheme write))
```

**Two approaches:**

### Option A: Gracefully Ignore (Quick)
```rust
// In eval_list(), add special form:
"import" => return Ok(Value::Unspecified),
```

**Pros:** Quick, tests run immediately
**Cons:** Not R7RS compliant

### Option B: Implement Module System (Proper)
- Parse import declarations
- Track which modules are imported
- Only allow procedures from imported modules

**Estimated effort:**
- Option A: 10 minutes
- Option B: Several hours (defer to later)

**Recommendation:** Start with Option A, implement B later

## Priority 3: Arithmetic Edge Cases

Currently might fail:
```scheme
(+)        ; Should return 0
(*)        ; Should return 1
(+ 5)      ; Should return 5
(* 7)      ; Should return 7
```

**Current implementation in `src/eval/mod.rs`:**
```rust
fn primitive_add(&self, args: Vec<Value>) -> Result<Value, EvalError> {
    let mut result = 0i64;
    for arg in args {
        // ... add each arg
    }
    Ok(Value::Integer(result))
}
```

**Fix needed:** Already handles empty args correctly! Test to confirm.

**Estimated effort:** 30 minutes testing/fixing

## Priority 4: More Comparison Operators

Test files use:
```scheme
(> 3 2)       ; Greater than
(<= 2 2)      ; Less than or equal
(>= 3 3)      ; Greater than or equal
```

**Implementation:** Similar to existing `<` and `=` primitives

**Estimated effort:** 30 minutes

## Priority 5: More List Procedures

Test files might use:
```scheme
(length (list 1 2 3))          ; => 3
(append (list 1 2) (list 3 4)) ; => (1 2 3 4)
(reverse (list 1 2 3))         ; => (3 2 1)
```

**Implementation:** Standard recursive list operations

**Estimated effort:** 1 hour

## Implementation Order (Recommended)

### Session 1: Make Tests Runnable (30 minutes)
1. Add `display` procedure (15 min)
2. Add `newline` procedure (5 min)
3. Add `import` special form (ignore) (5 min)
4. Test: `cargo test --test file_runner` should now work!

### Session 2: Complete I/O (30 minutes)
5. Add `write` procedure (15 min)
6. Test all R7RS test files run without errors

### Session 3: Fix Edge Cases (1 hour)
7. Test arithmetic with zero args: `(+)`, `(*)`
8. Add `>`, `<=`, `>=` operators
9. Add `write` to output comparison

### Session 4: More Features (as needed)
10. Implement more list procedures
11. Better module system
12. More predicates

## Verification Strategy

After each session, run:

```bash
# Test individual expressions
cargo test --test comparison_test

# Test full R7RS files
cargo test --test file_runner

# Compare outputs
chibi-scheme tests/schemes/arithmetic/basic.scm
# vs manually check Patina output
```

## Success Criteria

When Patina can do this, we're done:

```bash
# Generate expected outputs from chibi-scheme
cargo test --test file_runner generate_snapshots -- --ignored

# Run same files through Patina and compare
# (New test to be written)
cargo test --test file_runner compare_with_snapshots

# Should show:
# ✓ tests/schemes/arithmetic/basic.scm - PASS
# ✓ tests/schemes/arithmetic/comparisons.scm - PASS
# ✓ tests/schemes/lists/basic.scm - PASS
# ✓ tests/schemes/control/if.scm - PASS
# ✓ tests/schemes/control/begin.scm - PASS
```

## Code Locations

When implementing, modify these files:

```
src/eval/mod.rs
├─ Line 76-84:   eval_list() - Add "import" special form
├─ Line 252-271: apply_primitive() - Add "display", "write", "newline"
├─ Line 483-505: install_primitives() - Register new primitives
└─ Line 281-294: primitive_add() - Verify zero-arg case

tests/file_runner.rs
└─ Add new test: compare_patina_with_snapshots()
```

## Quick Start for Next Session

```rust
// src/eval/mod.rs - Add to apply_primitive()
"display" => self.primitive_display(args),
"write" => self.primitive_write(args),
"newline" => self.primitive_newline(args),

// Implement the primitives
fn primitive_display(&self, args: Vec<Value>) -> Result<Value, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }
    print!("{}", args[0].display_format()); // Add display_format() to Value
    Ok(Value::Unspecified)
}
```

## Testing as You Go

After each primitive is added:

```bash
# Quick test
cargo test --lib

# Integration test
cargo test --test comparison_test -- --nocapture

# Manual REPL test
cargo run
> (display "hello")
hello
> (newline)

> (write "hello")
"hello"
```

## Expected Timeline

- **Session 1 (30 min):** Basic I/O, tests runnable
- **Session 2 (30 min):** Complete I/O implementation
- **Session 3 (1 hour):** Edge cases and comparisons
- **Session 4+ (as needed):** Advanced features

**Total to run R7RS tests:** ~2-3 hours of focused coding

---

Have a great rest! When you're ready, we can start with Session 1 and make those R7RS test files run! 🚀
