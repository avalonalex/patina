# Hygiene Research: Mark-and-Sweep vs Syntactic Closures

**Date**: 2025-11-22
**Status**: Research in Progress
**Purpose**: Understand how reference Scheme implementations (Chibi, Gauche) handle macro hygiene to fix Patina's hygiene bugs

## Executive Summary

Patina's current hygiene implementation has a critical bug: free variables in macro templates are resolved at **expansion time** rather than **definition time**. This causes macros to incorrectly capture user bindings.

**Test case that fails:**
```scheme
(let ((x 'outer))
  (let-syntax ((m (syntax-rules () ((m) x))))
    (let ((x 'inner))
      (m))))
;; Expected: outer
;; Got: inner
```

This document researches two main approaches to hygiene:
1. **Syntactic Closures** (Chibi Scheme)
2. **Identifier Wrapping** (Gauche Scheme)

Both are variations of "mark-and-sweep" hygiene but with different implementation strategies.

---

## Background: What is Hygiene?

Hygienic macros must satisfy two properties:

### 1. **Referential Transparency** (No Accidental Capture)
Variables introduced by a macro should not capture bindings in the macro's use-site.

**Bad (unhygienic):**
```scheme
(define-syntax bad-swap
  (syntax-rules ()
    ((bad-swap x y)
     (let ((tmp x))
       (set! x y)
       (set! y tmp)))))

(let ((tmp 1) (other 2))
  (bad-swap tmp other))  ;; BUG: macro's 'tmp' captures user's 'tmp'!
```

### 2. **Lexical Scoping** (Correct Free Variable Resolution)
Free variables in a macro template should resolve to bindings visible at **macro definition time**, not use-site.

**This is our current bug:**
```scheme
(let ((x 'outer))
  (let-syntax ((m (syntax-rules () ((m) x))))
    (let ((x 'inner))
      (m))))  ;; Should return 'outer, not 'inner
```

The macro `m` references `x` which is free in the template. It should be captured at definition time (where `x = 'outer`), not at use-site (where `x = 'inner`).

---

## Approach 1: Syntactic Closures (Chibi Scheme)

**Source:** `~/Project/reference/chibi-scheme/eval.c`

### Concept

Chibi uses **syntactic closures** (called "synclo" in the code), which wrap identifiers with:
- The **environment** where the identifier was introduced
- A **free variable list** (names that should be looked up in use-site environment)
- The **expression** itself

**Key data structure** (from eval.c):
```c
typedef struct sexp_synclo_struct {
    sexp env;           /* environment for identifier lookup */
    sexp free_vars;     /* list of symbols that should NOT be captured */
    sexp expr;          /* the wrapped expression */
    sexp rename;        /* rename mapping (optional) */
} sexp_synclo;
```

### How It Works

1. **At macro definition time:**
   - Free variables in templates are wrapped as syntactic closures
   - Each closure captures the **defining environment**
   - Free variable list marks which names come from macro use-site

2. **At expansion time:**
   - When a syntactic closure is evaluated, lookup happens in its **captured environment**
   - Unless the name is in the free variable list (then use current environment)

3. **Key function** (`eval.c:113-116`):
```c
while (!cell && key && sexp_synclop(key)) {
    if (!sexp_pairp(ls) &&
        sexp_not(sexp_memq(ctx, sexp_synclo_expr(key), sexp_synclo_free_vars(key))))
        env = sexp_synclo_env(key);  // Use captured environment!
    key = sexp_synclo_expr(key);
}
```

### Advantages
- ✅ Conceptually simple: just wrap identifiers with their environment
- ✅ Directly implements lexical scoping semantics
- ✅ Works naturally with `let-syntax` and `letrec-syntax`

### Disadvantages
- ❌ Runtime overhead: every identifier lookup checks for syntactic closure
- ❌ Memory overhead: each closure stores an environment reference
- ❌ Complex implementation: need to track free variables correctly

### Example Application to Patina

For `(let-syntax ((m (syntax-rules () ((m) x)))) ...)`:

**Current Patina behavior:**
- Template `x` → `Template::Symbol(Identifier::new("x"))`
- Expansion checks `is_bound("x")` in **current environment** → finds `'inner`

**With syntactic closures:**
- Template `x` → `Template::Symbol(Identifier { name: "x", env: outer_env })`
- Expansion looks up `"x"` in **captured `outer_env`** → finds `'outer` ✅

---

## Approach 2: Identifier Wrapping (Gauche Scheme)

**Source:** `~/Project/reference/Gauche/src/macro.c`

### Concept

Gauche wraps identifiers with their **defining environment** but uses a more sophisticated "identifier" object system.

**Key insight from macro.c:254-274:**
```c
/* Keeping hygienic reference
 *
 * INPUT FORM:
 *   Symbols in the input form are converted to identifiers at the macro
 *   definition time, encapsulating the defining environment.
 *
 * TEMPLATE:
 *   Free symbols in the template are converted to identifiers
 *   encapsulating the defining environment.
 *
 * PATTERN VARIABLES:
 *   Pattern variables in the template are substituted with values
 *   from input form (which are identifiers).
 */
```

**Key data structure** (macro.c:285-288):
```c
typedef struct {
    ScmObj renames;    /* list of (var . identifier) - mapping of input
                          symbol/identifier to fresh identifier */
    ScmObj ellipsis;   /* symbol/identifier/keyword for ellipsis */
    ...
} PatternContext;
```

### How It Works

1. **At compile time:**
   - Free symbols in templates → **identifiers** wrapping defining environment
   - Pattern variables → leave as `Template::Var` (will be substituted)
   - Build **rename table** mapping variables to fresh identifiers

2. **At expansion time:**
   - Pattern variables: substitute values from input (which are already identifiers)
   - Free variables: look up in **wrapped environment**
   - Fresh identifiers ensure no capture

3. **Key comment** (macro.c:442-443):
```c
/* Renaming: Map input variable (symbol or identifier) to fresh identifier.
   The same (eq?) variable must map to the same identifier. */
```

### Advantages
- ✅ Efficient: identifier objects can be optimized
- ✅ Clean separation: pattern variables vs free variables
- ✅ Explicit rename table makes hygiene violations easy to debug

### Disadvantages
- ❌ More complex implementation than syntactic closures
- ❌ Requires identifier object system (not just symbols)
- ❌ Need to thread environment through compilation

---

## Approach 3: Mark-and-Sweep (Theoretical R7RS Model)

R7RS-small spec describes hygiene using "marks" or "colors":

1. Each macro expansion gets a fresh **mark**
2. Every identifier introduced by the macro gets tagged with that mark
3. During lookup:
   - Compare identifier marks
   - Only match identifiers with compatible marks
   - Pattern variables transfer their marks from input to output

**Paper reference:** ["Macros That Work" by Kohlbecker et al. (1986)](https://www.cs.indiana.edu/~dyb/pubs/LaSC-5-4-pp359-375.pdf)

This is more theoretical - both Chibi and Gauche implement variations of this concept using syntactic closures/identifiers rather than explicit marks.

---

## Current Patina Implementation

### What We Have

**Location:** `crates/patina-frontend/src/macro_expander/`

**Current hygiene mechanism** (in `expander.rs:568-594`):
```rust
fn rename_identifier(&self, id: &Identifier) -> Value {
    let name = id.name();

    // Don't rename if bound in environment
    if is_gensym(name.as_ref())
        || is_special_form(name.as_ref())
        || self.is_macro(name)
        || self.is_bound(name)  // <-- BUG HERE!
    {
        Value::Symbol(name.clone())  // Return as-is
    } else {
        // Rename with gensym
        let mut renamings = self.renamings.borrow_mut();
        let renamed = renamings
            .entry(name.clone())
            .or_insert_with(|| gensym(name));
        Value::Symbol(renamed.clone())
    }
}
```

**The bug:** `self.is_bound(name)` checks bindings in the **expansion-time environment** (passed to `Expander::new(env)`), not the **definition-time environment**.

### What's Missing

The `Identifier` struct (in `template.rs:22-35`):
```rust
#[derive(Clone, Debug)]
pub struct Identifier {
    name: Rc<str>,
    // TODO: Add scope information for full hygiene support  <-- THIS!
}
```

We need to add scope/environment information here!

---

## Recommended Solution for Patina

After studying both implementations, here's my recommendation:

### Phase 1: Capture Definition Environment (Immediate Fix)

**Add environment to Identifier:**
```rust
#[derive(Clone, Debug)]
pub struct Identifier {
    name: Rc<str>,
    /// Environment where this identifier was introduced (for hygiene)
    /// None for pattern variables (they get substituted, not renamed)
    env: Option<Rc<Environment>>,
}
```

**Changes needed:**

1. **In `compiler.rs:342-358`** - Capture environment when creating `Template::Symbol`:
```rust
Value::Symbol(s) => {
    if let Some(pvref) = self.pvars.get(s) {
        // Pattern variable
        Ok(Template::Var(*pvref))
    } else {
        // Free variable - wrap with defining environment
        Ok(Template::Symbol(Identifier::with_env(s.clone(), Some(self.env.clone()))))
    }
}
```

2. **Pass environment to compiler** - Need to thread environment through:
   - `Compiler::new(literals, ellipsis, env)` - add env parameter
   - Store `env: Rc<Environment>` in `Compiler` struct
   - Pass it when creating identifiers

3. **In `expander.rs:568-594`** - Check bound in **captured environment**:
```rust
fn rename_identifier(&self, id: &Identifier) -> Value {
    let name = id.name();

    // Check binding in the identifier's defining environment (if captured)
    let is_bound = if let Some(def_env) = id.env() {
        def_env.get(name).is_some()  // Look up in DEFINING env
    } else {
        self.env.get(name).is_some() // Fallback to current env
    };

    if is_gensym(name.as_ref())
        || is_special_form(name.as_ref())
        || self.is_macro(name)
        || is_bound  // Now checks CORRECT environment!
    {
        Value::Symbol(name.clone())
    } else {
        // Rename with gensym
        ...
    }
}
```

### Phase 2: Full Identifier System (Future Enhancement)

For complete hygiene (handling `=>` literals in `cond`, underscore wildcards, etc.):

1. **Create full `Identifier` type** with:
   - Name
   - Defining environment
   - Optional "mark" or "color" for disambiguation
   - Literal flag (for `syntax-rules` literals)

2. **Update `Value` enum** to distinguish symbols from identifiers:
```rust
pub enum Value {
    Symbol(Rc<str>),           // Bare symbols
    Identifier(Rc<Identifier>), // Wrapped identifiers (NEW!)
    // ...
}
```

3. **Modify lookup** to handle identifiers specially

This is a bigger change and can be done after Phase 1 is working.

---

## Implementation Progress

### Phase 1 Attempt: Environment Capture (2025-11-22)

**Status**: ✅ Partially Implemented, ❌ Discovered Fundamental Limitation

#### What We Implemented

1. ✅ **Added Environment to Identifier** (`template.rs`)
   - Added `env: Option<Rc<Environment>>` field
   - Created `with_env()` constructor
   - Added `env()` accessor

2. ✅ **Threaded Environment Through Compiler** (`compiler.rs`)
   - Added `env` field to `Compiler` struct
   - Created `Compiler::with_env()` constructor
   - Updated `compile_template()` to capture environment in `Identifier`

3. ✅ **Updated Expander** (`expander.rs`)
   - Modified `rename_identifier()` to check captured environment
   - Identifiers bound in defining environment don't get renamed

4. ✅ **Updated Call Sites**
   - `let_syntax.rs`: Pass appropriate environment (parent for `let-syntax`, new_env for `letrec-syntax`)
   - `define_syntax.rs`: Pass current environment
   - `interface.rs`: Pass test environment

#### The Problem We Discovered

**Test case still fails:**
```scheme
(let ((x 'outer))
  (let-syntax ((m (syntax-rules () ((m) x))))
    (let ((x 'inner))
      (m))))
;; Expected: outer
;; Got: inner  ❌
```

**Debug trace shows:**
```
[MACRO] Expanding macro 'm': (m)
[MACRO]   Expanded to: x
[EVAL] Evaluating: x (tail=true)
[ENV]   Lookup: 'x'
[EVAL] => inner
```

**Root cause**: Even though we capture the environment during compilation and check if `x` is bound there, we return a bare `Value::Symbol("x")`. When this symbol is evaluated, the evaluator looks it up in the **current evaluation environment** (where `x='inner`), not the captured definition environment (where `x='outer`).

**Why this happens**:
- Macro expansion returns plain `Value` objects (symbols, lists, etc.)
- These values don't carry environment information
- During evaluation, all symbols are looked up in the current environment
- We need a way to preserve the binding from the definition environment

---

## Revised Solution Options

After implementing Phase 1, we've discovered that capturing the environment at compile time isn't enough. We need a way to make free variables resolve to their definition-time bindings at **evaluation time**, not expansion time.

Here are three potential solutions:

### Option A: Value-Level Identifiers (Most Correct, Most Invasive)

Extend the `Value` enum to support identifiers that carry environment information:

```rust
pub enum Value {
    Symbol(Rc<str>),                    // Bare symbols (user code)
    Identifier(Rc<IdentifierValue>),    // Hygienic identifiers (macro output)
    // ... other variants
}

pub struct IdentifierValue {
    name: Rc<str>,
    env: Rc<Environment>,  // Environment for lookup
}
```

**During expansion**: Return `Value::Identifier` for free variables in templates

**During evaluation**:
- When evaluating `Value::Identifier(id)`, look up `id.name` in `id.env`
- When evaluating `Value::Symbol(s)`, look up `s` in current environment

**Advantages:**
- ✅ Exactly matches Chibi's syntactic closures
- ✅ Preserves lexical scoping perfectly
- ✅ Handles all edge cases correctly

**Disadvantages:**
- ❌ Requires changing `Value` enum (affects entire codebase)
- ❌ Need to update pattern matching everywhere
- ❌ Evaluation logic needs to distinguish Symbol vs Identifier
- ❌ Large implementation effort

**Estimated effort**: 2-3 days, ~500 lines changed across multiple files

---

### Option B: Let-Wrapping (Simple, Partially Correct)

During macro compilation, wrap the template in a `let` form that binds free variables:

**Example transformation:**
```scheme
;; Original macro
(let ((x 'outer))
  (let-syntax ((m (syntax-rules () ((m) x))))
    ...))

;; Compiled macro template internally becomes:
;;   (let ((##x#1 x)) ##x#1)
;; where ##x#1 is a gensym capturing the value at definition time
```

**Implementation**:
1. During `compile_syntax_rules`, identify free variables in templates
2. Create gensym bindings for each free variable
3. Wrap the entire template in a `let` form: `(let ((##var#N var) ...) template')`
4. Replace free variables in template with their gensyms

**Advantages:**
- ✅ No changes to `Value` enum
- ✅ Relatively simple to implement
- ✅ Works for basic hygiene cases

**Disadvantages:**
- ❌ Creates runtime overhead (extra `let` bindings)
- ❌ May not handle all edge cases (nested macros, macro-generating macros)
- ❌ Requires analyzing templates for free variables

**Estimated effort**: 1 day, ~200 lines in compiler

---

### Option C: Closure Wrapping (Moderate, Flexible)

Similar to Option B, but wrap in a lambda that closes over the environment:

**Example transformation:**
```scheme
;; Original macro
(let ((x 'outer))
  (let-syntax ((m (syntax-rules () ((m) x))))
    ...))

;; Compiled macro internally:
;;   ((lambda () x))  ; immediately invoked
;; The lambda captures x from the defining environment
```

**Implementation**:
1. During expansion, wrap free variable references in `(lambda () var)`
2. Immediately apply the lambda: `((lambda () var))`
3. The lambda closure captures the binding at definition time

**Advantages:**
- ✅ No changes to `Value` enum
- ✅ Leverages existing closure mechanism
- ✅ Conceptually clean

**Disadvantages:**
- ❌ Runtime overhead (lambda creation and application)
- ❌ More complex than let-wrapping
- ❌ May confuse stack traces

**Estimated effort**: 1-2 days, ~250 lines in compiler/expander

---

### Option D: Pre-Evaluate Free Variables (Broken for Variables)

Evaluate free variables at macro definition time and substitute their values:

**Example:**
```scheme
(let ((x 'outer))
  (let-syntax ((m (syntax-rules () ((m) x))))
    ;; At this point, look up 'x, get 'outer, store Value::Symbol("outer")
    ;; Template becomes: (m) -> 'outer
    ...))
```

**Why this is BROKEN:**
- ❌ Only works for immutable values
- ❌ Breaks if variable is mutated (`set!`)
- ❌ Can't handle variables that don't exist yet (forward references)
- ❌ Fundamentally wrong semantics

**DO NOT USE THIS APPROACH**

---

## Recommended Path Forward

Based on the analysis, here's the recommended implementation order:

### Short-term (Complete Phase 1): Option B - Let-Wrapping

**Pros:** Fast to implement, solves the immediate test case, doesn't break existing code

**Implementation plan:**
1. During `compile_template()`, collect free variables (symbols not in `pvars`)
2. After compiling template, wrap it in a `let` that binds free variables:
   ```rust
   let template' = Template::List(vec![
       Template::Symbol(Identifier::new("let")),
       Template::List(bindings),  // ((##x#1 x) (##y#2 y) ...)
       original_template_with_gensyms,
   ])
   ```
3. Test with hygiene cases

**Timeline:** 1 day

### Medium-term (Complete Hygiene): Option A - Value-Level Identifiers

**Pros:** Correct implementation matching reference Schemes

**Implementation plan:**
1. Add `Value::Identifier` variant
2. Update evaluator to handle identifiers
3. Update all pattern matching on `Value`
4. Comprehensive testing

**Timeline:** 2-3 days

### Alternative: Option C - Closure Wrapping

If let-wrapping proves insufficient, try closure wrapping before committing to Value-level identifiers.

---

## Implementation Plan (Revised)

### Phase 1A: Let-Wrapping (Immediate Fix)

- [ ] Implement free variable analysis in `compiler.rs`
- [ ] Generate let-wrapping in `compile_template()`
- [ ] Test with failing hygiene cases
- [ ] Run chibi test suite

### Phase 1B: Value-Level Identifiers (Complete Solution)

- [ ] Add `Value::Identifier` variant to `runtime`
- [ ] Update `eval_step_impl()` to handle identifiers
- [ ] Update all match statements on `Value`
- [ ] Comprehensive testing
- [ ] Performance benchmarking

---

## Open Questions

1. **What environment should be passed to Compiler?**
   - For `define-syntax`: global environment at definition point
   - For `let-syntax`: local environment including outer bindings
   - For `letrec-syntax`: local environment with macro bindings

2. **How to handle nested macro expansions?**
   - When a macro expands to another macro call
   - Need to preserve environment chain correctly

3. **Performance impact?**
   - Every `Identifier` now carries an `Rc<Environment>`
   - Cloning is cheap (Rc), but memory usage increases
   - Acceptable trade-off for correctness?

4. **Should we support identifier=? predicate?**
   - R7RS has `identifier?` and `free-identifier=?` for macro systems
   - Not required for basic hygiene, but useful for advanced macros

---

## References

### Source Code
- **Chibi Scheme**: `~/Project/reference/chibi-scheme/eval.c`
  - Lines 88-227: Environment renames and syntactic closures
  - Lines 404-420: `sexp_make_synclo_op` - creating syntactic closures

- **Gauche Scheme**: `~/Project/reference/Gauche/src/macro.c`
  - Lines 254-274: Hygiene strategy comments
  - Lines 442-443: Renaming strategy
  - Uses `gauche/priv/identifierP.h` for identifier implementation

### Papers & Specs
- R7RS-small specification (section 4.3 on macros)
- ["Macros That Work"](https://www.cs.indiana.edu/~dyb/pubs/LaSC-5-4-pp359-375.pdf) - Kohlbecker et al., 1986
- ["Syntactic Abstraction in Scheme"](https://www.cs.indiana.edu/~dyb/pubs/SAS.pdf) - Dybvig et al.

### Related Documentation
- `internal/ARCHIVE/macro_research/` - Previous macro implementation research
- `PRD/phase1/IMPLEMENTATION_STATUS.md` - Overall R7RS compliance status
- `docs/FEATURE_STATUS.md` - Detailed test-by-test status

---

## Summary and Decision Points

### What We Learned

1. **Environment capture alone is insufficient**: Simply storing which environment an identifier comes from doesn't solve the problem, because `Value::Symbol` gets looked up in the evaluation environment, not the captured environment.

2. **The evaluation barrier**: Macro expansion produces plain `Value` objects which lose their environment context. The evaluator has no way to know which environment to use for lookup.

3. **Three viable approaches exist**:
   - **Let-wrapping**: Quick fix, works for most cases
   - **Closure-wrapping**: Similar but uses lambdas
   - **Value-level identifiers**: Complete solution, matches reference implementations

### Current State (2025-11-22)

**Completed:**
- ✅ Infrastructure for environment capture (Identifier, Compiler, Expander)
- ✅ Call sites updated to pass environments
- ✅ Comprehensive research document

**Not working:**
- ❌ Basic hygiene test case still fails
- ❌ Free variables still resolve in evaluation environment

**Code is stable:** All changes compile successfully, no regressions in existing tests.

### Decision Points for User

**Question 1: Which approach to pursue?**

- **Option B (Let-wrapping)**: Fast, simple, good enough for most cases
  - **Pro**: Can be done in 1 day
  - **Con**: May not handle all edge cases
  - **Recommendation**: Start here, see if it's sufficient

- **Option A (Value-level identifiers)**: Complete, correct, matches Chibi
  - **Pro**: Solves all hygiene issues correctly
  - **Con**: 2-3 days, touches many files
  - **Recommendation**: If let-wrapping insufficient

**Question 2: Priority vs other work?**

Hygiene bugs affect 5 out of 19 macro tests (26%). Is this:
- High priority: Block other work until fixed?
- Medium priority: Fix soon, but other R7RS features first?
- Low priority: Document as known limitation?

### Next Steps

**If pursuing let-wrapping (Option B):**
1. Implement free variable collection in `compiler.rs`
2. Generate let-wrapper around templates with free vars
3. Test with hygiene cases
4. Evaluate if sufficient or need Option A

**If pursuing value-level identifiers (Option A):**
1. Design `Value::Identifier` variant carefully
2. Update runtime, evaluator, and all match sites
3. Comprehensive testing
4. Performance validation

**If deferring:**
1. Document limitation in `docs/FEATURE_STATUS.md`
2. Keep current implementation (infrastructure useful for future)
3. Continue with other R7RS features

---

## Next Actions

- [ ] User decides on approach (A, B, or defer)
- [ ] If B: Implement let-wrapping
- [ ] If A: Design Value::Identifier and update plan
- [ ] If defer: Document limitation and move on

---

**Author**: Claude Code
**Last Updated**: 2025-11-22
**Status**: ⏸️ Shelved - Deferred pending CORE_IR_MIGRATION

**Reason for deferral**: The `Value` enum is already quite heavy. Adding `Value::Identifier` would make it heavier. The IR migration may provide a better foundation for implementing hygiene correctly. The current partial implementation (environment capture infrastructure) is kept in place as it will be useful when we return to this issue.

**Next steps when resuming**:
1. Review if IR migration provides better primitives for hygiene
2. If yes: Implement hygiene in IR layer
3. If no: Implement Option B (let-wrapping) as interim solution
