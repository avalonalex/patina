# CPS Lambda Evaluation Fix

**Status**: In Progress (PR split into phases)
**Priority**: High - Blocks call/cc support for chibi tests
**Last Updated**: 2025-12-11

## Current State Summary

The CPS evaluator is partially implemented with the following status:

### What Works
- [x] `Procedure::CpsLambda` variant in patina-core
- [x] CPS lambdas store actual CPS body (`CpsExpr`)
- [x] Trampoline pattern avoids Rust stack overflow
- [x] Basic unit tests pass (literal, variable, primop, if)
- [x] `CpsExpr::Apply` for proper apply semantics
- [x] **Quasiquote**: CPS transformer handles quasiquote correctly
- [x] Chibi tests run without panic (89.7% passing, 1036/1155)

### What's Broken/Incomplete
- [ ] **Library lambda mode mixing**: Libraries loaded in non-CPS mode create regular Lambdas that error in CPS mode
- [ ] **Continuation predicates**: `(procedure? <continuation>)` returns `#f` instead of `#t`
- [ ] **Dynamic-wind**: Not yet implemented
- [ ] **call/cc with library functions**: `for-each` in call/cc example fails due to mode mixing

### Key Error Messages
1. `CPS lambda can only be invoked in CPS evaluation mode` - Library functions calling CPS lambdas
2. `Undefined variable: patina.internal.control/dynamic-wind` - dynamic-wind not implemented

---

## Phased Implementation Plan

The work is split into smaller PRs for easier review and to maintain a working codebase.

### PR 1: Stabilize Current CPS Foundation (This PR)

**Scope**: Get the CPS evaluator to a stable, testable state without attempting full R7RS compliance.

**Tasks**:
1. [x] Fix `restore_cont_bindings` missing function
2. [x] Add quasiquote support to CPS transformer
3. [x] Code compiles without errors
4. [x] No panics on common Scheme expressions (89.7% of chibi tests run without panic)
5. [x] Update PRD documentation

**Success Criteria**:
- [x] Code compiles without errors
- [x] No panics on common Scheme expressions
- [x] Chibi test suite runs to completion (even if not all pass)

**Current Status**:
- 89.7% passing (1036/1155)
- 2.4% failing (28)
- 7.9% errors (91) - mostly library mode mixing issues

**Non-Goals for PR 1**:
- Full R7RS chibi test suite passing
- Library mode-aware loading
- Delimited continuations (shift/reset)
- dynamic-wind implementation

---

### PR 2: Fix Library Lambda Mode Mixing

**Scope**: Ensure CPS lambdas can interoperate with library functions.

**Root Cause Analysis**:
The issue is that `call-with-values` (a Rust primitive in `primitives/values.rs`) uses
`evaluator.apply()` which is the direct evaluator. When user code creates CPS lambdas
(because CPS mode is enabled) and passes them as producers/consumers to `call-with-values`,
the direct evaluator can't invoke them.

Affected operations:
- `let-values` / `let*-values` macros (use `call-with-values`)
- Any macro using `=>` syntax in `case`/`cond`
- `delay`/`force` (lazy evaluation)
- Other primitives that call user-provided procedures

**Option A: Make primitives CPS-aware** (Complex but Complete)
- Modify `apply` in `Evaluator` to detect CPS lambdas and handle them properly
- Requires significant refactoring of the evaluation architecture

**Option B: Implement critical functions in CPS evaluator** (Targeted)
- Add CPS-native handling for `call-with-values` in `cps_eval.rs`
- Add CPS-native handling for `force` (lazy evaluation)
- Keep Rust primitives for non-CPS mode compatibility

**Option C: Always use CPS mode** (Simplest, breaking change)
- Remove non-CPS evaluation path
- All lambdas are CPS lambdas
- Libraries would need to be reloaded/recompiled

**Recommendation**: Option B for PR 2, consider Option C for Phase 2.

**Files to modify**:
- `crates/patina-tree-walker/src/eval/cps_eval.rs` - Add `call-with-values` handling
- `lib/scheme/base/binding.scm` - May need adjustments for CPS compatibility

**Success Criteria**:
- `(let-values (((a b) (values 1 2))) (+ a b))` works in CPS mode
- `(force (delay (+ 1 2)))` works in CPS mode

---

### PR 3: Full call/cc Implementation

**Scope**: Complete call/cc with continuation escape, reuse, and dynamic-wind.

**Tasks**:
1. Verify continuation capture stores all necessary state
2. Test continuation escape: `(+ 1 (call/cc (lambda (k) (+ 100 (k 2)))))` => 3
3. Test reusable continuations
4. Implement and test `dynamic-wind` interaction

**Success Criteria**:
- All call/cc tests from chibi r7rs-tests.scm pass
- `dynamic-wind` properly interacts with continuations

---

### PR 4: Delimited Continuations (shift/reset)

**Scope**: Add support for delimited continuations per CALLCC_IMPLEMENTATION.md.

**Tasks**:
1. Implement `make-continuation-prompt-tag`
2. Implement `call-with-continuation-prompt`
3. Implement `abort-current-continuation`
4. Implement `call-with-composable-continuation`
5. Create `(patina control)` library with `reset`/`shift` macros

**Success Criteria**:
- Basic shift/reset tests pass
- Can implement generators using shift/reset

---

### PR 5: Exception Handling

**Scope**: Implement R7RS exception handling using delimited continuations.

**Tasks**:
1. Implement `guard` syntax
2. Implement `raise` and `raise-continuable`
3. Implement `with-exception-handler`

**Success Criteria**:
- R7RS exception tests pass
- `guard` properly catches exceptions

---

## Known Limitations

### Library Lambdas (Fixed in PR 2)
Higher-order functions from Scheme libraries (map, for-each) don't work with CpsLambdas:
```scheme
(map (lambda (x) (* x 2)) '(1 2 3))  ; ERROR in CPS mode
```

**Root cause**: Libraries are loaded before CPS mode is enabled, so library lambdas are regular Lambdas (not CpsLambdas). When a regular Lambda calls `apply` on a CpsLambda, the direct evaluator is used, which can't handle CpsLambdas.

**Workaround**: Use explicit recursion instead of map/for-each, or implement custom versions in user code.

## Problem Statement

The CPS evaluator creates lambdas during CPS transformation, but when applied, these lambdas delegate to the direct evaluator which doesn't understand CPS semantics. This breaks `call/cc` because:

1. CPS lambdas expect a continuation parameter
2. The direct evaluator doesn't know about continuation parameters
3. When call/cc captures a continuation, lambdas can't invoke it properly

### Current Broken Code

```rust
// cps_eval.rs line 600-613
Procedure::Lambda { env, .. } => {
    // For now, delegate all lambda applications to the regular evaluator
    // TODO: Implement proper CPS lambda handling
    let result = self.apply_regular_proc(&p, args)?;
    self.invoke_continuation(cont, result, ...)
}

// cps_eval.rs line 522
fn make_cps_closure(...) -> Value {
    // Uses placeholder body - BROKEN
    let placeholder_body = vec![CoreExpr::Literal(...)];
    Value::Procedure(Rc::new(Procedure::Lambda {
        body: placeholder_body,  // CPS body lost!
        ...
    }))
}
```

## Solution: Add CpsLambda Procedure Type

### Phase 1: Add Type to patina-core

**File**: `crates/patina-core/src/value.rs`

```rust
pub enum Procedure {
    Primitive { ... },
    Lambda {
        body: Vec<CoreExpr>,  // Direct-style body
        ...
    },
    // NEW: CPS-style lambda
    CpsLambda {
        params: Vec<ScopedParam>,
        variadic: Option<ScopedParam>,
        cont_param: Rc<str>,      // The continuation parameter name
        body: Rc<CpsExpr>,        // CPS expression body (not CoreExpr!)
        env: Rc<Environment>,
        binding_scope: Option<ScopeId>,
    },
}
```

This requires importing `CpsExpr` into `patina-core`, which may require reorganizing the crate dependency graph or putting CpsExpr in a shared location.

### Phase 2: Update CPS Lambda Creation

**File**: `crates/patina-tree-walker/src/eval/cps_eval.rs`

```rust
fn make_cps_closure(
    &self,
    params: &[CpsParam],
    variadic: Option<&CpsParam>,
    cont_param: &Rc<str>,
    body: &Rc<CpsExpr>,
    env: &Rc<Environment>,
) -> Value {
    let scoped_params: Vec<ScopedParam> = params
        .iter()
        .map(|p| ScopedParam {
            name: p.name.clone(),
            scopes: p.scopes.clone(),
        })
        .collect();

    Value::Procedure(Rc::new(Procedure::CpsLambda {
        params: scoped_params,
        variadic: variadic.map(|p| ScopedParam {
            name: p.name.clone(),
            scopes: p.scopes.clone(),
        }),
        cont_param: cont_param.clone(),
        body: body.clone(),  // Store actual CPS body!
        env: env.clone(),
        binding_scope: None,
    }))
}
```

### Phase 3: Handle CpsLambda in apply_cps

**File**: `crates/patina-tree-walker/src/eval/cps_eval.rs`

```rust
fn apply_cps(...) -> Result<Value, EvalError> {
    match proc {
        Value::Procedure(p) => match p.as_ref() {
            // NEW: Handle CPS lambdas properly
            Procedure::CpsLambda {
                params,
                variadic,
                cont_param,
                body,
                env: lambda_env,
                binding_scope: _,
            } => {
                // Create new environment for the lambda
                let new_env = Rc::new(Environment::with_parent(lambda_env.clone()));

                // Bind fixed parameters
                for (param, arg) in params.iter().zip(args.iter()) {
                    new_env.define(param.name.to_string(), arg.clone());
                }

                // Bind variadic parameter if present
                if let Some(variadic_param) = variadic {
                    let rest_args: Vec<Value> = args[params.len()..].to_vec();
                    let rest_list = Value::list_from_vec(rest_args);
                    new_env.define(variadic_param.name.to_string(), rest_list);
                }

                // CRITICAL: Bind continuation parameter
                let cont_value = self.reify_continuation(&cont, &dynamic_winds);
                new_env.define(cont_param.to_string(), cont_value);

                // Evaluate CPS lambda body - it will call the continuation
                self.eval_step(&body, new_env, cont_env, prompt_stack, dynamic_winds)
            }

            // Direct-style lambda: delegate to direct evaluator
            Procedure::Lambda { env, .. } => {
                let result = self.apply_regular_proc(&p, args)?;
                self.invoke_continuation(
                    cont, result, env.clone(), cont_env, prompt_stack, dynamic_winds
                )
            }

            Procedure::Primitive { .. } => { ... }
        },

        Value::Continuation(k) => { ... }

        _ => Err(EvalError::NotAProcedure(...))
    }
}
```

## Architecture Considerations

### Crate Dependency Issue

Currently:
```
patina-core (defines Value, Procedure)
    ↑
patina-ir (defines CpsExpr)
    ↑
patina-tree-walker (defines CpsEvaluator)
```

Adding `CpsLambda` with `body: Rc<CpsExpr>` to `patina-core` creates a cycle:
```
patina-core → patina-ir (CpsExpr)
patina-ir → patina-core (Value)  // Circular!
```

### Solutions

**Option A**: Move `CpsExpr` to `patina-core`
- Simple but increases core crate size
- Makes sense if CpsExpr is fundamental to the Value type

**Option B**: Store CPS body as opaque type
```rust
// In patina-core
pub struct CpsBody(pub Rc<dyn std::any::Any>);

pub enum Procedure {
    CpsLambda {
        ...
        body: CpsBody,  // Opaque, downcasted in tree-walker
    }
}
```

**Option C**: Use trait object for procedure body
```rust
pub trait LambdaBody: Debug + Clone { ... }
```

**Recommended**: Option A (move CpsExpr to patina-core) for simplicity. CpsExpr is already a core IR type.

## Test Cases

### Basic call/cc
```scheme
(call/cc (lambda (k) (k 42)))  ; => 42
(call/cc (lambda (k) 42))      ; => 42 (k not invoked)
(+ 1 (call/cc (lambda (k) (k 2))))  ; => 3
```

### Continuation escape
```scheme
(+ 1 (call/cc (lambda (k) (+ 100 (k 2)))))  ; => 3 (100 is skipped)
```

### Reusable continuation
```scheme
(define k #f)
(+ 1 (call/cc (lambda (c) (set! k c) 2)))  ; => 3
(k 10)  ; => 11
```

## Files to Modify

1. `crates/patina-core/src/value.rs` - Add CpsLambda variant
2. `crates/patina-core/src/lib.rs` - Export CpsExpr if moved here
3. `crates/patina-ir/src/cps_expr.rs` - Possibly move to patina-core
4. `crates/patina-tree-walker/src/eval/cps_eval.rs` - Update make_cps_closure and apply_cps
5. `crates/patina-tree-walker/src/eval/application.rs` - Handle CpsLambda in direct evaluator (error)

## Estimated Changes

- ~100 LOC in patina-core (new Procedure variant + Display impl)
- ~150 LOC in cps_eval.rs (proper lambda handling)
- ~50 LOC miscellaneous (imports, tests)

Total: ~300 LOC, well-defined scope.
