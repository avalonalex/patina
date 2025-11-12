# Implementing `call/cc` in Patina's Tree-Walking Interpreter

**Last Updated**: 2025-11-11
**Status**: Design Document (Not Yet Implemented)

## Overview

This document details how to implement `call-with-current-continuation` (and its abbreviation `call/cc`) in Patina's tree-walking interpreter. The implementation captures the current execution state and packages it as a first-class procedure (the continuation) that can be invoked to "return" to that point with a value.

## R7RS Requirements

From R7RS-small section 6.10 (Control features):

- **Procedure**: `(call-with-current-continuation proc)`
- **Procedure**: `(call/cc proc)` - abbreviation for the above

> Captures the current continuation and passes it as an argument to `proc`. When the continuation is later invoked with a value, execution immediately returns to the point where `call/cc` was called, as if `call/cc` had returned that value.

**Key properties:**
1. Continuations are first-class values (can be stored, passed around)
2. Continuations can be invoked multiple times
3. Continuations have indefinite extent (outlive their dynamic scope)
4. Invoking a continuation abandons the current computation

## Patina's Current Architecture

### Trampoline-Based TCO

Patina uses a trampoline pattern for tail call optimization (`crates/patina-tree-walker/src/eval/mod.rs:177-219`):

```rust
pub enum EvalResult {
    Value(Value),                                    // Final result
    TailCall { expr: Value, env: Rc<Environment> },  // Tail call to continue
    TailCallPrimitive { proc: Value, args: Vec<Value> }, // Primitive tail call
}

pub fn eval(&self, expr: &Value) -> Result<Value, EvalError> {
    let mut current_expr = expr.clone();
    let mut current_env = self.global_env.clone();

    loop {
        match self.eval_step(&current_expr, &current_env)? {
            EvalResult::Value(v) => return Ok(v),
            EvalResult::TailCall { expr, env } => {
                current_expr = expr;
                current_env = env;
            }
            // ... handle TailCallPrimitive
        }
    }
}
```

**Implication**: The Rust call stack only grows to depth of deepest *non-tail* recursion. Tail calls are handled iteratively. However, we don't explicitly maintain a Scheme call stack - the continuation state is implicit in Rust's stack and the trampoline state.

### Value Representation

The `Procedure` enum already has a `Continuation` variant (`crates/patina-runtime/src/value.rs:86`):

```rust
pub enum Procedure {
    Primitive { name: &'static str, arity: Arity },
    Lambda { params: Vec<String>, variadic: Option<String>, body: Vec<Value>, env: Rc<Environment> },
    Continuation,  // ← Currently unused placeholder
}
```

**Problem**: The current `Continuation` variant has no state attached. We need to capture execution state.

## Implementation Strategy

### Option 1: Explicit Continuation Stack (Recommended)

**Approach**: Maintain an explicit continuation stack that captures what needs to happen after each expression.

#### 1.1 Enhanced Value Representation

```rust
// In crates/patina-runtime/src/value.rs

pub enum Procedure {
    Primitive { name: &'static str, arity: Arity },
    Lambda {
        params: Vec<String>,
        variadic: Option<String>,
        body: Vec<Value>,
        env: Rc<Environment>
    },
    Continuation {
        // Captured continuation stack
        stack: Rc<Vec<Continuation>>,
        // Captured environment at call/cc point
        env: Rc<Environment>,
    },
}

// Represents one continuation frame - what to do next
#[derive(Debug, Clone)]
pub enum Continuation {
    /// Return from current call to parent
    Return,

    /// Evaluate remaining expressions in sequence
    Sequence {
        remaining: Vec<Value>,
        env: Rc<Environment>,
    },

    /// After evaluating operator, evaluate operands
    EvalOperands {
        unevaluated_args: Vec<Value>,
        env: Rc<Environment>,
    },

    /// After evaluating all operands, apply operator
    Apply {
        operator: Value,
        evaluated_args: Vec<Value>,
        in_tail_position: bool,
    },

    /// After evaluating condition, evaluate consequent or alternative
    If {
        consequent: Value,
        alternative: Option<Value>,
        env: Rc<Environment>,
    },

    /// After evaluating test, check cases
    Case {
        clauses: Vec<(Vec<Value>, Vec<Value>)>,  // (datums, body)
        else_clause: Option<Vec<Value>>,
        env: Rc<Environment>,
    },

    /// Set! - after evaluating value, assign to variable
    Set {
        name: String,
        env: Rc<Environment>,
    },

    /// Define - after evaluating value, bind in environment
    Define {
        name: String,
        env: Rc<Environment>,
    },
}
```

**Rationale**: Each continuation frame represents a "what to do next" instruction. When we capture a continuation with `call/cc`, we clone the current continuation stack. When we invoke a continuation, we replace the current stack with the captured one.

#### 1.2 Modified Evaluator

```rust
// In crates/patina-tree-walker/src/eval/mod.rs

pub struct Evaluator {
    pub(in crate::eval) global_env: Rc<Environment>,
    pub(crate) debug: Rc<DebugConfig>,
    pub(crate) library_registry: RefCell<LibraryRegistry>,
    pub(crate) loader_registry: RefCell<LibraryLoaderRegistry>,
    // NEW: Thread-local continuation stack for current evaluation
    continuation_stack: RefCell<Vec<Continuation>>,
}

impl Evaluator {
    pub fn eval(&self, expr: &Value) -> Result<Value, EvalError> {
        // Clear continuation stack for new top-level evaluation
        self.continuation_stack.borrow_mut().clear();

        let mut current_value = None;
        let mut current_expr = Some(expr.clone());
        let mut current_env = self.global_env.clone();

        loop {
            if let Some(expr) = current_expr.take() {
                // Evaluate the expression
                match self.eval_step(&expr, &current_env)? {
                    EvalResult::Value(v) => {
                        current_value = Some(v);
                        // Expression evaluated to value, process continuation
                    }
                    EvalResult::TailCall { expr, env } => {
                        // Tail call - continue with new expression
                        current_expr = Some(expr);
                        current_env = env;
                        continue;
                    }
                    EvalResult::TailCallPrimitive { proc, args } => {
                        // Handle primitive tail call
                        // ... existing logic ...
                    }
                }
            }

            // Process the continuation stack
            if let Some(value) = current_value.take() {
                match self.continuation_stack.borrow_mut().pop() {
                    None => {
                        // No more continuations - we're done
                        return Ok(value);
                    }
                    Some(cont) => {
                        // Process this continuation with the value
                        match self.apply_continuation(cont, value, &mut current_expr, &mut current_env)? {
                            ContResult::Continue(v) => {
                                current_value = Some(v);
                            }
                            ContResult::Eval { expr, env } => {
                                current_expr = Some(expr);
                                current_env = env;
                            }
                        }
                    }
                }
            }
        }
    }

    fn apply_continuation(
        &self,
        cont: Continuation,
        value: Value,
        expr_out: &mut Option<Value>,
        env_out: &mut Rc<Environment>,
    ) -> Result<ContResult, EvalError> {
        match cont {
            Continuation::Return => {
                // Just pass value up
                Ok(ContResult::Continue(value))
            }

            Continuation::Sequence { remaining, env } => {
                if remaining.is_empty() {
                    Ok(ContResult::Continue(value))
                } else {
                    // Push continuation for rest of sequence
                    if remaining.len() > 1 {
                        self.continuation_stack.borrow_mut().push(
                            Continuation::Sequence {
                                remaining: remaining[1..].to_vec(),
                                env: env.clone(),
                            }
                        );
                    }
                    // Evaluate next expression
                    Ok(ContResult::Eval {
                        expr: remaining[0].clone(),
                        env,
                    })
                }
            }

            Continuation::Apply { operator, evaluated_args, in_tail_position } => {
                // All operands evaluated, now apply
                // Don't push a continuation if in tail position
                if !in_tail_position {
                    self.continuation_stack.borrow_mut().push(Continuation::Return);
                }
                match self.apply(operator, evaluated_args, in_tail_position)? {
                    EvalResult::Value(v) => Ok(ContResult::Continue(v)),
                    EvalResult::TailCall { expr, env } => {
                        Ok(ContResult::Eval { expr, env })
                    }
                    // ... handle TailCallPrimitive
                }
            }

            // ... implement other continuation types
        }
    }
}

enum ContResult {
    Continue(Value),           // Continue with this value
    Eval { expr: Value, env: Rc<Environment> }, // Evaluate this expression
}
```

#### 1.3 Implementing `call/cc`

```rust
// In crates/patina-tree-walker/src/eval/special_forms.rs

fn handle_callcc(&self, args: &[Value], env: &Rc<Environment>) -> Result<EvalResult, EvalError> {
    if args.len() != 1 {
        return Err(EvalError::WrongArity {
            expected: "1".to_string(),
            actual: args.len(),
        });
    }

    // Evaluate the procedure argument
    let proc = self.eval_in_env(&args[0], env)?;

    // Capture current continuation
    let captured_stack = self.continuation_stack.borrow().clone();
    let captured_env = env.clone();

    let continuation = Value::Procedure(Procedure::Continuation {
        stack: Rc::new(captured_stack),
        env: captured_env,
    });

    // Apply proc to the continuation
    // This is NOT in tail position - we need to return to the caller
    self.apply(proc, vec![continuation], false)
}
```

#### 1.4 Invoking a Continuation

```rust
// In crates/patina-tree-walker/src/eval/application.rs

pub(super) fn apply(
    &self,
    proc: Value,
    args: Vec<Value>,
    in_tail_position: bool,
) -> Result<super::EvalResult, EvalError> {
    match proc {
        Value::Procedure(Procedure::Primitive { name, arity }) => {
            // ... existing primitive handling
        }

        Value::Procedure(Procedure::Lambda { .. }) => {
            // ... existing lambda handling
        }

        Value::Procedure(Procedure::Continuation { stack, env }) => {
            // Continuation invocation!
            if args.len() != 1 {
                return Err(EvalError::WrongArity {
                    expected: "1".to_string(),
                    actual: args.len(),
                });
            }

            let return_value = args[0].clone();

            // Replace current continuation stack with captured one
            *self.continuation_stack.borrow_mut() = (*stack).clone();

            // Return the value - it will be processed by the restored continuation
            Ok(super::EvalResult::Value(return_value))
        }

        _ => Err(EvalError::NotAProcedure(format!("{}", proc))),
    }
}
```

### Option 2: CPS Transformation (Not Recommended)

**Approach**: Transform all code to Continuation-Passing Style where every function takes an explicit continuation parameter.

**Pros**:
- Continuations are naturally first-class
- Elegant from a theoretical perspective

**Cons**:
- Requires complete rewrite of evaluator
- Breaks existing TCO implementation
- Complex transformation of special forms
- Performance overhead on all evaluations

**Verdict**: Too invasive for Patina's current architecture.

### Option 3: Stack Copying (Simpler Alternative)

**Approach**: For a tree-walker, we could try to capture the Rust call stack state, but this is not possible in safe Rust. We'd need to:

1. Manually track evaluation context in a Rust-side stack
2. Deep-copy that stack on `call/cc`
3. Restore it on continuation invocation

**Problem**: This is essentially Option 1, but we'd still need to represent continuation frames explicitly.

## Implementation Plan

### Phase 1: Foundation (2-3 days)

1. **Define Continuation Types** (`crates/patina-runtime/src/value.rs`)
   - [ ] Add `Continuation` enum with frame types
   - [ ] Update `Procedure::Continuation` to store state
   - [ ] Implement `Clone` for continuation types
   - [ ] Update `Display` for continuations (display as `#<continuation>`)

2. **Add Continuation Stack** (`crates/patina-tree-walker/src/eval/mod.rs`)
   - [ ] Add `continuation_stack: RefCell<Vec<Continuation>>` to `Evaluator`
   - [ ] Initialize in `new()`

### Phase 2: Evaluator Restructuring (3-5 days)

3. **Modify Main Evaluation Loop**
   - [ ] Update `eval()` to process continuation stack
   - [ ] Implement `apply_continuation()` for each continuation type
   - [ ] Ensure TCO still works with continuation stack

4. **Update Special Forms to Use Continuations**
   - [ ] `if` - push `Continuation::If` before evaluating condition
   - [ ] `begin` - push `Continuation::Sequence`
   - [ ] `set!` - push `Continuation::Set`
   - [ ] `define` - push `Continuation::Define`
   - [ ] `cond` - push appropriate continuation
   - [ ] Application - push `Continuation::EvalOperands` then `Continuation::Apply`

### Phase 3: call/cc Implementation (1-2 days)

5. **Implement `call-with-current-continuation`**
   - [ ] Add `handle_callcc()` in `special_forms.rs`
   - [ ] Capture continuation stack and environment
   - [ ] Create continuation procedure
   - [ ] Apply user procedure to continuation

6. **Implement Continuation Invocation**
   - [ ] Add `Procedure::Continuation` case to `apply()`
   - [ ] Replace continuation stack with captured stack
   - [ ] Return argument value

7. **Add Primitives**
   - [ ] Register `call-with-current-continuation` primitive
   - [ ] Register `call/cc` as alias

### Phase 4: Testing (2-3 days)

8. **Write Tests** (`crates/patina-tests/tests/continuations.rs`)
   - [ ] Basic capture and invoke
   - [ ] Non-local exit (escape from nested computation)
   - [ ] Multiple invocations of same continuation
   - [ ] Continuations outliving their dynamic scope
   - [ ] Backtracking (like chibi's test08-callcc.scm)
   - [ ] Interaction with TCO
   - [ ] Interaction with exceptions (once implemented)

9. **Run Compliance Tests**
   - [ ] Add call/cc tests to `compliance/control.rs`
   - [ ] Run chibi r7rs-tests.scm and verify call/cc tests pass

### Phase 5: Optimization (Optional)

10. **Performance Improvements**
    - [ ] Use `Rc` for sharing continuation frames instead of cloning
    - [ ] Implement copy-on-write for continuation stack
    - [ ] Profile and optimize continuation application

## Key Challenges

### 1. Interaction with TCO

**Problem**: Tail calls currently use `EvalResult::TailCall` to avoid pushing Rust stack frames. With continuations, we need to ensure tail calls still don't push *continuation* frames.

**Solution**: When in tail position, don't push a `Continuation::Return`. The continuation stack should only grow for non-tail calls.

### 2. Continuation Extent

**Problem**: Continuations must have indefinite extent - they can outlive the dynamic scope where they were created.

**Solution**: Using `Rc<Vec<Continuation>>` means continuations are heap-allocated and reference-counted. They'll live as long as needed.

### 3. Multiple Invocations

**Problem**: A continuation can be called multiple times, and each invocation should return to the same point.

**Solution**: Store continuation stack in `Rc` (not `Box`). When invoked, clone the stack so it can be invoked again.

```rust
// In continuation application:
*self.continuation_stack.borrow_mut() = (*stack).clone();
```

### 4. Interaction with Exceptions

**Problem**: Once we implement `guard` and `raise`, continuations need to properly interact with exception handlers.

**Solution**: Exception handlers should be represented as special continuation frames:

```rust
enum Continuation {
    // ... existing variants
    ExceptionHandler {
        handler: Value,  // The guard handler procedure
        env: Rc<Environment>,
    },
}
```

When an exception is raised, unwind the continuation stack until we find an `ExceptionHandler` frame.

### 5. Space Complexity

**Problem**: Capturing the entire continuation stack on every `call/cc` could be expensive.

**Mitigation**:
- Use structural sharing with `Rc` where possible
- Most Scheme programs don't use `call/cc` heavily
- This is an acceptable tradeoff for correctness in a tree-walker

## Testing Strategy

### Unit Tests

```scheme
;; Basic capture and return
(define k #f)
(define result
  (call/cc (lambda (cont)
             (set! k cont)
             42)))
;; result should be 42

(k 99)  ;; Should "return" 99 from the call/cc

;; Non-local exit
(define (search-list lst)
  (call/cc (lambda (return)
             (for-each (lambda (x)
                         (if (= x 5)
                             (return 'found)))
                       lst)
             'not-found)))

(search-list '(1 2 3 4 5 6))  ;; => found
(search-list '(1 2 3 4))      ;; => not-found

;; Backtracking (from chibi test08-callcc.scm)
;; Generate Pythagorean triples
```

### Integration Tests

Run chibi-scheme's `tests/basic/test08-callcc.scm` which implements a backtracking search for Pythagorean triples using continuations.

### Compliance Tests

R7RS section 6.10 specifies `call/cc` behavior. Verify:
- ✅ Continuation captures current state
- ✅ Invoking continuation returns to capture point
- ✅ Continuations can be invoked multiple times
- ✅ Continuations have indefinite extent
- ✅ Continuations accept exactly one argument

## References

- **R7RS Spec**: Section 6.10 (Control features)
- **Chibi Implementation**: `~/Project/reference/chibi-scheme/vm.c:1235-1260`
- **Chibi Tests**: `~/Project/reference/chibi-scheme/tests/basic/test08-callcc.scm`
- **Academic**: "Representing Control in the Presence of First-Class Continuations" (Hieb, Dybvig, Bruggeman)

## Alternative: Delimited Continuations (Future)

Delimited continuations (`call/cc` with explicit prompts) are more powerful and efficient. Consider for Phase 2+ if we want to support libraries like `scheme.generator`:

- `call-with-prompt`
- `abort-to-prompt`
- `make-continuation-prompt-tag`

These are not in R7RS-small but are useful for advanced control flow.

## Estimated Timeline

- **Total effort**: 8-13 days
- **Complexity**: High (8/10)
- **Priority**: Medium (required for R7RS compliance, but not used by most programs)
- **Dependencies**: None (can be implemented independently)

## Success Criteria

- [ ] `call/cc` and `call-with-current-continuation` primitives implemented
- [ ] Continuations can be captured and invoked
- [ ] All unit tests pass
- [ ] Chibi's test08-callcc.scm passes
- [ ] R7RS compliance tests for call/cc pass
- [ ] No regression in existing TCO tests
- [ ] Documentation updated in FEATURE_STATUS.md
