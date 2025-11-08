# Debug Mode Implementation Plan

**Date:** 2025-11-07
**Status:** Ready to Implement
**Priority:** High (Critical for macro development)
**Based on:** PRD/DEBUG_MODE.md

---

## Executive Summary

Implement a comprehensive debug mode system accessible via Scheme procedures like `(debug-enable 'eval)` and `(debug-mode 'on)`. This will be **essential for macro development** where we need to trace syntax transformations.

**Key Decision:** Start with **Scheme-based API** (not REPL commands) for consistency with the language.

---

## Phase 1: Minimal Debug Mode (This Week)

**Goal:** Get basic eval tracing working for immediate use in macro development.

### 1.1 Core Data Structures

```rust
// src/eval/debug.rs (new file)

use std::cell::RefCell;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DebugStage {
    Lex,
    Parse,
    Eval,
    Apply,
    Env,
    Expand,  // Critical for macros!
}

pub struct DebugConfig {
    enabled_stages: RefCell<HashSet<DebugStage>>,
    indent_level: RefCell<usize>,
}

impl DebugConfig {
    pub fn new() -> Self {
        Self {
            enabled_stages: RefCell::new(HashSet::new()),
            indent_level: RefCell::new(0),
        }
    }

    pub fn enable(&self, stage: DebugStage) {
        self.enabled_stages.borrow_mut().insert(stage);
    }

    pub fn disable(&self, stage: DebugStage) {
        self.enabled_stages.borrow_mut().remove(&stage);
    }

    pub fn is_enabled(&self, stage: DebugStage) -> bool {
        self.enabled_stages.borrow().contains(&stage)
    }

    pub fn clear(&self) {
        self.enabled_stages.borrow_mut().clear();
    }

    pub fn enable_all(&self) {
        let mut stages = self.enabled_stages.borrow_mut();
        stages.insert(DebugStage::Lex);
        stages.insert(DebugStage::Parse);
        stages.insert(DebugStage::Eval);
        stages.insert(DebugStage::Apply);
        stages.insert(DebugStage::Env);
        stages.insert(DebugStage::Expand);
    }

    pub fn indent(&self) {
        *self.indent_level.borrow_mut() += 1;
    }

    pub fn dedent(&self) {
        let mut level = self.indent_level.borrow_mut();
        if *level > 0 {
            *level -= 1;
        }
    }

    pub fn current_indent(&self) -> String {
        "  ".repeat(*self.indent_level.borrow())
    }
}
```

### 1.2 Integration with Evaluator

```rust
// src/eval/mod.rs - Add to Evaluator struct

pub struct Evaluator {
    global_env: Rc<Environment>,
    debug: Rc<DebugConfig>,  // Add this
}

impl Evaluator {
    pub fn new() -> Self {
        let global_env = Rc::new(Environment::new());
        Self::install_primitives(&global_env);

        Self {
            global_env: global_env.clone(),
            debug: Rc::new(DebugConfig::new()),  // Add this
        }
    }

    // Add debug helper
    fn debug_eval(&self, msg: &str) {
        if self.debug.is_enabled(DebugStage::Eval) {
            eprintln!("[EVAL]{} {}", self.debug.current_indent(), msg);
        }
    }
}
```

### 1.3 Scheme API - Primitive Functions

Add these primitives:

```rust
// src/eval/primitives/mod.rs - Add to dispatcher

"debug-enable" => debug::debug_enable(self, args),
"debug-disable" => debug::debug_disable(self, args),
"debug-clear" => debug::debug_clear(self, args),
"debug-status" => debug::debug_status(self, args),
"debug-mode" => debug::debug_mode(self, args),  // Convenience: on/off/all
```

```rust
// src/eval/primitives/debug.rs (new file)

use super::super::error::EvalError;
use super::super::Evaluator;
use super::super::debug::DebugStage;
use crate::value::Value;

pub(super) fn debug_enable(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "debug-enable")?;

    match &args[0] {
        Value::Symbol(stage_name) => {
            let stage = match stage_name.as_str() {
                "lex" => DebugStage::Lex,
                "parse" => DebugStage::Parse,
                "eval" => DebugStage::Eval,
                "apply" => DebugStage::Apply,
                "env" => DebugStage::Env,
                "expand" => DebugStage::Expand,
                _ => return Err(EvalError::TypeError(format!(
                    "Unknown debug stage: {}. Valid: lex, parse, eval, apply, env, expand",
                    stage_name
                ))),
            };

            evaluator.debug.enable(stage);
            Ok(Value::Symbol("enabled".to_string()))
        }
        _ => Err(EvalError::TypeError(
            "debug-enable expects a symbol (lex, parse, eval, apply, env, expand)".to_string()
        )),
    }
}

pub(super) fn debug_disable(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "debug-disable")?;

    match &args[0] {
        Value::Symbol(stage_name) => {
            let stage = match stage_name.as_str() {
                "lex" => DebugStage::Lex,
                "parse" => DebugStage::Parse,
                "eval" => DebugStage::Eval,
                "apply" => DebugStage::Apply,
                "env" => DebugStage::Env,
                "expand" => DebugStage::Expand,
                _ => return Err(EvalError::TypeError(format!(
                    "Unknown debug stage: {}", stage_name
                ))),
            };

            evaluator.debug.disable(stage);
            Ok(Value::Symbol("disabled".to_string()))
        }
        _ => Err(EvalError::TypeError(
            "debug-disable expects a symbol".to_string()
        )),
    }
}

pub(super) fn debug_clear(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 0, "debug-clear")?;
    evaluator.debug.clear();
    Ok(Value::Symbol("cleared".to_string()))
}

pub(super) fn debug_status(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 0, "debug-status")?;

    let stages = vec!["lex", "parse", "eval", "apply", "env", "expand"];
    let mut enabled = Vec::new();

    for stage_name in stages {
        let stage = match stage_name {
            "lex" => DebugStage::Lex,
            "parse" => DebugStage::Parse,
            "eval" => DebugStage::Eval,
            "apply" => DebugStage::Apply,
            "env" => DebugStage::Env,
            "expand" => DebugStage::Expand,
            _ => continue,
        };

        if evaluator.debug.is_enabled(stage) {
            enabled.push(Value::Symbol(stage_name.to_string()));
        }
    }

    Ok(evaluator.list_from_vec(enabled))
}

pub(super) fn debug_mode(
    evaluator: &Evaluator,
    args: Vec<Value>,
) -> Result<Value, EvalError> {
    evaluator.check_arity_exact(&args, 1, "debug-mode")?;

    match &args[0] {
        Value::Symbol(mode) => match mode.as_str() {
            "on" | "all" => {
                evaluator.debug.enable_all();
                Ok(Value::Symbol("all-enabled".to_string()))
            }
            "off" => {
                evaluator.debug.clear();
                Ok(Value::Symbol("disabled".to_string()))
            }
            _ => Err(EvalError::TypeError(
                "debug-mode expects 'on, 'off, or 'all".to_string()
            )),
        },
        _ => Err(EvalError::TypeError(
            "debug-mode expects a symbol".to_string()
        )),
    }
}
```

### 1.4 Add Debug Tracing to Evaluator

```rust
// src/eval/mod.rs - Add tracing to eval_in_env

pub(super) fn eval_in_env(&self, expr: &Value, env: &Rc<Environment>) -> Result<Value, EvalError> {
    // Debug trace entry
    if self.debug.is_enabled(DebugStage::Eval) {
        eprintln!("[EVAL]{} Evaluating: {}", self.debug.current_indent(), expr);
        self.debug.indent();
    }

    let result = match expr {
        Value::Integer(_) | Value::BigInteger(_) | Value::Rational(_)
        | Value::Real(_) | Value::Complex(_, _) => Ok(expr.clone()),

        Value::Boolean(_) => Ok(expr.clone()),
        Value::String(_) => Ok(expr.clone()),
        Value::Character(_) => Ok(expr.clone()),

        Value::Symbol(name) => {
            if self.debug.is_enabled(DebugStage::Env) {
                eprintln!("[ENV]{} Lookup: '{}'", self.debug.current_indent(), name);
            }
            env.get(name)
                .ok_or_else(|| EvalError::UndefinedVariable(name.clone()))
        }

        Value::Pair(_) => self.eval_list(expr, env),

        Value::Null => Ok(Value::Null),
        Value::Vector(_) => Ok(expr.clone()),
        Value::Bytevector(_) => Ok(expr.clone()),

        Value::Procedure(_) => Ok(expr.clone()),
        Value::Unspecified => Ok(expr.clone()),
        Value::Values(_) => Ok(expr.clone()),
    };

    // Debug trace exit
    if self.debug.is_enabled(DebugStage::Eval) {
        self.debug.dedent();
        match &result {
            Ok(val) => eprintln!("[EVAL]{} => {}", self.debug.current_indent(), val),
            Err(e) => eprintln!("[EVAL]{} => ERROR: {}", self.debug.current_indent(), e),
        }
    }

    result
}
```

### 1.5 Usage Examples

```scheme
;; Enable eval tracing
(debug-enable 'eval)

;; Test it
(define (factorial n)
  (if (= n 0)
      1
      (* n (factorial (- n 1)))))

(factorial 3)
;; Output:
;; [EVAL] Evaluating: (factorial 3)
;; [EVAL]   Evaluating: 3
;; [EVAL]   => 3
;; [EVAL]   Evaluating: (= n 0)
;; [EVAL]     Evaluating: n
;; [EVAL]     => 3
;; [EVAL]     Evaluating: 0
;; [EVAL]     => 0
;; [EVAL]   => #f
;; [EVAL]   Evaluating: (* n (factorial (- n 1)))
;; ...

;; Disable specific stage
(debug-disable 'eval)

;; Enable all stages
(debug-mode 'on)

;; Disable all
(debug-mode 'off)

;; Check what's enabled
(debug-status)  ; => (eval apply)
```

---

## Phase 2: Macro Expansion Tracing (Next Week)

**Critical for macro development!**

### 2.1 Add Expand Stage

```rust
// When we implement macros, add:

fn debug_expand(&self, before: &Value, after: &Value) {
    if self.debug.is_enabled(DebugStage::Expand) {
        eprintln!("[EXPAND]{} Before: {}", self.debug.current_indent(), before);
        self.debug.indent();
        eprintln!("[EXPAND]{} After:  {}", self.debug.current_indent(), after);
        self.debug.dedent();
    }
}
```

### 2.2 Usage for Macro Debugging

```scheme
;; Enable macro expansion tracing
(debug-enable 'expand)

;; Define a macro
(define-syntax when
  (syntax-rules ()
    ((when test result ...)
     (if test (begin result ...)))))

;; Use it
(when #t (+ 1 2) (* 3 4))

;; Output:
;; [EXPAND] Before: (when #t (+ 1 2) (* 3 4))
;; [EXPAND]   After:  (if #t (begin (+ 1 2) (* 3 4)))
```

---

## Phase 3: S-expression Output (Future)

**For advanced debugging and programmatic analysis**

### 3.1 Alternative Output Format

```rust
// Add output_format option to DebugConfig

pub enum DebugFormat {
    Log,   // [EVAL] ... (current)
    Sexp,  // (debug eval ...) (future)
}

// Add primitive:
(debug-format 'sexp)  ; Switch to S-expression output
(debug-format 'log)   ; Switch back to log format
```

### 3.2 S-expression Trace Format

```scheme
;; Enable sexp format
(debug-format 'sexp)
(debug-enable 'eval)

;; Evaluate
(+ 1 2)

;; Output (as valid Scheme):
(debug eval (expr (+ 1 2)) (depth 0))
(debug eval (expr 1) (result 1) (depth 1))
(debug eval (expr 2) (result 2) (depth 1))
(debug apply (proc +) (args 1 2) (result 3) (depth 1))
(debug eval (result 3) (depth 0))
```

**Benefits:**
- Can capture trace to a variable: `(define trace (debug-capture ...))`
- Can filter/analyze: `(filter (lambda (e) (eq? (cadr e) 'apply)) trace)`
- Can replay or transform traces

---

## Implementation Checklist

### Week 1: Basic Debug Mode
- [ ] Create `src/eval/debug.rs` with `DebugConfig` and `DebugStage`
- [ ] Add `debug: Rc<DebugConfig>` to `Evaluator`
- [ ] Create `src/eval/primitives/debug.rs` with primitives
- [ ] Register debug primitives in dispatcher
- [ ] Add debug tracing to `eval_in_env` (eval stage)
- [ ] Add debug tracing to `apply` (apply stage)
- [ ] Test with simple expressions
- [ ] Document in README

### Week 2: Macro Expansion Tracing
- [ ] Add `Expand` stage to `DebugStage` enum
- [ ] Add `debug_expand` calls to macro expander (when implemented)
- [ ] Test with `when`/`unless` macros
- [ ] Create examples in `tests/fixtures/examples/debug/`

### Future: Advanced Features
- [ ] S-expression output format
- [ ] Debug trace capture to variable
- [ ] Programmable filtering
- [ ] Performance profiling mode

---

## Testing Strategy

### Unit Tests

```rust
#[test]
fn test_debug_enable() {
    let mut interp = Interpreter::new();
    interp.eval_str("(debug-enable 'eval)").unwrap();
    // Check that eval stage is enabled
}

#[test]
fn test_debug_mode_all() {
    let mut interp = Interpreter::new();
    interp.eval_str("(debug-mode 'on)").unwrap();
    let status = interp.eval_str("(debug-status)").unwrap();
    // Check that all stages are in status list
}
```

### Integration Tests

```scheme
;; tests/fixtures/examples/debug/01_basic_tracing.scm

(debug-enable 'eval)
(+ 1 2)
(debug-disable 'eval)

;; Verify output contains [EVAL] lines
```

---

## Benefits for Macro Development

1. **Understand expansion:** See exactly how `when` expands to `if`
2. **Debug hygiene:** Track renamed identifiers
3. **Trace nested expansion:** See macro calls within macros
4. **Performance:** Identify slow expansions
5. **Learning:** Understand how macros work

---

## Success Criteria

- ✅ Can enable/disable debug stages via Scheme code
- ✅ Eval tracing shows nested evaluation with proper indentation
- ✅ Apply tracing shows procedure calls and arguments
- ✅ Expand tracing shows before/after macro expansion
- ✅ Zero performance impact when disabled
- ✅ Clear, readable output for debugging

---

## Next Steps

1. **This week:** Implement Phase 1 (basic debug mode)
2. **Next week:** Add macro expansion tracing (Phase 2)
3. **Future:** S-expression output format (Phase 3)

**Priority:** HIGH - Start immediately, needed for macro development!
