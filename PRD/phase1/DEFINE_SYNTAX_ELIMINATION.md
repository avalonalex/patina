# DefineSyntax Elimination from CoreExpr

**Status:** Research Complete, Blocked on Prerequisite Work
**Goal:** Remove `DefineSyntax` from CoreExpr to enable clean VM compilation
**Impact:** Cleaner separation between compile-time (macros) and runtime (evaluation)

---

## Prerequisites: Fix Existing Macro Bugs First

Before starting this refactoring, we should fix the existing macro-related failures in the chibi r7rs test suite. This establishes a clean baseline and ensures we can detect regressions.

### Current Macro Test Status (Section 4.3 Macros)

- **Tests:** 26 total
- **Passed:** 16 (61.5%)
- **Failed:** 1 (3.8%)
- **Errors:** 9 (34.6%)

### Bugs to Fix Before Refactoring

| Bug | Error Message | Likely Cause | Priority |
|-----|---------------|--------------|----------|
| 1 | `syntax-rules literals must be a proper list` | Parser edge case with unusual literal lists | High |
| 2 | `Not a procedure: #<macro:foo399>` | Macro not expanding, returned as value | High |
| 3 | `Not a procedure: #<macro:swap!>` | Same - macro in `let` body not expanding | High |
| 4 | `Macro name must be a symbol` | Identifier vs Symbol handling in define-syntax | Medium |
| 5 | `Not a procedure: 1` | Expansion failure causing wrong eval | Medium |
| 6 | `Not a procedure: ok` | Hygiene issue with `cond` `=>` clause | Medium |
| 7 | `Undefined variable: sequence3` | Cascading from bug #1 | Low (fixes itself) |
| 8 | `Undefined variable: bar`, `ff` | Cascading failures | Low (fixes itself) |

### Why Fix First?

1. **Establish baseline**: Clear pass/fail state before refactoring
2. **Regression detection**: Can verify no regressions after changes
3. **Root cause clarity**: Bugs may reveal architecture issues that inform refactoring
4. **Lower risk**: Safer to fix bugs in stable code than during refactoring

### Recommended Sequence

1. **Phase 0 (Current)**: Fix macro bugs (this section)
   - Target: 24+ passing in macro section (up from 16)
   - Estimate: 2-3 focused bug fixes

2. **Phase 1-4**: DefineSyntax elimination (rest of this document)
   - Proceed once macro tests are stable

---

## Executive Summary

Currently `DefineSyntax` exists in CoreExpr, which means macro definitions flow through to the evaluator. This is problematic for VM backends because macros are a compile-time concept with no runtime representation in bytecode.

**Solution:** Fully expand macros during desugaring by processing `define-syntax` immediately when encountered, rather than deferring to evaluation.

---

## Current Architecture

```
Source → Parse → Value AST
                    ↓
            ┌──────────────────────────────┐
            │ Desugarer (with environment) │
            │                              │
            │ When sees define-syntax:     │
            │   → Creates CoreExpr::DefineSyntax │
            │   → Defers compilation to eval    │
            └──────────────────────────────┘
                    ↓
            CoreExpr (includes DefineSyntax)
                    ↓
            ┌──────────────────────────────┐
            │ Evaluator (core_eval.rs)     │
            │                              │
            │ When evaluates DefineSyntax: │
            │   → Compiles syntax-rules    │
            │   → Stores in environment    │
            └──────────────────────────────┘
```

### Key Files in Current Flow

| Stage | File | Function |
|-------|------|----------|
| Desugar define-syntax | `patina-frontend/src/desugarer/mod.rs:567-598` | `desugar_define_syntax()` |
| CoreExpr variant | `patina-core/src/core_expr.rs:144-153` | `DefineSyntax { ... }` |
| Evaluate DefineSyntax | `patina-tree-walker/src/eval/core_eval.rs:276-298` | match arm |
| Compile syntax-rules | `patina-tree-walker/src/eval/mod.rs:1671-1708` | `compile_syntax_rules()` |

---

## Target Architecture

```
Source → Parse → Value AST
                    ↓
            ┌──────────────────────────────┐
            │ Desugarer (with environment) │
            │                              │
            │ When sees define-syntax:     │
            │   → Compiles syntax-rules    │  ← NEW: compile here
            │   → Stores in environment    │  ← NEW: mutate env here
            │   → Returns Unspecified      │  ← No CoreExpr needed
            └──────────────────────────────┘
                    ↓
            CoreExpr (NO DefineSyntax)
                    ↓
            ┌──────────────────────────────┐
            │ Evaluator                    │
            │   → Pure evaluation          │
            │   → No macro compilation     │
            └──────────────────────────────┘
```

### Benefits

1. **VM-Ready**: Bytecode compiler doesn't need to handle macros
2. **Cleaner Separation**: Compile-time vs runtime clearly separated
3. **Simpler CoreExpr**: One fewer variant to handle
4. **Correct Semantics**: Macros are compile-time, should be processed at compile-time

---

## Implementation Plan

### Phase 1: Move Compilation to Desugarer

**Goal:** Desugarer compiles and installs macros, returns no CoreExpr

#### 1.1 Add Macro Compilation to Desugarer

The desugarer already has `compile_syntax_rules_with_scopes()` for `let-syntax`. We need to use this for `define-syntax` too.

**File:** `patina-frontend/src/desugarer/mod.rs`

```rust
// Current (lines 567-598):
fn desugar_define_syntax(&self, args: &Value) -> Result<CoreExpr> {
    // ... parse name and transformer ...
    Ok(CoreExpr::DefineSyntax {
        name,
        transformer: Rc::new(transformer.clone()),
        definition_scopes: self.current_scopes.clone(),
    })
}

// Target:
fn desugar_define_syntax(&self, args: &Value) -> Result<CoreExpr> {
    // 1. Parse name and transformer
    let (name, transformer) = self.parse_define_syntax_args(args)?;

    // 2. Compile the macro NOW (not at eval time)
    let compiled_macro = self.compile_syntax_rules_with_scopes(
        &transformer,
        name.clone(),
        self.env.as_ref().ok_or(DesugarError::NoEnvironment)?,
        &self.current_scopes,
    )?;

    // 3. Install in environment immediately
    let macro_value = Value::Macro(Rc::new(compiled_macro));
    self.env.as_ref().unwrap().define(name.to_string(), macro_value);

    // 4. Return a no-op (or we could return nothing and handle specially)
    Ok(CoreExpr::Literal(Rc::new(Value::Unspecified)))
}
```

#### 1.2 Handle Environment Mutation

**Challenge:** The desugarer currently takes `&self`, but we need to mutate the environment.

**Options:**

A. **Make environment `Rc<Environment>`** (already is) - Just call `define()` which uses `RefCell`
   - Pros: Simple, environment already supports this
   - Cons: Desugarer has side effects (but so does current approach)

B. **Return macro bindings separately** - Desugarer returns `(CoreExpr, Vec<MacroBinding>)`
   - Pros: Pure desugaring, explicit side effects
   - Cons: More complex API, caller must install macros

C. **Two-pass desugaring** - First pass collects macros, second pass expands
   - Pros: Clean separation
   - Cons: Doesn't work for interleaved definitions

**Recommendation:** Option A - Environment already uses `RefCell`, and we're just moving the mutation from eval to desugar.

#### 1.3 Handle Missing Environment

Currently `env` is `Option<Rc<Environment>>`. For `define-syntax` we MUST have an environment.

```rust
fn desugar_define_syntax(&self, args: &Value) -> Result<CoreExpr> {
    let env = self.env.as_ref().ok_or_else(|| {
        DesugarError::InvalidSyntax(
            "define-syntax requires an environment context".to_string()
        )
    })?;
    // ...
}
```

### Phase 2: Remove DefineSyntax from CoreExpr

#### 2.1 Remove Variant

**File:** `patina-core/src/core_expr.rs`

```rust
pub enum CoreExpr {
    Literal(Rc<Value>),
    Var { name: Symbol, scopes: ScopeSet },
    Quote(Rc<Value>),
    Quasiquote(Rc<Value>),
    Lambda { ... },
    If { ... },
    Set { ... },
    Begin(Vec<CoreExpr>),
    Define { name: Symbol, value: Rc<CoreExpr> },
    // REMOVED: DefineSyntax { ... },
    Import { import_sets: Vec<Value> },
    Parameterize { ... },
    Expand { expr: Rc<CoreExpr> },
    CaseLambda { clauses: Vec<CaseLambdaClause> },
    App { ... },
    Apply { ... },
    PrimCall { ... },
    Let { ... },
}
```

#### 2.2 Remove from Evaluator

**File:** `patina-tree-walker/src/eval/core_eval.rs`

Remove the match arm for `CoreExpr::DefineSyntax` (lines 276-298).

#### 2.3 Remove from Display, map_children, etc.

**File:** `patina-core/src/core_expr.rs`

Remove all handling of `DefineSyntax` in:
- `impl Display for CoreExpr` (lines 438-443)
- `map_children()` (lines 316-324)
- `kind()` (line 260)

### Phase 3: Update let-syntax/letrec-syntax

These already compile macros during desugaring (lines 793-895), so they should continue to work. However, verify that:

1. They don't produce any `DefineSyntax` CoreExpr
2. They correctly install macros in the local environment
3. The desugared body has access to those macros

**Current behavior (correct):**
```rust
fn desugar_let_syntax_impl(&self, args: &Value, is_letrec: bool) -> Result<CoreExpr> {
    // ... compile macros and install in body_env ...

    // Desugar body with environment containing macros
    let body_desugarer = self.with_new_env(body_env, definition_scopes);
    let desugared_body = body.iter()
        .map(|e| body_desugarer.desugar(e))
        .collect::<Result<_>>()?;

    // Return just the desugared body - macros are already expanded
    Ok(CoreExpr::Begin(desugared_body))
}
```

### Phase 4: Move compile_syntax_rules

Currently `compile_syntax_rules` lives in `patina-tree-walker/src/eval/mod.rs`. Since it's now called from the desugarer (in `patina-frontend`), we should move it to a shared location.

**Options:**

A. **Move to patina-macros** - Natural home for macro compilation
   - Function: `patina_macros::compile_syntax_rules()`
   - Called from: desugarer and evaluator (for backward compat during transition)

B. **Move to patina-core** - Along with CompiledMacro
   - Function: `patina_core::compile_syntax_rules()`
   - Keeps all macro types together

**Recommendation:** Option A - patina-macros already handles macro expansion, compilation is logically related.

**New signature:**
```rust
// patina-macros/src/lib.rs
pub fn compile_syntax_rules(
    expr: &Value,                    // (syntax-rules ...)
    name: Rc<str>,
    env: &Rc<Environment>,           // For free variable capture
    definition_scopes: &ScopeSet,
) -> Result<CompiledMacro, MacroError>
```

---

## Dependency Changes

### Before

```
patina-frontend (desugarer)
    → patina-core (CoreExpr::DefineSyntax)
    → patina-macros (expand_macro)

patina-tree-walker (evaluator)
    → patina-core (CoreExpr::DefineSyntax)
    → patina-macros (CompiledMacro)
    → compile_syntax_rules (local)
```

### After

```
patina-frontend (desugarer)
    → patina-core (NO DefineSyntax)
    → patina-macros (expand_macro, compile_syntax_rules)  ← NEW

patina-tree-walker (evaluator)
    → patina-core (NO DefineSyntax)
    → patina-macros (CompiledMacro)  ← compile_syntax_rules removed
```

---

## Edge Cases

### 1. Interleaved Definitions

```scheme
(define-syntax foo ...)
(define x (foo 1))      ; foo must be available
(define-syntax bar ...)
(bar x)                 ; bar must be available, x must be defined
```

**Solution:** Process forms sequentially. After desugaring `(define-syntax foo ...)`, `foo` is in the environment and available for subsequent forms.

### 2. Mutual Recursion (letrec-syntax)

```scheme
(letrec-syntax
  ((foo (syntax-rules () ((foo x) (bar x))))
   (bar (syntax-rules () ((bar x) x))))
  (foo 1))
```

**Already handled:** `desugar_let_syntax_impl` with `is_letrec: true` compiles all macros first, then installs them, so they can reference each other.

### 3. Macros in Library Files

Library loading already uses the desugarer with an environment. The `define-syntax` forms in library files will be compiled and installed during library loading.

### 4. REPL Interaction

```
> (define-syntax my-when ...)
> (my-when #t "hi")
```

The REPL evaluates each form through the full pipeline. After desugaring `(define-syntax my-when ...)`:
1. Macro is compiled and installed in environment
2. Returns `CoreExpr::Literal(Unspecified)`
3. Evaluates to `Unspecified` (correct REPL output)
4. Next form sees `my-when` in environment

### 5. Error in Macro Definition

```scheme
(define-syntax bad
  (syntax-rules ()
    ((bad x) (syntax-error "always fails"))))
```

**Question:** When is the error raised?
- Currently: At eval time when `DefineSyntax` is evaluated
- After change: At desugar time when `define-syntax` is processed

**Impact:** Errors happen earlier, which is generally better. However, if we're loading a library but never use the macro, the error would now be raised.

**Decision:** This is acceptable - malformed macros should fail at definition time, not use time.

### 6. Hygiene: Free Variables in Templates

```scheme
(define free-var 42)
(define-syntax use-free
  (syntax-rules ()
    ((use-free) free-var)))

(let ((free-var 100))
  (use-free))  ; Should return 42, not 100
```

**Requirement:** When compiling the macro, we must capture the environment so that `free-var` in the template refers to the `free-var` visible at definition time.

**How it works now:**
1. `CoreExpr::DefineSyntax` stores `definition_scopes`
2. Evaluator calls `compile_syntax_rules(transformer, env, definition_scopes)`
3. Compiler stores `env` in `CompiledMacro` for free variable lookup

**After change:**
1. Desugarer calls `compile_syntax_rules(transformer, env, current_scopes)`
2. Same `env` capture happens, just earlier
3. `definition_scopes` comes from `self.current_scopes` in desugarer

**Key insight:** The desugarer already has access to the same environment that the evaluator would use. The `let-syntax` implementation already does this correctly.

### 7. Macro-Introduced Bindings

```scheme
(define-syntax make-binding
  (syntax-rules ()
    ((make-binding val)
     (let ((x val)) x))))  ; 'x' is macro-introduced

(let ((x 999))
  (make-binding 1))  ; Should return 1, not 999
```

**How it works:** The flip-scope algorithm handles this:
1. Fresh scope created for each expansion
2. Template `x` gets the macro scope added
3. User's `x` doesn't have the macro scope
4. Lookup distinguishes them via scope subset matching

**Impact of this change:** None. The expansion happens during desugaring (already true). The scope flipping happens in `expand_macro`. We're not changing any of that.

---

## Testing Strategy

### Unit Tests

1. **Desugarer compiles macros:** Verify macro is in environment after desugaring `define-syntax`
2. **No DefineSyntax in output:** Verify CoreExpr tree contains no DefineSyntax
3. **Macro expansion works:** Verify subsequent forms are expanded correctly

### Integration Tests

1. **All existing macro tests pass:** `cargo test --package patina-tests`
2. **let-syntax/letrec-syntax still work:** These already compile at desugar time
3. **Library loading works:** Libraries with macros load correctly
4. **REPL interaction works:** Interactive macro definitions work

### Regression Tests

1. **Shadowing:** `(let ((let 1)) let)` returns 1, not an error
2. **Hygiene:** All hygiene tests in `scheme_base.rs` pass
3. **Error messages:** Macro errors show reasonable locations

---

## Migration Checklist

### Phase 0: Fix Existing Macro Bugs (PREREQUISITE)

- [ ] **Bug 1:** Fix `syntax-rules literals must be a proper list`
- [ ] **Bug 2-3:** Fix `Not a procedure: #<macro:...>` (macro not expanding)
- [ ] **Bug 4:** Fix `Macro name must be a symbol` (Identifier handling)
- [ ] **Bug 5-6:** Fix remaining expansion/hygiene issues
- [ ] **Verify:** Macro section passes 24+ tests (up from 16)

### Phase 1: Move Compilation to Desugarer

- [ ] **Phase 1.1:** Update `desugar_define_syntax` to compile and install macro
- [ ] **Phase 1.2:** Verify environment mutation works (RefCell)
- [ ] **Phase 1.3:** Handle missing environment error case

### Phase 2: Remove DefineSyntax from CoreExpr

- [ ] **Phase 2.1:** Remove `DefineSyntax` from CoreExpr enum
- [ ] **Phase 2.2:** Remove from evaluator match arm
- [ ] **Phase 2.3:** Remove from Display, map_children, kind

### Phase 3-4: Cleanup

- [ ] **Phase 3:** Verify let-syntax/letrec-syntax still work
- [ ] **Phase 4:** Move `compile_syntax_rules` to patina-macros (optional, can defer)
- [ ] **Testing:** All tests pass, no regressions in macro section
- [ ] **Cleanup:** Remove dead code, update documentation

---

## Relationship with Binding Objects Design

See `PRD/phase1/BINDING_OBJECTS_DESIGN.md` for the longer-term vision.

### Current Hygiene Model

Patina currently resolves hygiene at **evaluation time** using scope sets:

```
1. Macro expansion adds scopes to identifiers
2. Desugarer preserves scopes in CoreExpr::Var { name, scopes }
3. Evaluator uses get_with_scopes() for subset matching
```

This means:
- `Var { name, scopes }` carries hygiene info through CoreExpr
- `Set { var, scopes, value }` also carries scopes
- `Lambda { params: Vec<ScopedParam>, ... }` has scoped parameters

### How DefineSyntax Elimination Interacts

**Key observation:** This refactoring does NOT change the hygiene model. It only moves WHEN macros are compiled:

| Aspect | Before | After |
|--------|--------|-------|
| Macro compilation | Eval time | Desugar time |
| Scope capture | `definition_scopes` in CoreExpr | `definition_scopes` in CompiledMacro |
| Expansion | Desugar time | Desugar time (unchanged) |
| Hygiene resolution | Eval time | Eval time (unchanged) |

The `definition_scopes` field in `CoreExpr::DefineSyntax` was already being passed to `compile_syntax_rules`. We're just doing that compilation earlier.

### Why This is Safe

1. **Scope sets are immutable**: Once captured at definition time, they don't change
2. **Expansion environment available**: Desugarer already has the environment for macro lookup
3. **Existing let-syntax works**: The `let-syntax` implementation already compiles at desugar time

### Future: Binding Objects

The BINDING_OBJECTS_DESIGN proposes resolving hygiene at **expansion time** using `BindingId`:

```
1. Expansion creates fresh BindingId for each binding
2. Variable references resolved to BindingId during expansion
3. IR contains LocalVar(BindingId), no scope matching at eval time
```

This is a more fundamental change that would:
- Eliminate `Var { name, scopes }` in favor of `LocalVar(BindingId)`
- Eliminate scope-based lookup entirely
- Make evaluation O(1) lookup instead of O(n) subset matching

**Relationship to this work:**

| This Refactoring | Binding Objects |
|-----------------|-----------------|
| Moves macro compilation earlier | Moves hygiene resolution earlier |
| Removes compile-time artifact from CoreExpr | Removes runtime hygiene from CoreExpr |
| Prerequisite: No | Prerequisite: Yes (or parallel) |

This refactoring is **independent** of binding objects. It can be done before, after, or in parallel. However, both changes move work earlier in the pipeline, which is the right direction for VM support.

### Recommended Sequence

1. **Now:** DefineSyntax elimination (this document)
   - Removes compile-time artifact from runtime IR
   - Minimal hygiene changes
   - Low risk

2. **VM Phase:** Binding objects (BINDING_OBJECTS_DESIGN)
   - Resolves hygiene at expansion time
   - Simplifies CoreExpr further
   - Higher effort, higher payoff

---

## Future Considerations

### VM Backend

With `DefineSyntax` eliminated, the VM backend becomes simpler:

```rust
fn compile(&mut self, expr: &CoreExpr) -> Bytecode {
    match expr {
        // No DefineSyntax to handle!
        CoreExpr::Literal(v) => self.emit_const(v),
        CoreExpr::Lambda { .. } => self.compile_closure(..),
        // ...
    }
}
```

### Serialization

If we ever want to serialize CoreExpr (e.g., for caching compiled code), we no longer need to handle macro transformers as data.

### JIT

JIT compilation can focus purely on runtime forms, with no compile-time artifacts to handle.

---

## Appendix: Code Locations

### Files to Modify

| File | Changes |
|------|---------|
| `patina-frontend/src/desugarer/mod.rs` | Update `desugar_define_syntax` |
| `patina-core/src/core_expr.rs` | Remove `DefineSyntax` variant |
| `patina-tree-walker/src/eval/core_eval.rs` | Remove eval arm |
| `patina-tree-walker/src/eval/mod.rs` | Remove or relocate `compile_syntax_rules` |

### Files to Verify (No Changes Expected)

| File | Reason |
|------|--------|
| `patina-frontend/src/desugarer/mod.rs` lines 793-895 | let-syntax already correct |
| `patina-macros/src/macro_expander/mod.rs` | No changes needed |
| `patina-core/src/compiled_macro.rs` | No changes needed |

---

## Summary

Removing `DefineSyntax` from CoreExpr is a straightforward refactoring that:

1. Moves macro compilation from eval-time to desugar-time
2. Eliminates a CoreExpr variant that doesn't belong at runtime
3. Prepares the codebase for VM backend development
4. Maintains all existing functionality and test coverage

The main change is in `desugar_define_syntax`: instead of creating `CoreExpr::DefineSyntax`, it compiles the macro immediately and returns `CoreExpr::Literal(Unspecified)`.
