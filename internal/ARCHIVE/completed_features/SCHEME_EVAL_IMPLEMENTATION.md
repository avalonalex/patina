# (scheme eval) Library Implementation Plan

**Status:** Planning
**Priority:** High (enables 4 more R7RS tests)
**Complexity:** Medium

## Overview

Implement the R7RS `(scheme eval)` library which provides runtime evaluation capabilities. This is one of the remaining gaps blocking full R7RS compliance.

### Current State

- **Chibi r7rs-tests.scm Section 6.12**: 0/4 tests passing (all error with "Undefined variable: eval")
- **Stub exists**: `crates/patina-runtime/src/stdlib/scheme_stubs.rs` has empty `build_scheme_eval()`

### R7RS Specification (Section 6.12)

The `(scheme eval)` library exports:
- `eval` - Evaluate an expression in a given environment
- `environment` - Create an environment from import sets

Related libraries (not in scope for this PR):
- `(scheme r5rs)`: `null-environment`, `scheme-report-environment`
- `(scheme repl)`: `interaction-environment`

## R7RS Requirements

### `environment` Procedure

```
(environment list1 ...)  →  environment-specifier
```

**Semantics:**
- Takes zero or more import sets (each is a list like `(scheme base)`)
- Returns a specifier for an environment containing bindings from those imports
- The resulting environment is **immutable**
- Import sets follow the same syntax as `import` declarations

**Examples:**
```scheme
(environment '(scheme base))           ; Environment with (scheme base) exports
(environment '(scheme base) '(scheme write))  ; Combined exports
(environment)                          ; Empty environment
```

### `eval` Procedure

```
(eval expr-or-def environment-specifier)  →  values
```

**Semantics:**
- If `expr-or-def` is an expression, evaluate it in the specified environment and return result(s)
- If `expr-or-def` is a definition, define the identifier(s) in the environment (error if immutable)
- The expression is evaluated in tail position within `eval`

**Examples:**
```scheme
(eval '(* 7 3) (environment '(scheme base)))
  → 21

(let ((f (eval '(lambda (f x) (f x x))
               (null-environment 5))))
  (f + 10))
  → 20

(eval '(define foo 32) (environment '(scheme base)))
  → error (environment is immutable)
```

## Implementation Design

### Phase 1: Add Value::EnvironmentSpecifier

**File:** `crates/patina-core/src/value.rs`

Add a new Value variant to represent first-class environments:

```rust
/// Environment specifier for eval (R7RS Section 6.12)
/// Wraps an Environment that can be passed to eval
/// The bool indicates whether the environment is mutable
EnvironmentSpecifier {
    env: Rc<Environment>,
    mutable: bool,
},
```

**Rationale:**
- Environments need to be first-class values for `environment` to return them
- The `mutable` flag distinguishes `environment` results (immutable) from `interaction-environment` (mutable)
- Using a struct variant keeps related data together

**Required changes:**
1. Add variant to `Value` enum
2. Add `type_name()` case: `"environment"`
3. Add `Display` implementation: `#<environment>`
4. Update exhaustive matches in:
   - `crates/patina-frontend/src/desugarer/mod.rs`
   - `crates/patina-tree-walker/src/eval/mod.rs`

### Phase 2: Implement Primitives

**File:** `crates/patina-tree-walker/src/eval/primitives/eval.rs` (new)

#### `environment` Implementation

```rust
/// (environment list1 ...) → environment-specifier
///
/// Creates an immutable environment from the given import sets.
fn primitive_environment(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    // 1. Create a fresh empty environment
    let env = Rc::new(Environment::new());

    // 2. Process each import set argument
    for arg in args {
        // Each arg should be a quoted list like (scheme base)
        let lib_name = extract_library_name(&arg)?;

        // Load the library
        let library = evaluator.load_library(&lib_name)
            .map_err(|e| EvalError::RuntimeError(format!("Cannot load library: {}", e)))?;

        // Install exports into the new environment
        for (name, value) in &library.exports {
            env.define(name.clone(), value.clone());
        }
    }

    // 3. Return as immutable environment specifier
    Ok(Value::EnvironmentSpecifier {
        env,
        mutable: false
    })
}

/// Extract library name from a quoted list like (scheme base) or '(scheme base)
fn extract_library_name(value: &Value) -> Result<Vec<String>, EvalError> {
    // Handle both (scheme base) and '(scheme base) forms
    // Convert to Vec<String> for load_library
}
```

#### `eval` Implementation

```rust
/// (eval expr-or-def environment-specifier) → values
///
/// Evaluates an expression in the specified environment.
fn primitive_eval(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 2, "eval")?;

    let expr = &args[0];
    let env_spec = &args[1];

    // Extract environment from specifier
    let (env, mutable) = match env_spec {
        Value::EnvironmentSpecifier { env, mutable } => (env.clone(), *mutable),
        _ => return Err(EvalError::TypeError(
            format!("eval: expected environment, got {}", env_spec.type_name())
        )),
    };

    // Check if this is a definition
    if is_definition(expr) {
        if !mutable {
            return Err(EvalError::RuntimeError(
                "eval: cannot define in immutable environment".to_string()
            ));
        }
        // Evaluate definition in the environment
        evaluator.eval_in_env(expr, &env)
    } else {
        // Evaluate expression in the environment
        evaluator.eval_in_env(expr, &env)
    }
}

/// Check if a value represents a definition form
fn is_definition(expr: &Value) -> bool {
    match expr {
        Value::Pair(pair) => {
            let (car, _) = &*pair.borrow();
            matches!(car,
                Value::Symbol(s) if s.as_ref() == "define" || s.as_ref() == "define-values"
            )
        }
        _ => false
    }
}
```

### Phase 3: Register Primitives

**File:** `crates/patina-tree-walker/src/eval/primitives/mod.rs`

```rust
mod eval;

// In register_all or install_primitives:
eval::register(registry);
```

**File:** `crates/patina-runtime/src/stdlib/scheme_eval.rs` (new)

Create a proper library builder (replacing the stub):

```rust
use crate::environment::Environment;
use crate::LibraryBuilder;
use std::rc::Rc;

pub fn build_scheme_eval(name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    // Primitives are registered via the evaluator's primitive registry
    // Just return the list of exports
    vec![
        "eval".to_string(),
        "environment".to_string(),
    ]
}
```

### Phase 4: Update Library Loading

**File:** `crates/patina-runtime/src/stdlib/mod.rs`

Register `(scheme eval)` with the Rust library loader:

```rust
// In register_rust_libraries or similar:
loader.register(
    vec!["scheme".to_string(), "eval".to_string()],
    scheme_eval::build_scheme_eval,
);
```

## Test Plan

### Unit Tests

**File:** `crates/patina-tests/tests/scheme_eval.rs` (new)

```rust
#[test]
fn test_environment_basic() {
    let result = eval_program(r#"
        (environment '(scheme base))
    "#);
    assert!(result.contains("environment"));
}

#[test]
fn test_eval_simple_expression() {
    let result = eval_program(r#"
        (eval '(* 7 3) (environment '(scheme base)))
    "#);
    assert_eq!(result, "21");
}

#[test]
fn test_eval_with_lambda() {
    let result = eval_program(r#"
        (let ((f (eval '(lambda (x) (* x x))
                       (environment '(scheme base)))))
          (f 5))
    "#);
    assert_eq!(result, "25");
}

#[test]
fn test_eval_define_in_immutable_env_errors() {
    let result = eval_program(r#"
        (eval '(define foo 32) (environment '(scheme base)))
    "#);
    assert!(result.contains("ERROR"));
}

#[test]
fn test_environment_multiple_libraries() {
    let result = eval_program(r#"
        (eval '(+ (expt 2 10) (inexact (sin 0)))
              (environment '(scheme base) '(scheme inexact)))
    "#);
    assert_eq!(result, "1024.0");
}

#[test]
fn test_environment_empty() {
    // Empty environment should work but have no bindings
    let result = eval_program(r#"
        (eval '(if #t 1 2) (environment))
    "#);
    // 'if' is a special form, should work even in empty env
    assert_eq!(result, "1");
}
```

### Chibi Compatibility Tests

The following tests from `r7rs-tests.scm` should pass after implementation:

```scheme
;; Test 1: eval with null-environment (requires scheme r5rs - defer)
;; (test 20
;;     (let ((f (eval '(lambda (f x) (f x x)) (null-environment 5))))
;;       (f + 10)))

;; Test 2: eval with environment
(test 1024 (eval '(expt 2 10) (environment '(scheme base))))

;; Test 3: eval with scheme inexact
(test 0.0 (inexact (eval '(sin 0) (environment '(scheme inexact)))))

;; Test 4: eval with multiple libraries
(test 1024.0 (eval '(+ (expt 2 10) (inexact (sin 0)))
                   (environment '(scheme base) '(scheme inexact))))
```

**Note:** Test 1 requires `null-environment` from `(scheme r5rs)` which is out of scope.

## Implementation Checklist

### Phase 1: Value Type
- [ ] Add `EnvironmentSpecifier` variant to `Value` enum
- [ ] Implement `type_name()` for new variant
- [ ] Implement `Display` for new variant
- [ ] Update exhaustive matches in desugarer
- [ ] Update exhaustive matches in evaluator

### Phase 2: Primitives
- [ ] Create `eval.rs` primitives module
- [ ] Implement `extract_library_name` helper
- [ ] Implement `environment` primitive
- [ ] Implement `is_definition` helper
- [ ] Implement `eval` primitive
- [ ] Register primitives in `mod.rs`

### Phase 3: Library
- [ ] Create `scheme_eval.rs` library builder (or update stub)
- [ ] Register library with loader
- [ ] Verify library exports correct names

### Phase 4: Testing
- [ ] Create `scheme_eval.rs` test file
- [ ] Add basic environment tests
- [ ] Add eval expression tests
- [ ] Add eval lambda tests
- [ ] Add immutability error tests
- [ ] Add multiple library tests
- [ ] Run chibi compatibility tests
- [ ] Update CLAUDE.md with new test counts

### Phase 5: Documentation
- [ ] Update `docs/FEATURE_STATUS.md`
- [ ] Archive this PRD to `internal/ARCHIVE/completed_features/`

## Edge Cases and Considerations

### Special Forms in eval

Special forms like `if`, `lambda`, `quote` should work in any environment because they're handled by the evaluator, not looked up in the environment. Need to verify this works correctly.

### Macro Handling

Macros defined in imported libraries should be available in the environment. The current library loading already handles this, but need to verify macros work correctly through `eval`.

### Error Handling

- Invalid import set syntax → clear error message
- Library not found → clear error message
- Definition in immutable environment → error per R7RS
- Wrong number of arguments → standard arity error

### Future: null-environment and scheme-report-environment

These are in `(scheme r5rs)` and require:
- `null-environment 5` - Only syntactic keywords (if, lambda, quote, etc.)
- `scheme-report-environment 5` - Full R5RS bindings

Implementation approach:
- Create a hardcoded list of R5RS bindings
- For `null-environment`, create env with only special forms
- For `scheme-report-environment`, load `(scheme base)` minus R7RS-only additions

This is out of scope for the current PR but noted for future work.

## Dependencies

- Existing library loading infrastructure (`load_library`, `process_import_set`)
- Existing evaluation infrastructure (`eval_in_env`)
- Environment type from `patina-core`

## Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| Circular dependency: eval primitive needs evaluator | Use existing pattern from other primitives that access evaluator |
| Environment mutability confusion | Clear `mutable` flag, good error messages |
| Macro hygiene in evaled code | Use existing macro expansion infrastructure |

## Success Criteria

1. All 4 tests in Section 6.12 that don't require `(scheme r5rs)` pass
2. `eval` and `environment` work correctly with all existing libraries
3. Immutability is properly enforced
4. No regressions in existing tests
