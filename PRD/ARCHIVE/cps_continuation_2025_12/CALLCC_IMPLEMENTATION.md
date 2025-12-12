# Implementing Continuations in Patina's Tree-Walking Interpreter

**Last Updated**: 2025-12-12
**Status**: ✅ Core Complete (call/cc, dynamic-wind, exceptions working; shift/reset pending)

## Implementation Status (2025-12-12)

### ✅ Completed

1. **CPS Infrastructure** - Full CPS transformation and evaluation
   - `CpsExpr` IR in `patina-core/src/cps_expr.rs`
   - CPS transformer in `patina-ir/src/cps_transform.rs`
   - CPS evaluator in `patina-tree-walker/src/eval/cps_eval.rs`
   - Trampoline pattern for stack safety

2. **Basic call/cc** - Working implementation
   - `(call-with-current-continuation proc)` and `(call/cc proc)` work
   - Continuations satisfy `procedure?` predicate
   - Continuation invocation properly aborts current computation
   - Works with library functions like `map` and `for-each` (all library code uses CPS)

3. **CPS-Only Evaluation Mode** (NEW - 2025-12-12)
   - CPS is now the default and only evaluation mode
   - All library code is loaded using CPS evaluation → creates `Procedure::CpsLambda`
   - `--cps` flag removed from REPL
   - `EvalMode` enum removed
   - No need for thread-local escape mechanism for library interop (still used for continuation invocation)

4. **dynamic-wind** - Full implementation with continuation re-entry
   - Before/after thunks properly tracked per dynamic extent
   - Continuation capture inside dynamic-wind works correctly
   - Re-invoking captured continuation re-runs before thunk
   - Exiting via continuation runs after thunk
   - Nested dynamic-wind with continuation capture/re-entry works
   - Classic R7RS test passes: `(connect talk1 disconnect connect talk2 disconnect)`

5. **Exception Handling** - Full implementation (NEW - 2025-12-12)
   - `guard`, `raise`, `raise-continuable` all implemented
   - `with-exception-handler` implemented
   - All R7RS exception tests pass (26/26)

6. **Test Results**
   - Chibi r7rs-tests.scm: 1159/1159 passing (100%)
   - All internal tests pass (~1400 tests)
   - `(call-with-current-continuation procedure?)` → `#t`
   - `(call-with-current-continuation (lambda (exit) (map ... (exit x) ...) #t))` → works
   - dynamic-wind with call/cc re-entry works correctly
   - Exception handling with `guard` and `raise` works correctly

### 🚧 Not Yet Implemented

1. **Delimited Continuations (shift/reset)** - Designed but not coded
   - Prompt stack infrastructure
   - `make-continuation-prompt-tag`
   - `call-with-continuation-prompt`
   - `abort-current-continuation`
   - `call-with-composable-continuation`
   - `(patina control)` library

### ✅ Technical Debt Resolved (2025-12-12)

- **CPS-only evaluation** - All evaluation now uses CPS transformation
- **Library loading uses CPS** - All lambdas are CpsLambdas
- **`--cps` flag removed** - CPS is the default mode
- **`EvalMode` enum removed** - Simplified backend API
- Internal escape mechanism retained for continuation invocation within CPS

---

## Overview

This document details how to implement continuations in Patina's tree-walking interpreter using **CPS (Continuation-Passing Style) transformation**. The implementation provides:

1. **R7RS `call/cc`** - Full continuations for R7RS compliance
2. **Delimited continuations** - Racket-style `shift`/`reset` via `(patina control)` library
3. **Exception handling foundation** - `guard`/`raise` built on delimited continuations

**Key Decision**: We use CPS transformation rather than explicit continuation stacks because:
- CPS naturally supports both full and delimited continuations
- Delimited continuations (`shift`/`reset`) are the more fundamental primitive
- `call/cc` can be elegantly implemented in terms of `shift`/`reset`
- This aligns with Phase 2 VM design which uses delimited continuations as the primitive

## R7RS Requirements

From R7RS-small section 6.10 (Control features):

- **Procedure**: `(call-with-current-continuation proc)`
- **Procedure**: `(call/cc proc)` - abbreviation for the above
- **Procedure**: `(dynamic-wind before thunk after)`

**Key properties of continuations:**
1. Continuations are first-class values (can be stored, passed around)
2. Continuations can be invoked multiple times
3. Continuations have indefinite extent (outlive their dynamic scope)
4. Invoking a continuation abandons the current computation
5. `dynamic-wind` handlers must be called when crossing continuation boundaries

## Architecture: CPS Transformation

### Why CPS Over Explicit Continuation Stack

The previous design considered an "explicit continuation stack" approach, but CPS transformation is superior for several reasons:

| Aspect | Explicit Stack | CPS Transformation |
|--------|---------------|-------------------|
| Delimited continuations | Complex (need prompt markers) | Natural (just capture up to reset) |
| Full continuations | Must copy entire stack | Continuation is just a closure |
| Implementation | Many continuation frame types | Uniform: everything is a function |
| TCO | Must track tail position carefully | Naturally preserved |
| Exception handling | Separate unwind mechanism | Just abort to handler prompt |

### CPS Basics

In CPS, every function takes an extra parameter: the continuation (what to do with the result).

```scheme
;; Direct style
(define (add1 x) (+ x 1))
(add1 5)  ; => 6

;; CPS style
(define (add1-cps x k) (k (+ x 1)))
(add1-cps 5 (lambda (result) result))  ; => 6
```

**Key insight**: In CPS, control flow is explicit. "Returning" means calling the continuation. "Escaping" means calling a different continuation.

### Delimited Continuations: shift/reset

Delimited continuations are more powerful and easier to implement than full `call/cc`. They provide bounded continuation capture.

```scheme
;; reset establishes a boundary (prompt)
;; shift captures the continuation up to the enclosing reset

(+ 1 (reset (+ 2 (shift k (k (k 10))))))
; Step 1: shift captures k = (lambda (v) (+ 2 v))
; Step 2: (k (k 10)) = (k (+ 2 10)) = (k 12) = (+ 2 12) = 14
; Step 3: (+ 1 14) = 15
```

**Why delimited is better:**
- **Bounded**: Only captures stack up to the prompt (not entire stack)
- **Composable**: Multiple prompts can be nested with different tags
- **Efficient**: Smaller continuations, faster capture
- **Expressive**: Can implement exceptions, generators, async/await

### Implementation Options for call/cc

Since we're using CPS transformation, we have three options for implementing `call/cc`:

#### Option A: call/cc Directly on CPS

In CPS, every function already takes an explicit continuation parameter `k`. Implementing `call/cc` is straightforward—the continuation is literally what's being passed around.

```rust
// In CPS evaluator, call/cc is trivial:
fn eval_callcc(&mut self, f: Value, k: Continuation, env: Env) -> Result<Value, EvalError> {
    // The continuation 'k' is already available - wrap it as a Value and pass to f
    let reified_k = Value::Continuation(k.clone());

    // (call/cc f) = (f k), where k is the current continuation
    self.apply(f, vec![reified_k], k, env)
}
```

```scheme
;; Conceptually in CPS:
;; (call/cc f) transforms to:
;; (f (lambda (v ignore-k) (k v)) k)
;;     ^^^^^^^^^^^^^^^^^^^^^^
;;     captured continuation that ignores its own continuation
```

| Pros | Cons |
|------|------|
| Simple - continuation already explicit | Full continuations only (no bounded capture) |
| Efficient - no prompt lookup overhead | `dynamic-wind` needs separate tracking |
| Direct mapping to CPS semantics | Can't express shift/reset on top of it easily |
| Easy to understand and debug | Less unified model |

#### Option B: call/cc via shift/reset

Build `call/cc` on top of delimited continuations with a top-level prompt:

```scheme
;; call/cc in terms of shift/reset (Racket style)
(define *top-level-prompt* (make-continuation-prompt-tag 'top-level))

(define (call/cc f)
  (call-with-composable-continuation
    (lambda (k)
      (f (lambda (v)
           (abort-current-continuation *top-level-prompt* (lambda () (k v))))))
    *top-level-prompt*))
```

| Pros | Cons |
|------|------|
| Unified model - one primitive for all control | More machinery (prompt stack, tags, abort) |
| shift/reset can implement call/cc, generators, exceptions | Overhead from prompt lookup |
| `dynamic-wind` integrates naturally with prompts | More complex mental model |
| Matches Racket's proven architecture | call/cc performance slightly worse |

#### Option C: Hybrid Approach (Recommended)

Implement both primitives sharing the same infrastructure:

```
                    CPS Evaluator
                         │
         ┌───────────────┴───────────────┐
         │                               │
    call/cc (direct)            shift/reset (prompt-based)
         │                               │
         └───────────────┬───────────────┘
                         │
              Shared Infrastructure:
              • Continuation representation
              • dynamic-wind handler stack
              • Environment capture
```

**How it works:**

1. **Shared continuation representation** - Both use the same `Continuation` type (a CPS closure + captured environment)

2. **call/cc is direct** - Since continuation is already explicit in CPS, just reify it:
   ```rust
   fn call_cc(&mut self, f: Value, k: Continuation) -> Result<Value, EvalError> {
       let wrapped_k = self.reify_continuation(k.clone());
       self.apply(f, vec![wrapped_k], k)
   }
   ```

3. **shift/reset use prompt stack** - Add prompt markers to track delimited boundaries:
   ```rust
   fn reset(&mut self, body: CpsExpr, k: Continuation, tag: PromptTag) -> Result<Value, EvalError> {
       self.prompt_stack.push(Prompt { tag, continuation: k });
       self.eval(body, identity_continuation)
   }

   fn shift(&mut self, f: Value, tag: PromptTag) -> Result<Value, EvalError> {
       let prompt = self.find_prompt(&tag)?;
       let delimited_k = self.capture_to_prompt(&prompt);
       // Pop frames up to prompt, apply f with captured continuation
       self.apply(f, vec![delimited_k], prompt.continuation)
   }
   ```

4. **dynamic-wind integrates with both** - Wind handlers tracked in evaluator state, checked on any continuation invocation

| Pros | Cons |
|------|------|
| Efficient call/cc (no prompt overhead) | Two code paths to maintain |
| Full power of delimited continuations | Slightly more complex implementation |
| Best of both worlds | Need to ensure consistency between paths |
| Clear separation of concerns | |

#### Recommendation

**Use Option C (Hybrid)** because:

1. **call/cc is common** - Many Scheme programs use it; direct implementation is faster
2. **shift/reset are powerful** - Needed for `(patina control)` library, generators, etc.
3. **CPS makes both natural** - The continuation is already explicit, so both are straightforward
4. **Shared infrastructure** - Continuation representation, `dynamic-wind`, environment capture are the same

The key insight: in CPS, a continuation is just a closure. The difference between `call/cc` and `shift` is only *how much* of the continuation to capture:
- `call/cc`: captures everything (full continuation)
- `shift`: captures up to the nearest `reset` prompt (delimited)

Both operations work on the same underlying representation.

## Implementation Strategy

### Phase 1: Core CPS Infrastructure

#### 1.1 CPS-Converted CoreExpr

Add a CPS intermediate representation:

```rust
// In crates/patina-ir/src/cps.rs (new file)

/// CPS expression - every sub-expression has explicit continuation
#[derive(Debug, Clone)]
pub enum CpsExpr {
    /// Literal value, pass to continuation
    Literal {
        value: Value,
        cont: Box<CpsExpr>,
    },

    /// Variable reference, pass to continuation
    Var {
        name: String,
        cont: Box<CpsExpr>,
    },

    /// Lambda (closure) - takes extra continuation parameter
    Lambda {
        params: Vec<String>,
        cont_param: String,  // The continuation parameter
        body: Box<CpsExpr>,
    },

    /// Application - evaluate operator, operands, then apply
    App {
        operator: Box<CpsExpr>,
        operands: Vec<CpsExpr>,
        cont: Box<CpsExpr>,
    },

    /// If - evaluate test, then branch
    If {
        test: Box<CpsExpr>,
        consequent: Box<CpsExpr>,
        alternate: Box<CpsExpr>,
    },

    /// Set! - evaluate value, mutate, continue
    Set {
        var: String,
        value: Box<CpsExpr>,
        cont: Box<CpsExpr>,
    },

    /// Begin - sequence expressions
    Begin {
        exprs: Vec<CpsExpr>,
    },

    /// Call continuation with value (the "return")
    Continue {
        cont: String,      // Continuation variable name
        value: Box<CpsExpr>,
    },

    // === Delimited Continuation Primitives ===

    /// Reset - establish a prompt boundary
    Reset {
        tag: PromptTag,
        body: Box<CpsExpr>,
        cont: Box<CpsExpr>,
    },

    /// Shift - capture continuation up to prompt
    Shift {
        tag: PromptTag,
        cont_var: String,  // Binds captured continuation
        body: Box<CpsExpr>,
    },

    /// Abort - jump to prompt, discarding intermediate computation
    Abort {
        tag: PromptTag,
        value: Box<CpsExpr>,
    },
}

/// Prompt tag for identifying reset boundaries
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PromptTag {
    pub name: String,
    pub id: u64,  // Unique identifier
}
```

#### 1.2 CPS Transformation Pass

Transform CoreExpr to CpsExpr:

```rust
// In crates/patina-frontend/src/cps_transform.rs (new file)

use patina_ir::{CoreExpr, CpsExpr, PromptTag};

pub struct CpsTransformer {
    gensym_counter: u64,
}

impl CpsTransformer {
    pub fn new() -> Self {
        Self { gensym_counter: 0 }
    }

    fn gensym(&mut self, prefix: &str) -> String {
        self.gensym_counter += 1;
        format!("{}_{}", prefix, self.gensym_counter)
    }

    /// Transform CoreExpr to CPS
    /// The `cont` parameter is what to do with the result
    pub fn transform(&mut self, expr: &CoreExpr, cont: CpsCont) -> CpsExpr {
        match expr {
            CoreExpr::Literal(v) => {
                // (literal v) in CPS: (cont v)
                cont.apply(CpsExpr::Literal {
                    value: v.clone(),
                    cont: Box::new(CpsExpr::Halt), // placeholder
                })
            }

            CoreExpr::Var(name) => {
                // (var x) in CPS: (cont x)
                cont.apply(CpsExpr::Var {
                    name: name.clone(),
                    cont: Box::new(CpsExpr::Halt),
                })
            }

            CoreExpr::Lambda { params, variadic, body } => {
                // (lambda (x ...) body) in CPS:
                // (cont (lambda (x ... k) [body in CPS with k]))
                let cont_param = self.gensym("k");
                let cps_body = self.transform_body(body, &cont_param);

                let mut all_params = params.clone();
                // variadic handling...

                cont.apply(CpsExpr::Lambda {
                    params: all_params,
                    cont_param,
                    body: Box::new(cps_body),
                })
            }

            CoreExpr::App { operator, operands } => {
                // (f a b) in CPS:
                // [f in CPS with (lambda (f')
                //   [a in CPS with (lambda (a')
                //     [b in CPS with (lambda (b')
                //       (f' a' b' cont))])])]
                self.transform_app(operator, operands, cont)
            }

            CoreExpr::If { test, consequent, alternate } => {
                // (if test then else) in CPS:
                // [test in CPS with (lambda (t)
                //   (if t [then in CPS] [else in CPS]))]
                let result_var = self.gensym("if_result");
                let join_cont = cont.clone();

                let cps_then = self.transform(consequent, join_cont.clone());
                let cps_else = self.transform(alternate, join_cont);

                self.transform(test, CpsCont::If {
                    consequent: Box::new(cps_then),
                    alternate: Box::new(cps_else),
                })
            }

            // ... other cases
        }
    }

    fn transform_body(&mut self, body: &[CoreExpr], cont_param: &str) -> CpsExpr {
        // Transform body expressions, last one uses the continuation
        if body.is_empty() {
            CpsExpr::Continue {
                cont: cont_param.to_string(),
                value: Box::new(CpsExpr::Literal {
                    value: Value::Unspecified,
                    cont: Box::new(CpsExpr::Halt),
                }),
            }
        } else if body.len() == 1 {
            self.transform(&body[0], CpsCont::Var(cont_param.to_string()))
        } else {
            // Sequence: evaluate all but last for effect
            let last = &body[body.len() - 1];
            let init = &body[..body.len() - 1];

            let cps_last = self.transform(last, CpsCont::Var(cont_param.to_string()));

            init.iter().rev().fold(cps_last, |acc, expr| {
                let ignore_var = self.gensym("_");
                self.transform(expr, CpsCont::Seq {
                    var: ignore_var,
                    next: Box::new(acc),
                })
            })
        }
    }
}

/// Represents what to do with a value (meta-continuation during transform)
#[derive(Clone)]
enum CpsCont {
    /// Pass to named continuation variable
    Var(String),
    /// Ignore result, continue with next expression
    Seq { var: String, next: Box<CpsExpr> },
    /// Branch based on boolean
    If { consequent: Box<CpsExpr>, alternate: Box<CpsExpr> },
    // ... other cases
}

impl CpsCont {
    fn apply(&self, value_expr: CpsExpr) -> CpsExpr {
        match self {
            CpsCont::Var(k) => CpsExpr::Continue {
                cont: k.clone(),
                value: Box::new(value_expr),
            },
            CpsCont::Seq { var, next } => {
                // Let-bind the value, then continue
                CpsExpr::Let {
                    var: var.clone(),
                    value: Box::new(value_expr),
                    body: next.clone(),
                }
            }
            CpsCont::If { consequent, alternate } => {
                CpsExpr::If {
                    test: Box::new(value_expr),
                    consequent: consequent.clone(),
                    alternate: alternate.clone(),
                }
            }
        }
    }
}
```

#### 1.3 CPS Evaluator

Evaluate CPS expressions:

```rust
// In crates/patina-tree-walker/src/eval/cps_eval.rs (new file)

use patina_ir::CpsExpr;
use patina_runtime::{Environment, Value};
use std::collections::HashMap;
use std::rc::Rc;

/// Prompt stack for delimited continuations
struct PromptStack {
    prompts: Vec<Prompt>,
}

struct Prompt {
    tag: PromptTag,
    continuation: Rc<CpsClosure>,  // What to do after reset body completes
    env: Rc<Environment>,
}

/// A captured delimited continuation
#[derive(Clone)]
struct DelimitedContinuation {
    /// The CPS code representing the captured computation
    body: Rc<CpsExpr>,
    /// Environment at capture point
    env: Rc<Environment>,
    /// Prompt tag this was captured at
    tag: PromptTag,
}

pub struct CpsEvaluator {
    prompt_stack: PromptStack,
    global_env: Rc<Environment>,
}

impl CpsEvaluator {
    pub fn eval(&mut self, expr: &CpsExpr, env: Rc<Environment>) -> Result<Value, EvalError> {
        match expr {
            CpsExpr::Literal { value, .. } => Ok(value.clone()),

            CpsExpr::Var { name, .. } => {
                env.get(name).ok_or_else(|| EvalError::UndefinedVariable(name.clone()))
            }

            CpsExpr::Lambda { params, cont_param, body } => {
                Ok(Value::Procedure(Procedure::CpsLambda {
                    params: params.clone(),
                    cont_param: cont_param.clone(),
                    body: Rc::new((**body).clone()),
                    env: env.clone(),
                }))
            }

            CpsExpr::Continue { cont, value } => {
                // Look up continuation and invoke it
                let cont_val = env.get(cont)?;
                let arg = self.eval(value, env.clone())?;
                self.apply_continuation(cont_val, arg)
            }

            CpsExpr::Reset { tag, body, cont } => {
                // Push prompt, evaluate body, pop prompt
                let prompt = Prompt {
                    tag: tag.clone(),
                    continuation: self.capture_current_cont(cont, env.clone()),
                    env: env.clone(),
                };
                self.prompt_stack.prompts.push(prompt);

                let result = self.eval(body, env.clone())?;

                self.prompt_stack.prompts.pop();

                // Continue with reset's continuation
                self.eval(cont, env.with_binding("reset_result", result))
            }

            CpsExpr::Shift { tag, cont_var, body } => {
                // Find matching prompt
                let prompt_idx = self.find_prompt(tag)?;

                // Capture continuation from current point to prompt
                let captured = self.capture_delimited_continuation(prompt_idx);

                // Remove frames up to and including prompt
                self.prompt_stack.prompts.truncate(prompt_idx);

                // Bind captured continuation and evaluate body
                let new_env = env.with_binding(cont_var,
                    Value::DelimitedContinuation(captured));

                self.eval(body, new_env)
            }

            CpsExpr::Abort { tag, value } => {
                // Find matching prompt
                let prompt_idx = self.find_prompt(tag)?;
                let prompt = &self.prompt_stack.prompts[prompt_idx];

                // Evaluate the value to abort with
                let abort_value = self.eval(value, env)?;

                // Discard everything up to prompt, invoke prompt's handler
                let handler_cont = prompt.continuation.clone();
                let handler_env = prompt.env.clone();

                self.prompt_stack.prompts.truncate(prompt_idx);

                // Invoke the prompt's continuation with abort value
                self.apply_continuation(
                    Value::CpsClosure(handler_cont),
                    abort_value
                )
            }

            // ... other cases
        }
    }

    fn find_prompt(&self, tag: &PromptTag) -> Result<usize, EvalError> {
        self.prompt_stack.prompts.iter()
            .rposition(|p| p.tag == *tag)
            .ok_or_else(|| EvalError::NoMatchingPrompt(tag.clone()))
    }

    fn capture_delimited_continuation(&self, prompt_idx: usize) -> DelimitedContinuation {
        // Capture the computation from current point to prompt
        // This is the key operation for shift
        DelimitedContinuation {
            // ... capture relevant state
        }
    }
}
```

### Phase 2: Delimited Continuation Primitives

#### 2.1 Value Representation Updates

```rust
// In crates/patina-runtime/src/value.rs

pub enum Procedure {
    Primitive { name: &'static str, arity: Arity, library: Option<&'static str> },
    Lambda { params: Vec<String>, variadic: Option<String>, body: Vec<Value>, env: Rc<Environment> },

    // New: CPS lambda with explicit continuation parameter
    CpsLambda {
        params: Vec<String>,
        cont_param: String,
        body: Rc<CpsExpr>,
        env: Rc<Environment>,
    },

    // New: Captured delimited continuation
    DelimitedContinuation {
        captured_expr: Rc<CpsExpr>,
        captured_env: Rc<Environment>,
        prompt_tag: PromptTag,
    },

    // Full continuation (for call/cc compatibility)
    FullContinuation {
        // Implemented in terms of delimited continuation + top-level prompt
        inner: Box<Procedure>,  // DelimitedContinuation
        top_prompt: PromptTag,
    },
}

// Prompt tag as a first-class value
pub enum Value {
    // ... existing variants ...

    /// Continuation prompt tag
    PromptTag(Rc<PromptTag>),

    /// Delimited continuation (composable)
    DelimitedContinuation(Rc<DelimitedContinuationData>),
}

#[derive(Debug, Clone)]
pub struct PromptTag {
    pub name: String,
    pub id: u64,
}

#[derive(Debug, Clone)]
pub struct DelimitedContinuationData {
    /// The captured computation
    pub frames: Vec<ContinuationFrame>,
    /// Environment at capture
    pub env: Rc<Environment>,
    /// Tag of the prompt this was captured at
    pub tag: PromptTag,
}
```

#### 2.2 Primitive Implementations

```rust
// In crates/patina-tree-walker/src/eval/primitives/control.rs

/// (make-continuation-prompt-tag) -> prompt-tag
/// (make-continuation-prompt-tag name) -> prompt-tag
fn make_continuation_prompt_tag(args: &[Value]) -> Result<Value, EvalError> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let name = match args.get(0) {
        Some(Value::Symbol(s)) => s.to_string(),
        Some(v) => return Err(EvalError::TypeError("symbol", v.type_name())),
        None => "prompt".to_string(),
    };

    Ok(Value::PromptTag(Rc::new(PromptTag {
        name,
        id: COUNTER.fetch_add(1, Ordering::SeqCst),
    })))
}

/// (continuation-prompt-tag? v) -> boolean
fn continuation_prompt_tag_p(args: &[Value]) -> Result<Value, EvalError> {
    check_arity(args, 1)?;
    Ok(Value::Boolean(matches!(args[0], Value::PromptTag(_))))
}

/// (call-with-continuation-prompt thunk tag [handler]) -> value
/// Establishes a prompt and calls thunk
fn call_with_continuation_prompt(
    evaluator: &CpsEvaluator,
    args: &[Value],
    env: Rc<Environment>,
) -> Result<Value, EvalError> {
    check_arity_range(args, 2, 3)?;

    let thunk = args[0].as_procedure()?;
    let tag = args[1].as_prompt_tag()?;
    let handler = args.get(2).cloned();

    evaluator.with_prompt(tag, handler, || {
        evaluator.apply(thunk, vec![], env)
    })
}

/// (abort-current-continuation tag value) -> (never returns normally)
/// Aborts to the nearest prompt with the given tag
fn abort_current_continuation(
    evaluator: &CpsEvaluator,
    args: &[Value],
) -> Result<Value, EvalError> {
    check_arity(args, 2)?;

    let tag = args[0].as_prompt_tag()?;
    let value = args[1].clone();

    // This unwinds the stack to the prompt
    Err(EvalError::Abort { tag: tag.clone(), value })
}

/// (call-with-composable-continuation proc tag) -> value
/// Captures delimited continuation up to prompt with tag
fn call_with_composable_continuation(
    evaluator: &CpsEvaluator,
    args: &[Value],
    env: Rc<Environment>,
) -> Result<Value, EvalError> {
    check_arity(args, 2)?;

    let proc = args[0].as_procedure()?;
    let tag = args[1].as_prompt_tag()?;

    // Capture continuation up to prompt
    let captured = evaluator.capture_continuation_to_prompt(&tag)?;

    // Call proc with the captured continuation
    evaluator.apply(proc, vec![Value::DelimitedContinuation(captured)], env)
}
```

### Phase 3: shift/reset Macros

The high-level `shift` and `reset` are macros built on the primitives:

```scheme
;; In lib/patina/control.sld

(define-library (patina control)
  (export
    ;; Prompt tags
    make-continuation-prompt-tag
    continuation-prompt-tag?
    default-continuation-prompt-tag

    ;; Low-level primitives
    call-with-continuation-prompt
    abort-current-continuation
    call-with-composable-continuation

    ;; High-level: shift/reset (Danvy/Filinski style)
    reset
    shift

    ;; High-level: control/prompt (Felleisen style)
    prompt
    control

    ;; R7RS call/cc (implemented via delimited continuations)
    call-with-current-continuation
    call/cc)

  (import (scheme base)
          (patina internal control))

  (begin
    ;; Default prompt tag for reset/shift
    (define default-continuation-prompt-tag
      (make-continuation-prompt-tag 'default))

    ;; reset: establish a prompt boundary
    ;; (reset expr ...) = (call-with-continuation-prompt (lambda () expr ...) default-tag)
    (define-syntax reset
      (syntax-rules ()
        ((reset body ...)
         (call-with-continuation-prompt
           (lambda () body ...)
           default-continuation-prompt-tag))))

    ;; shift: capture delimited continuation
    ;; (shift k body ...) captures k = continuation up to enclosing reset
    (define-syntax shift
      (syntax-rules ()
        ((shift k body ...)
         (call-with-composable-continuation
           (lambda (k)
             (abort-current-continuation
               default-continuation-prompt-tag
               (lambda () body ...)))
           default-continuation-prompt-tag))))

    ;; reset0/shift0: variants that don't re-establish prompt when invoking k
    (define-syntax reset0
      (syntax-rules ()
        ((reset0 body ...)
         (call-with-continuation-prompt
           (lambda () body ...)
           default-continuation-prompt-tag
           (lambda (thunk) (thunk))))))  ; Handler just calls thunk

    (define-syntax shift0
      (syntax-rules ()
        ((shift0 k body ...)
         (call-with-composable-continuation
           (lambda (raw-k)
             (abort-current-continuation
               default-continuation-prompt-tag
               (lambda ()
                 (let ((k (lambda (v)
                            ;; Don't re-establish prompt
                            (raw-k v))))
                   body ...))))
           default-continuation-prompt-tag))))

    ;; Felleisen-style control/prompt (non-composable by default)
    (define-syntax prompt
      (syntax-rules ()
        ((prompt body ...)
         (call-with-continuation-prompt
           (lambda () body ...)
           default-continuation-prompt-tag
           (lambda (thunk) (thunk))))))

    (define-syntax control
      (syntax-rules ()
        ((control k body ...)
         (call-with-composable-continuation
           (lambda (k)
             (abort-current-continuation
               default-continuation-prompt-tag
               (lambda () body ...)))
           default-continuation-prompt-tag))))

    ;; === R7RS call/cc ===
    ;; Implemented via delimited continuations with a top-level prompt

    (define top-level-prompt-tag
      (make-continuation-prompt-tag 'top-level))

    ;; call/cc: capture full continuation
    ;; The continuation, when invoked, aborts to top-level
    (define (call-with-current-continuation proc)
      (call-with-composable-continuation
        (lambda (k)
          (proc (lambda (v)
                  (abort-current-continuation
                    top-level-prompt-tag
                    (lambda () (k v))))))
        top-level-prompt-tag))

    (define call/cc call-with-current-continuation)))
```

### Phase 4: dynamic-wind Integration

`dynamic-wind` must interact properly with continuations:

```rust
// In crates/patina-tree-walker/src/eval/cps_eval.rs

/// Dynamic wind record
#[derive(Clone)]
struct DynamicWindRecord {
    before: Value,  // Thunk to call on entry
    after: Value,   // Thunk to call on exit
    depth: usize,   // Nesting depth
}

impl CpsEvaluator {
    /// Track dynamic-wind handlers
    dynamic_winds: Vec<DynamicWindRecord>,

    /// (dynamic-wind before thunk after)
    fn dynamic_wind(&mut self, before: Value, thunk: Value, after: Value, env: Rc<Environment>)
        -> Result<Value, EvalError>
    {
        // Call before thunk
        self.apply(before.clone(), vec![], env.clone())?;

        // Push wind record
        let record = DynamicWindRecord {
            before: before.clone(),
            after: after.clone(),
            depth: self.dynamic_winds.len(),
        };
        self.dynamic_winds.push(record);

        // Call main thunk
        let result = self.apply(thunk, vec![], env.clone());

        // Pop wind record
        self.dynamic_winds.pop();

        // Call after thunk
        self.apply(after, vec![], env)?;

        result
    }

    /// When invoking a continuation, run appropriate dynamic-wind handlers
    fn invoke_continuation_with_winds(
        &mut self,
        cont: &DelimitedContinuationData,
        value: Value,
    ) -> Result<Value, EvalError> {
        let current_depth = self.dynamic_winds.len();
        let target_depth = cont.wind_depth;

        // Unwind: call 'after' thunks from current to common ancestor
        while self.dynamic_winds.len() > target_depth {
            let record = self.dynamic_winds.pop().unwrap();
            self.apply(record.after, vec![], self.global_env.clone())?;
        }

        // Rewind: call 'before' thunks from common ancestor to target
        for record in &cont.wind_records {
            self.apply(record.before.clone(), vec![], self.global_env.clone())?;
            self.dynamic_winds.push(record.clone());
        }

        // Now invoke the continuation
        self.apply_delimited_continuation(cont, value)
    }
}
```

## Implementation Plan

### Step 1: CPS Infrastructure ✅ COMPLETE
- [x] Define `CpsExpr` IR in `patina-core`
- [x] Implement CPS transformation pass in `patina-ir`
- [x] Add `CpsEvaluator` in `patina-tree-walker`
- [x] Ensure existing tests pass with CPS path (97.3% passing)

### Step 2: Basic call/cc ✅ COMPLETE
- [x] Implement `call/cc` and `call-with-current-continuation`
- [x] Continuation invocation aborts current computation
- [x] Continuations satisfy `procedure?` predicate
- [x] Continuation escape through library functions (thread-local mechanism)

### Step 3: Delimited Continuation Primitives (TODO)
- [ ] Add `PromptTag` and `DelimitedContinuation` to Value
- [ ] Implement prompt stack in evaluator
- [ ] Implement `call-with-continuation-prompt`
- [ ] Implement `abort-current-continuation`
- [ ] Implement `call-with-composable-continuation`

### Step 4: shift/reset Library (TODO)
- [ ] Create `lib/patina/control.sld`
- [ ] Implement `reset` and `shift` macros
- [ ] Implement `control` and `prompt` variants
- [ ] Write tests for delimited continuations

### Step 5: dynamic-wind ✅ COMPLETE
- [x] Implement `dynamic-wind` primitive
- [x] Track wind handlers in evaluator state (via `DynamicWindCleanup` continuation)
- [x] Run handlers on continuation invocation
- [x] Capture/restore wind state in reified continuations
- [x] Pass R7RS dynamic-wind tests (including nested + call/cc re-entry)

### Step 6: Exception Handling ✅ COMPLETE
- [x] Implement `guard` syntax
- [x] Implement `raise` and `raise-continuable`
- [x] Implement `with-exception-handler`
- [x] Pass R7RS exception tests (26/26)

### Step 7: CPS-Only Evaluation ✅ COMPLETE (2025-12-12)
- [x] Make CPS the default evaluation mode
- [x] Load libraries in CPS mode (via `eval_cps` in `evaluate_parsed_library`)
- [x] Fixed `eval_cps` to use passed environment (was ignoring it!)
- [x] Remove `EvalMode` enum from `TreeWalker`
- [x] Remove `--cps` flag from REPL
- [x] All 1159 chibi R7RS tests pass (100%)
- [x] All ~1400 internal tests pass

## Testing Strategy

### Reference Test Suites

We can borrow comprehensive test cases from these reference implementations:

#### Gauche Test Suite (Primary Reference)

**Location**: `~/Project/reference/Gauche/tests/`

| File | Lines | Coverage |
|------|-------|----------|
| `continuation.scm` | 796 | call/cc, dynamic-wind, prompts, continuation marks |
| `partcont.scm` | 400+ | shift/reset, interaction with call/cc, parameters |
| `partcont-racket.scm` | - | Racket-compatible delimited continuation tests |
| `partcont-srfi-226.scm` | - | SRFI-226 compliance tests |

**Key advantage**: Tests are annotated with expected behavior across implementations:
```scheme
;; native : [d01][d02][d04][d01][d03][d04]
;; meta   : [d01][d02][d04][d01][d03][d04]
;; srfi226: [d01][d02][d04][d01][d03][d04]
;; racket : [d01][d02][d04][d01][d03][d04]
(test* "dynamic-wind + reset/shift 2" ...)
```

#### Racket Test Suite

**Location**: `~/Project/reference/racket/pkgs/racket-test/tests/racket/`

| File | Coverage |
|------|----------|
| `prompt-sfs.scm` | Memory/stack frame testing for prompts |
| `contract/prompt-tag.rkt` | Prompt tag contracts |
| `stress/prompt-mem-use.rkt` | Memory stress tests |

#### Chibi-scheme Test Suite

**Location**: `~/Project/reference/chibi-scheme/tests/`

| File | Coverage |
|------|----------|
| `basic/test08-callcc.scm` | Pythagorean triple backtracking example |
| `r7rs-tests.scm` | R7RS compliance (call/cc, dynamic-wind sections) |

---

### Test Categories

#### 1. Basic call/cc Tests

```scheme
;; Environment capture with doubling loop (Gauche continuation.scm:14-23)
(define (callcc-test1)
  (let ((r '())
        (c #f))
    (let ((w (let ((v 1))
               (set! v (+ (call/cc (lambda (c0) (set! c c0) v)) v))
               (set! r (cons v r))
               v)))
      (if (<= w 1024) (c w) r))))
;; Expected: '(2048 1024 512 256 128 64 32 16 8 4 2)

;; Multiple values (Gauche continuation.scm:27-43)
(test "call/cc (values)" '(1 2 3)
  (call-with-values
    (lambda () (call/cc (lambda (c) (c 1 2 3))))
    list))

;; Zero values
(test "call/cc (zero value)" '()
  (call-with-values
    (lambda () (call/cc (lambda (c) (c))))
    list))

;; Inline capture in list construction (tests VM stack handling)
(define (callcc-test2)
  (let ((cc #f) (r '()))
    (let ((s (list 1 2 3 4 (call/cc (lambda (c) (set! cc c) 5)) 6 7 8)))
      (if (null? r)
        (begin (set! r s) (cc -1))
        (list r s)))))
;; Expected: '((1 2 3 4 5 6 7 8) (1 2 3 4 -1 6 7 8))

;; call/cc in do-loop (tests frame preparation)
(test "call/cc (do)" 6
  (lambda ()
    (do ((x 0 (+ x 1))
         (y 0 (call/cc (lambda (c) c))))
        ((> x 5) x)
      #f)))

;; set! interaction (critical for mutable variable handling)
(test "call/cc and set!" '((#t . 2) (#t . 1))
  (lambda ()
    (let ((cont #f) (r '()))
      (let ((f (lambda (x y)
                 (set! r (cons (cons x y) r))
                 (set! x #f))))
        (f #t (call/cc (lambda (c) (set! cont c) 1))))
      (if cont
        (let ((k cont))
          (set! cont #f)
          (k 2))
        r))))
```

#### 2. dynamic-wind Tests

```scheme
;; R5RS standard example (Gauche continuation.scm:199-212)
(define (dynwind-test1)
  (let ((path '())
        (c #f))
    (let ((add (lambda (s) (set! path (cons s path)))))
      (dynamic-wind
        (lambda () (add 'connect))
        (lambda () (add (call/cc (lambda (c0) (set! c c0) 'talk1))))
        (lambda () (add 'disconnect)))
      (if (< (length path) 4)
        (c 'talk2)
        (reverse path)))))
;; Expected: '(connect talk1 disconnect connect talk2 disconnect)

;; Nested dynamic-wind (Gauche continuation.scm:226-244)
(test "dynamic-wind nested" '(a b c d e f g b c d e f g h)
  (lambda ()
    (let ((x '()) (c #f))
      (dynamic-wind
        (lambda () (set! x (cons 'a x)))
        (lambda ()
          (dynamic-wind
            (lambda () (set! x (cons 'b x)))
            (lambda ()
              (dynamic-wind
                (lambda () (set! x (cons 'c x)))
                (lambda () (set! c (call/cc (lambda (k) k))))
                (lambda () (set! x (cons 'd x)))))
            (lambda () (set! x (cons 'e x))))
          (dynamic-wind
            (lambda () (set! x (cons 'f x)))
            (lambda () (if c (c #f)))
            (lambda () (set! x (cons 'g x)))))
        (lambda () (set! x (cons 'h x))))
      (reverse x))))

;; Error in before thunk (Gauche continuation.scm:262-279)
(test "dynamic-wind - error in before thunk" '(a b c d h)
  ...)

;; Error in after thunk (Gauche continuation.scm:281-298)
(test "dynamic-wind - error in after thunk" '(a b c d e f h)
  ...)
```

#### 3. Delimited Continuation Tests (shift/reset)

```scheme
;; Basic reset/shift (Gauche partcont.scm:20-34)
(test "reset/shift combination 1" 1000
  (begin
    (define k1 #f)
    (define k2 #f)
    (define k3 #f)
    (reset
      (shift k (set! k1 k)
        (shift k (set! k2 k)
          (shift k (set! k3 k))))
      1000)
    (k1)))

;; reset/shift with values (Gauche partcont.scm:40-55)
(test "reset/shift + values 1" '(1 2 3)
  (values->list (reset (values 1 2 3))))

(test "reset/shift + values 2" '(1 2 3)
  (begin
    (define k1 #f)
    (reset
      (shift k (set! k1 k))
      (values 1 2 3))
    (values->list (k1))))

;; Calling partial continuation (Gauche partcont.scm:557-560)
(test "calling pc" 10
  (+ 1 (reset (+ 2 (shift k (+ 3 (k 4)))))))

(test "calling pc" '(1 3 2 4)
  (cons 1 (reset (cons 2 (shift k (cons 3 (k (cons 4 '()))))))))

;; Multiple invocations of captured continuation
(test "calling pc multi" 14
  (+ 1 (reset (+ 2 (shift k (+ 3 (k 5) (k 1)))))))

(test "calling pc multi" '(1 3 2 2 4)
  (cons 1 (reset (cons 2 (shift k (cons 3 (k (k (cons 4 '())))))))))
```

#### 4. shift/reset + call/cc Interaction

```scheme
;; Interaction test 1 (Gauche partcont.scm:77-93)
(test "reset/shift + call/cc 1" "[r01][r02][r02][r03]"
  (with-output-to-string
    (lambda ()
      (define k1 #f)
      (define done #f)
      (call/cc
        (lambda (k0)
          (reset
            (display "[r01]")
            (shift k (set! k1 k))
            (display "[r02]")
            (unless done
              (set! done #t)
              (k0))
            (display "[r03]"))))
      (k1))))

;; Interaction test 2 (Gauche partcont.scm:99-112)
(test "reset/shift + call/cc 2" "[r01][s01][s02][s02]"
  (with-output-to-string
    (lambda ()
      (define k1 #f)
      (define k2 #f)
      (reset
        (display "[r01]")
        (shift k (set! k1 k))
        (display "[s01]")
        (call/cc (lambda (k) (set! k2 k)))
        (display "[s02]"))
      (k1)
      (reset (reset (k2))))))
```

#### 5. shift/reset + dynamic-wind Interaction

```scheme
;; dynamic-wind inside shift body (Gauche partcont.scm:354-366)
(test "dynamic-wind + reset/shift 1" "[d01][d02][d03][d04]"
  (with-output-to-string
    (lambda ()
      (reset
        (shift k
          (dynamic-wind
            (lambda () (display "[d01]"))
            (lambda ()
              (display "[d02]")
              (k)
              (display "[d03]"))
            (lambda () (display "[d04]"))))))))

;; dynamic-wind around shift (Gauche partcont.scm:372-385)
(test "dynamic-wind + reset/shift 2" "[d01][d02][d04][d01][d03][d04]"
  (with-output-to-string
    (lambda ()
      (define k1 #f)
      (reset
        (dynamic-wind
          (lambda () (display "[d01]"))
          (lambda ()
            (display "[d02]")
            (shift k (set! k1 k))
            (display "[d03]"))
          (lambda () (display "[d04]"))))
      (k1))))
```

#### 6. shift/reset + parameterize Interaction

```scheme
;; Parameter scoping with shift (Gauche partcont.scm:61-71)
(test "reset/shift + parameterize 1" "010"
  (with-output-to-string
    (lambda ()
      (define p (make-parameter 0))
      (display (p))
      (reset
        (parameterize ((p 1))
          (display (p))
          ;; shift body executes outside reset, so p=0
          (shift k (display (p))))))))
```

#### 7. Prompt Tag Tests

```scheme
;; Basic prompt with tag (Gauche continuation.scm:444-462)
(test "call-with-continuation-prompt basic" 1
  (call-with-continuation-prompt (lambda () 1)))

(test "abort-current-continuation" '(foo bar)
  (let ((tag (make-continuation-prompt-tag)))
    (call-with-continuation-prompt
      (lambda ()
        (+ 1
           (abort-current-continuation tag 'foo 'bar)
           2))
      tag
      list)))

;; Missing prompt tag error
(test "abort to missing prompt" <error>
  (let ((tag1 (make-continuation-prompt-tag))
        (tag2 (make-continuation-prompt-tag)))
    (call-with-continuation-prompt
      (lambda ()
        (abort-current-continuation tag2 'oops))
      tag1)))
```

#### 8. Backtracking Example (amb)

```scheme
;; Pythagorean triple search (Chibi test08-callcc.scm)
(define fail (lambda () (error "no solution")))

(define (in-range a b)
  (call/cc
    (lambda (cont)
      (let enumerate ((i a))
        (if (> i b)
          (fail)
          (let ((save fail))
            (set! fail
              (lambda ()
                (set! fail save)
                (enumerate (+ i 1))))
            (cont i)))))))

(test "pythagorean triple" 345  ; 3^2 + 4^2 = 5^2
  (let ((x (in-range 1 10))
        (y (in-range 1 10))
        (z (in-range 1 10)))
    (if (= (+ (* x x) (* y y)) (* z z))
      (+ (* x 100) (* y 10) z)
      (fail))))
```

---

### Edge Cases to Test

Based on Gauche's test suite, these edge cases are critical:

1. **Continuation in list construction** - Tests VM stack handling when call/cc appears inside `(list ...)` expression
2. **Continuation in do-loop step** - Tests frame preparation during iteration
3. **set! + continuation interaction** - Tests mutable variable boxing behavior
4. **Error during dynamic-wind handler** - Tests cleanup when before/after thunks raise errors
5. **Nested dynamic-wind with continuation** - Tests proper wind/unwind ordering
6. **shift inside call/cc inside reset** - Tests interaction between full and delimited continuations
7. **Parameter scoping with shift** - Tests that shift body executes in correct parameter context
8. **Multiple prompt tags** - Tests prompt tag discrimination
9. **Abort to wrong/missing prompt** - Tests error handling for prompt mismatches
10. **Continuation reuse** - Tests calling same continuation multiple times

---

### Test File Organization

```
crates/patina-tests/tests/
├── continuations/
│   ├── call_cc_basic.rs        # Basic call/cc tests
│   ├── call_cc_values.rs       # Multiple values with continuations
│   ├── call_cc_edge_cases.rs   # VM stack, set!, do-loop tests
│   ├── dynamic_wind.rs         # dynamic-wind tests
│   ├── dynamic_wind_errors.rs  # Error handling in wind thunks
│   ├── delimited_basic.rs      # shift/reset basics
│   ├── delimited_values.rs     # Multiple values with shift/reset
│   ├── delimited_interaction.rs # shift/reset + call/cc interaction
│   ├── delimited_dynamic.rs    # shift/reset + dynamic-wind
│   ├── delimited_params.rs     # shift/reset + parameterize
│   ├── prompt_tags.rs          # Prompt tag tests
│   └── backtracking.rs         # amb, pythagorean triples
└── compliance/
    └── control.rs              # R7RS section 6.10 compliance
```

---

## License Compliance for Borrowed Tests

### Gauche Test Suite (BSD 3-Clause)

Gauche Scheme is licensed under the **BSD 3-Clause License**, which permits reuse with attribution.

**License Terms Summary:**
1. Redistributions of source code must retain the copyright notice
2. Redistributions in binary form must reproduce the copyright notice
3. Neither the name of the authors nor contributors may be used to endorse products without permission

**Required Attribution:**

When adapting tests from Gauche, include this header in test files:

```scheme
;;; Continuation tests adapted from Gauche Scheme
;;; Original source: https://github.com/shirok/Gauche
;;;
;;; Copyright (c) 2000-2025 Shiro Kawai <shiro@acm.org>
;;;
;;; Redistribution and use in source and binary forms, with or without
;;; modification, are permitted provided that the following conditions
;;; are met:
;;;
;;;  1. Redistributions of source code must retain the above copyright
;;;     notice, this list of conditions and the following disclaimer.
;;;
;;;  2. Redistributions in binary form must reproduce the above copyright
;;;     notice, this list of conditions and the following disclaimer in the
;;;     documentation and/or other materials provided with the distribution.
;;;
;;;  3. Neither the name of the authors nor the names of its contributors
;;;     may be used to endorse or promote products derived from this
;;;     software without specific prior written permission.
;;;
;;; THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS
;;; "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT
;;; LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
;;; A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT
;;; OWNER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
;;; SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED
;;; TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
;;; PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
;;; LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
;;; NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
;;; SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

### Racket Test Suite (Apache 2.0 / MIT)

Racket is dual-licensed under Apache 2.0 and MIT licenses. Both are permissive and allow reuse with attribution.

**Required Attribution:**

```scheme
;;; Tests adapted from Racket
;;; Original source: https://github.com/racket/racket
;;;
;;; Copyright (c) 2010-2025 PLT Design Inc.
;;; Licensed under Apache 2.0 or MIT License
;;; See: https://github.com/racket/racket/blob/master/LICENSE
```

### Chibi-scheme Test Suite (BSD 3-Clause)

Chibi-scheme is also BSD 3-Clause licensed.

**Required Attribution:**

```scheme
;;; Tests adapted from Chibi-scheme
;;; Original source: https://github.com/ashinn/chibi-scheme
;;;
;;; Copyright (c) 2009-2025 Alex Shinn
;;; Licensed under BSD 3-Clause License
```

### Implementation Strategy

**Recommended approach for Patina:**

1. **Create `THIRD_PARTY_LICENSES.md`** in project root documenting all borrowed test licenses

2. **Organize borrowed tests** in a clear structure:
   ```
   crates/patina-tests/tests/
   ├── continuations/
   │   ├── gauche_adapted.rs      # Tests adapted from Gauche (with header)
   │   ├── racket_adapted.rs      # Tests adapted from Racket (with header)
   │   └── patina_original.rs     # Our original tests
   ```

3. **Use "adapted from" language** - We're writing Rust test harnesses that call Scheme code inspired by these test suites, not copying verbatim

4. **Document in test files** which specific test cases were inspired by which source

**Example Rust test file header:**

```rust
//! Continuation tests for Patina
//!
//! Many test cases in this module are adapted from:
//! - Gauche Scheme (BSD 3-Clause) - https://github.com/shirok/Gauche
//!   Copyright (c) 2000-2025 Shiro Kawai
//! - Chibi-scheme (BSD 3-Clause) - https://github.com/ashinn/chibi-scheme
//!   Copyright (c) 2009-2025 Alex Shinn
//!
//! See THIRD_PARTY_LICENSES.md for full license texts.
```

---

## References

### Academic Papers

1. **"A Monadic Framework for Delimited Continuations"** (Dybvig, Peyton Jones, Sabry 2007)
   - Foundational paper on shift/reset semantics

2. **"Abstracting Control"** (Danvy, Filinski 1990)
   - Original shift/reset operators

3. **"Representing Control in the Presence of First-Class Continuations"** (Hieb, Dybvig, Bruggeman 1990)
   - Stack-based implementation

4. **"Binding as Sets of Scopes"** (Flatt 2016)
   - Racket's hygiene model (relevant for macro interaction)

### Reference Implementations

#### Racket

**Core Implementation:**
- `~/Project/reference/racket/racket/src/bc/src/fun.c`
  - `call_with_continuation_prompt` (lines 7247-7521)
  - `abort_current_continuation` (lines 7698-7778)
  - `Scheme_Prompt` structure (schpriv.h lines 2054-2070)

**Control Library:**
- `~/Project/reference/racket/racket/collects/racket/control.rkt` (269 lines)
  - shift/reset macros built on primitives
  - control/prompt (Felleisen style)
  - reset0/shift0 variants

**Test Infrastructure:**
- `~/Project/reference/racket/pkgs/racket-test/tests/racket/prompt-sfs.scm` (82 lines) - Stack frame/memory tests
- `~/Project/reference/racket/pkgs/racket-test/tests/racket/contract/prompt-tag.rkt` (182 lines) - Contract tests
- `~/Project/reference/racket/pkgs/racket-test/tests/racket/stress/prompt-mem-use.rkt` (76 lines) - Memory stress tests
- `~/Project/reference/racket/pkgs/racket-benchmarks/tests/racket/benchmarks/control/cont.rkt` (32 lines) - Performance benchmarks

#### Gauche (Primary Test Reference)

**Test Suites:**
- `~/Project/reference/Gauche/tests/continuation.scm` (796 lines)
  - Comprehensive call/cc tests
  - dynamic-wind tests with error handling
  - Prompt/abort tests
  - Continuation marks

- `~/Project/reference/Gauche/tests/partcont.scm` (400+ lines)
  - shift/reset tests with cross-implementation annotations
  - Interaction with call/cc, dynamic-wind, parameters
  - guard/error handling interaction

- `~/Project/reference/Gauche/tests/partcont-racket.scm` - Racket compatibility tests
- `~/Project/reference/Gauche/tests/partcont-srfi-226.scm` - SRFI-226 compliance

**Key Feature:** Tests annotated with expected behavior across implementations:
```scheme
;; native : [d01][d02][d04][d01][d03][d04]
;; meta   : [d01][d02][d04][d01][d03][d04]
;; srfi226: [d01][d02][d04][d01][d03][d04]
;; racket : [d01][d02][d04][d01][d03][d04]
```

#### Chibi-scheme

**Implementation:**
- `~/Project/reference/chibi-scheme/vm.c` - Simpler call/cc implementation

**Tests:**
- `~/Project/reference/chibi-scheme/tests/basic/test08-callcc.scm` (35 lines) - Pythagorean triple backtracking
- `~/Project/reference/chibi-scheme/tests/r7rs-tests.scm` (2332 lines) - R7RS compliance

### R7RS Specification

- Section 6.10: Control features (call/cc, dynamic-wind)
- Section 6.11: Exceptions (guard, raise)

### SRFI References

- **SRFI-226**: Control Features (delimited continuations, continuation marks)
  - Comprehensive specification for delimited continuations
  - Test suite available in Gauche

## Success Criteria

- [x] `call/cc` basic functionality works (100% of tests pass - 1159/1159)
- [x] Continuations satisfy `procedure?` predicate
- [x] Continuation invocation escapes through library functions (now natively via CPS)
- [x] `dynamic-wind` properly interacts with continuations
- [x] Continuation capture inside dynamic-wind works (nested + re-entry)
- [x] Regression test added for dynamic-wind + call/cc re-entry
- [ ] `(patina control)` library provides shift/reset
- [x] Exception handling (`guard`, `raise`) works correctly (26/26 tests)
- [x] Chibi's r7rs-tests.scm passes (1159/1159 - 100%)
- [x] No regression in existing TCO tests
- [x] Performance acceptable for common use cases
- [x] CPS is the default and only evaluation mode (2025-12-12)
