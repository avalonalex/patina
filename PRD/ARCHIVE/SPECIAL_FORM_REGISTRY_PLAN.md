# Special Form Registry Implementation Plan

**Status:** Ready to implement
**Date:** 2025-11-13
**Goal:** Refactor special forms from 1080-line monolith to modular registry system

## Background

Currently, all special forms are implemented as methods in `special_forms.rs` (1080 lines):
- `eval_quote`, `eval_if`, `eval_define`, `eval_set`, `eval_lambda`, `eval_begin`, etc.
- Called directly from evaluator's dispatch logic
- Not extensible, not independently testable
- Difficult to maintain

**Success from Primitive Registry:** We just migrated 125 primitives to a registry system with excellent results. Special forms should follow the same pattern.

---

## Architecture Design

### Core Trait

```rust
// crates/patina-tree-walker/src/eval/special_forms/trait.rs

use super::super::{EvalError, EvalResult, Evaluator};
use patina_runtime::{Environment, Value};
use std::rc::Rc;

/// Trait for special form implementations
///
/// Special forms are syntactic constructs that don't evaluate their arguments
/// in the normal way. They have full control over evaluation order and can
/// introduce new bindings, control flow, or prevent evaluation entirely.
pub trait SpecialForm {
    /// The name of this special form (e.g., "quote", "if", "lambda")
    fn name(&self) -> &'static str;

    /// Evaluate this special form
    ///
    /// # Arguments
    ///
    /// * `evaluator` - The evaluator (for recursive evaluation)
    /// * `args` - The arguments after the special form name (unevaluated)
    /// * `env` - The environment in which to evaluate
    /// * `in_tail_position` - Whether this form is in tail position (for TCO)
    ///
    /// # Returns
    ///
    /// An `EvalResult` which can be:
    /// - `Value(v)` - A computed value
    /// - `TailCall { expr, env }` - A tail call to be trampolined
    /// - `TailCallPrimitive { proc, args }` - A tail call to a primitive
    ///
    /// # Examples
    ///
    /// For `(quote x)`, args would be the Value representing `(x)`.
    /// For `(if test then else)`, args would be `(test then else)`.
    fn eval(
        &self,
        evaluator: &Evaluator,
        args: &Value,
        env: &Rc<Environment>,
        in_tail_position: bool,
    ) -> Result<EvalResult, EvalError>;

    /// Optional: Get help text for this special form
    ///
    /// This can be used by a help system to document special forms.
    fn help(&self) -> &'static str {
        "No documentation available."
    }

    /// Optional: Validate syntax without evaluating
    ///
    /// This can be used for better error messages and static analysis.
    /// Default implementation does no validation.
    fn validate_syntax(&self, _args: &Value) -> Result<(), EvalError> {
        Ok(())
    }
}
```

### Registry

```rust
// crates/patina-tree-walker/src/eval/special_forms/registry.rs

use super::trait::SpecialForm;
use super::super::{EvalError, EvalResult, Evaluator};
use patina_runtime::{Environment, Value};
use std::collections::HashMap;
use std::rc::Rc;

/// Registry for special forms
///
/// Manages all special form implementations and provides dispatch.
pub struct SpecialFormRegistry {
    forms: HashMap<&'static str, Box<dyn SpecialForm>>,
}

impl SpecialFormRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        SpecialFormRegistry {
            forms: HashMap::new(),
        }
    }

    /// Register a special form
    pub fn register(&mut self, form: Box<dyn SpecialForm>) {
        self.forms.insert(form.name(), form);
    }

    /// Check if a special form is registered
    pub fn contains(&self, name: &str) -> bool {
        self.forms.contains_key(name)
    }

    /// Get a special form by name
    pub fn get(&self, name: &str) -> Option<&dyn SpecialForm> {
        self.forms.get(name).map(|boxed| &**boxed)
    }

    /// Evaluate a special form
    ///
    /// This is the main dispatch method called by the evaluator.
    pub fn eval(
        &self,
        name: &str,
        args: &Value,
        evaluator: &Evaluator,
        env: &Rc<Environment>,
        in_tail_position: bool,
    ) -> Result<EvalResult, EvalError> {
        let form = self.get(name).ok_or_else(|| {
            EvalError::InvalidSyntax(format!("Unknown special form: {}", name))
        })?;

        form.eval(evaluator, args, env, in_tail_position)
    }

    /// List all registered special forms (for introspection/help)
    pub fn list_forms(&self) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = self.forms.keys().copied().collect();
        names.sort();
        names
    }
}

impl Default for SpecialFormRegistry {
    fn default() -> Self {
        Self::new()
    }
}
```

---

## Implementation Plan

### Phase 1: Infrastructure (1-2 hours)

1. **Create directory structure:**
   ```
   crates/patina-tree-walker/src/eval/special_forms/
   ├── mod.rs              # Registry setup + exports
   ├── trait.rs            # SpecialForm trait definition
   ├── registry.rs         # SpecialFormRegistry implementation
   └── (individual form files to be added)
   ```

2. **Move existing special_forms.rs:**
   - Rename `special_forms.rs` to `special_forms_old.rs`
   - Keep it temporarily for reference
   - Will delete after all forms are migrated

3. **Add registry to Evaluator:**
   ```rust
   // In crates/patina-tree-walker/src/eval/mod.rs
   pub struct Evaluator {
       pub(crate) global_env: Rc<Environment>,
       pub(crate) primitive_registry: PrimitiveRegistry,
       pub(crate) special_form_registry: SpecialFormRegistry,  // NEW
       // ... rest
   }
   ```

### Phase 2: Migrate Simple Forms (2-3 hours)

Start with the simplest forms to establish the pattern:

#### 1. **Quote** (~15 lines → `quote.rs`)

```rust
// crates/patina-tree-walker/src/eval/special_forms/quote.rs

use super::trait::SpecialForm;
use crate::eval::{EvalError, EvalResult, Evaluator};
use patina_runtime::{Environment, Value};
use std::rc::Rc;

pub struct QuoteForm;

impl SpecialForm for QuoteForm {
    fn name(&self) -> &'static str {
        "quote"
    }

    fn help(&self) -> &'static str {
        "(quote datum) returns datum without evaluating it."
    }

    fn eval(
        &self,
        evaluator: &Evaluator,
        args: &Value,
        _env: &Rc<Environment>,
        _in_tail_position: bool,
    ) -> Result<EvalResult, EvalError> {
        let (quoted, rest) = evaluator.extract_pair(args)?;
        if !matches!(rest, Value::Null) {
            return Err(EvalError::InvalidSyntax(
                "quote expects exactly one argument".to_string(),
            ));
        }
        Ok(EvalResult::Value(quoted))
    }
}
```

#### 2. **Expand** (~50 lines → `expand.rs`)

Similar pattern for the `expand` form (macro expansion visualization).

#### 3. **Import** (~100 lines → `import.rs`)

The import form for library imports.

### Phase 3: Migrate Control Flow Forms (3-4 hours)

#### 4. **If** (~50 lines → `if.rs`)

Key challenge: Needs to handle tail calls properly.

```rust
pub struct IfForm;

impl SpecialForm for IfForm {
    fn name(&self) -> &'static str {
        "if"
    }

    fn help(&self) -> &'static str {
        "(if test consequent [alternate]) evaluates test and returns \
         consequent if true, alternate if false."
    }

    fn eval(
        &self,
        evaluator: &Evaluator,
        args: &Value,
        env: &Rc<Environment>,
        in_tail_position: bool,
    ) -> Result<EvalResult, EvalError> {
        // Extract test, consequent, and optional alternate
        // Evaluate test first
        // Then tail-call appropriate branch if in_tail_position
        // (Extract current logic from eval_if_impl)
    }
}
```

#### 5. **Begin** (~40 lines → `begin.rs`)

Sequential evaluation with tail call for last expression.

### Phase 4: Migrate Binding Forms (3-4 hours)

#### 6. **Define** (~65 lines → `define.rs`)

Variable and function definition.

#### 7. **Set!** (~25 lines → `set.rs`)

Assignment form.

#### 8. **Lambda** (~100 lines → `lambda.rs`)

Most complex form - procedure creation with environment capture.

### Phase 5: Migrate Complex Forms (2-3 hours)

#### 9. **Apply** (~65 lines → `apply.rs`)

Apply procedure to list of arguments.

#### 10. **Define-syntax** (~170 lines → `define_syntax.rs`)

Macro definition - largest and most complex form.

#### 11. **Quasiquote** (~300 lines → `quasiquote.rs`)

Template construction with unquote/unquote-splicing - most complex form.

### Phase 6: Update Evaluator Dispatch (1-2 hours)

Update `eval_step_impl` in `crates/patina-tree-walker/src/eval/mod.rs`:

**Before:**
```rust
match first_value {
    Value::Symbol(s) if s.as_ref() == "quote" => {
        return self.eval_quote(&rest_value);
    }
    Value::Symbol(s) if s.as_ref() == "if" => {
        return self.eval_if(&rest_value, env);
    }
    // ... 10+ more cases
}
```

**After:**
```rust
// Check if it's a special form
if let Value::Symbol(sym) = &first_value {
    if self.special_form_registry.contains(sym.as_ref()) {
        return self.special_form_registry.eval(
            sym.as_ref(),
            &rest_value,
            self,
            env,
            in_tail_position,
        );
    }
}
```

### Phase 7: Testing & Cleanup (1-2 hours)

1. **Run all tests** - ensure no regressions
2. **Delete `special_forms_old.rs`**
3. **Update documentation**
4. **Add registry introspection tests**

---

## File Organization

Final structure:

```
crates/patina-tree-walker/src/eval/special_forms/
├── mod.rs              # Registry setup + exports + helper functions
├── trait.rs            # SpecialForm trait
├── registry.rs         # SpecialFormRegistry
├── quote.rs            # QuoteForm (~20 lines)
├── expand.rs           # ExpandForm (~60 lines)
├── import.rs           # ImportForm (~110 lines)
├── if.rs               # IfForm (~60 lines)
├── begin.rs            # BeginForm (~50 lines)
├── define.rs           # DefineForm (~75 lines)
├── set.rs              # SetForm (~35 lines)
├── lambda.rs           # LambdaForm (~110 lines)
├── apply.rs            # ApplyForm (~75 lines)
├── define_syntax.rs    # DefineSyntaxForm (~180 lines)
└── quasiquote.rs       # QuasiquoteForm (~310 lines)
```

**Total:** ~1145 lines (vs 1080 before)
- Slight increase due to trait overhead
- But much better organization (12 focused files vs 1 monolith)

---

## Benefits

### 1. **Modularity**
- Each form is independently testable
- Clear module boundaries
- Easy to understand each form in isolation

### 2. **Extensibility**
- Add new special forms without modifying evaluator
- DSL extensions can add custom forms
- Gradual typing phase can add type-checking forms

### 3. **Testability**
- Unit test each form independently
- Mock evaluator for isolated testing
- Better test coverage

### 4. **Documentation**
- Each form has help text
- Can generate documentation automatically
- Syntax validation separated from evaluation

### 5. **Maintainability**
- Small focused files (20-300 lines each)
- Clear responsibilities
- Easier to review and modify

---

## Migration Strategy

### Backward Compatibility

All migrations should maintain:
- ✅ All existing tests pass
- ✅ No behavioral changes
- ✅ Internal refactor only (no API changes)

### Phased Approach

1. **Phases 1-2 (Simple forms):** Establish pattern, ~2-4 hours
2. **Phase 3 (Control flow):** Test TCO handling, ~3-4 hours
3. **Phase 4 (Binding):** Complex lambda form, ~3-4 hours
4. **Phase 5 (Complex):** Quasiquote and define-syntax, ~2-3 hours
5. **Phase 6 (Dispatch):** Update evaluator, ~1-2 hours
6. **Phase 7 (Cleanup):** Testing and docs, ~1-2 hours

**Total estimated effort:** 12-19 hours (~2-3 days)

### Risk Mitigation

- Keep `special_forms_old.rs` as reference during migration
- Migrate one form at a time with test validation
- Forms are independent - can pause/resume anytime
- No changes to public API or behavior

---

## Future Enhancements

Once registry is in place:

### 1. **Help System**
```scheme
(help 'if)
; => "(if test consequent [alternate]) evaluates test..."
```

### 2. **Syntax Validation**
- Validate syntax before evaluation
- Better error messages with line/column info
- Static analysis support

### 3. **Custom Special Forms**
```rust
// User-defined special form (for DSLs)
evaluator.register_special_form(Box::new(MyCustomForm));
```

### 4. **Form Composition**
- Combine forms to create new forms
- Macro-like expansion at special form level
- Build DSLs on top of Scheme

### 5. **Introspection**
```scheme
(list-special-forms)
; => (quote if define set! lambda begin apply ...)
```

---

## Comparison with Primitive Registry

| Aspect | Primitive Registry | Special Form Registry |
|--------|-------------------|----------------------|
| **Trait** | `PrimitiveFn` struct | `SpecialForm` trait |
| **Dispatch** | By name (string) | By name (symbol) |
| **Arguments** | Pre-evaluated | Unevaluated |
| **TCO Support** | Via `in_tail` param | Via `EvalResult` enum |
| **Complexity** | Simple (125 primitives) | Complex (12 forms, some huge) |
| **Files** | 9 category files | 12 form files |
| **Benefit** | Extensibility + help | Modularity + testability |

Both follow similar patterns but special forms are more complex due to:
- Unevaluated arguments
- Control flow handling
- Environment manipulation
- Tail call optimization requirements

---

## Next Steps

1. **Review this plan** - Validate approach
2. **Create infrastructure** - Trait + Registry
3. **Start with simple forms** - Quote, Expand
4. **Test incrementally** - One form at a time
5. **Complete migration** - All 12 forms
6. **Update documentation** - Architecture review

**Ready to proceed?** This follows the exact pattern we used successfully for primitives today! 🚀
