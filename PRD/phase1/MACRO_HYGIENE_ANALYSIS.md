# Macro Hygiene System Analysis

**Date**: 2025-11-26
**Status**: Debug Infrastructure Complete, Core Bug Identified
**Related Issue**: `letrec-syntax` hygiene test failure

## Overview

This document captures findings from a comprehensive analysis of Patina's macro expansion and hygiene implementation. The analysis was conducted to debug a failing `letrec-syntax` test case and identify architectural gaps that make macro debugging difficult.

### Test Case Under Investigation

```scheme
(test 7 (letrec-syntax
  ((my-or (syntax-rules ()
            ((my-or) #f)
            ((my-or e) e)
            ((my-or e1 e2 ...)
             (let ((temp e1))
               (if temp
                   temp
                   (my-or e2 ...)))))))
  (let ((x #f)
        (y 7)
        (temp 8)
        (let odd?)
        (if even?))
    (my-or x
           (let temp)
           (if y)
           y))))
```

This test verifies that:
1. The `temp` introduced by `my-or` doesn't capture the user's `temp` binding
2. The `let` and `if` introduced by `my-or` don't capture the user's `let` and `if` bindings
3. Hygiene is maintained through recursive macro expansion

---

## Current Architecture

### Strengths

#### 1. Racket-Style Scope Sets
**Location**: `crates/patina-core/src/scope.rs`

The implementation follows Flatt's "Binding as Sets of Scopes" (POPL 2016):
- `ScopeId`: Unique identifier for each binding form
- `ScopeSet`: Set of scopes an identifier carries (SmallVec for efficiency)
- `flip_scope()`: Toggle a scope in the set
- `is_subset_of()`: Key operation for hygiene lookup

```rust
// Example: Hygiene lookup rule
// A binding matches a reference if binding.scopes ⊆ reference.scopes
assert!(binding_scopes.is_subset_of(&reference_scopes));
```

#### 2. PVREF-Based Pattern/Template Compilation
**Location**: `crates/patina-macros/src/macro_expander/compiler.rs`

Inspired by Gauche Scheme:
- O(1) variable lookup via `PVRef(level, index)`
- Precomputed `num_following` for non-backtracking ellipsis matching
- Proper nested ellipsis support with tree-structured `MatchEnv`

#### 3. Dual Binding System in Environment
**Location**: `crates/patina-core/src/environment.rs`

```rust
pub struct Environment {
    /// Simple name-based bindings (for built-ins and top-level)
    bindings: Rc<RefCell<HashMap<String, Value>>>,
    /// Scope-aware bindings (for hygiene)
    scoped_bindings: Rc<RefCell<HashMap<String, Vec<ScopedBinding>>>>,
    parent: Option<Rc<Environment>>,
}
```

Lookup via `get_with_scopes()`:
1. Collect all bindings where `binding.scopes ⊆ reference.scopes`
2. Return the most specific (largest scope set) match
3. Fall back to simple bindings if no scoped match

#### 4. Flip-Scope Hygiene Algorithm
**Location**: `crates/patina-macros/src/macro_expander/mod.rs`

```rust
pub fn expand_macro(...) -> Result<Value, MacroError> {
    let macro_scope = ScopeId::fresh();

    // Step 1: Flip on INPUT (adds macro_scope to use-site identifiers)
    let flipped_args = flip_scope_on_value(args, macro_scope);

    // ... pattern matching and template expansion ...

    // Step 2: Flip on OUTPUT
    // - Use-site identifiers: macro_scope removed (was added, then flipped off)
    // - Introduced identifiers: macro_scope added (wasn't there, then flipped on)
    let result = flip_scope_on_value(&expanded, macro_scope);

    Ok(result)
}
```

#### 5. Existing Debug Infrastructure
- `MACRO_DEBUG` flag (`crates/patina-runtime/src/macro_debug.rs`)
- `MacroTracer` for selective tracing (`crates/patina-macros/src/tracer.rs`)

---

## Identified Gaps

### Gap 1: `is_special_form()` Defeats Hygiene (CRITICAL)

**Location**: `crates/patina-macros/src/macro_expander/expander.rs:569-616`

```rust
fn rename_identifier(&self, id: &Identifier) -> Value {
    let name = id.name();

    // Special forms and macros are never renamed
    if is_special_form(name.as_ref()) || self.is_macro(name) {
        return Value::Symbol(name.clone());  // <-- PROBLEM!
    }
    // ...
}
```

And at line 672-702:
```rust
fn is_special_form(name: &str) -> bool {
    matches!(
        name,
        "quote" | "if" | "define" | "set!" | "lambda" | "begin"
        | "let" | "let*" | "letrec" | "letrec*"
        | "cond" | "case" | "and" | "or" | "do"
        // ... etc
    )
}
```

**Problem**: When a macro template contains `let`, `if`, or other "special form" names, they are returned as plain `Symbol` values instead of `Identifier` values with scopes.

**Impact on Test Case**:
- `my-or` introduces `let` and `if` in its template
- These become `Symbol("let")` and `Symbol("if")`
- The user binds `(let odd?)` and `(if even?)` - also `Symbol` values
- They are **indistinguishable** - hygiene fails!

**Why This Exists**: The intent was to avoid renaming language keywords. But in a fully hygienic system, even `let` can be used as a variable name and should be hygienically distinguished.

**Recommended Fix**:
```rust
fn rename_identifier(&self, id: &Identifier, in_operator_position: bool) -> Value {
    let name = id.name();

    // Only treat as special form when in OPERATOR position
    if in_operator_position && is_special_form(name.as_ref()) {
        return Value::Symbol(name.clone());
    }

    // All other cases: apply normal hygiene
    // ...
}
```

---

### Gap 2: Missing Scope Information in Debug Output ~~(HIGH)~~ ✅ FIXED

**Status**: ✅ **IMPLEMENTED** (2025-11-26)

**Implementation**:
- Added `format_with_scopes()` in `crates/patina-core/src/debug_format.rs`
- Enhanced macro expansion debug output to show scopes at each step
- Shows input/output with scope sets, pattern variable bindings with scopes

**Current Output** (when `(macro-debug-mode 'on)`):
```
[MACRO] ========================================
[MACRO] Expanding macro: my-or
[MACRO]   Fresh macro scope: S77
[MACRO]   Definition scopes: {}
[MACRO]   Input (normal):      (my-or x y)
[MACRO]   Input (with scopes): (my-or x y)
[MACRO]   After input flip (scope S77 toggled):
[MACRO]     (my-or x y)
[MACRO]   === Pattern variable bindings ===
[MACRO]     e1 = x
[MACRO]   Before output flip:
[MACRO]     (let ((temp x)) (if temp temp (my-or y)))
[MACRO]   After output flip (scope S77 toggled):
[MACRO]     (with scopes) (let ((temp{S77} x)) (if temp{S77} temp{S77} (my-or y)))
[MACRO] ========================================
[MACRO] Expansion of 'my-or' complete!
[MACRO] Result: (let ((temp x)) (if temp temp (my-or y)))
```

---

### Gap 3: No Tracing in Environment Lookup ~~(MEDIUM)~~ ✅ FIXED

**Status**: ✅ **IMPLEMENTED** (2025-11-26)

**Implementation**:
- Added tracing to `define_with_scopes()` and `get_with_scopes()` in `crates/patina-core/src/environment.rs`
- Shows all candidates considered during subset matching
- Shows which binding wins (most specific scope set)
- Controlled by unified `(macro-debug-mode 'on)` flag

**Current Output**:
```
[ENV] Defining 'x' with scopes {S76} = #f
[ENV] Defining 'y' with scopes {S76} = 7
[ENV] Defining 'temp' with scopes {S79} = #f
[ENV] Looking up 'temp' with scopes {S77}
[ENV]   Checking binding {S79} ⊆ {S77} : NO
[ENV]   No scoped candidates, falling back to simple lookup
[ENV]   Fallback result: #f
```

---

### Gap 4: Free Variable Check Uses Simple Lookup (MEDIUM)

**Location**: `crates/patina-macros/src/macro_expander/compiler.rs:492-504`

```rust
// Fall back to marks-and-ribs hygiene (no scopes available)
let should_capture = self.env.as_ref().is_some_and(|env| env.get(s).is_some());
```

**Problem**: This uses `env.get(s)` (simple name lookup), not scoped lookup. When checking if `temp` is a "free variable" in the macro template, it might find the **wrong** `temp` - one from the use site rather than the definition site.

**Mitigation**: When `definition_scopes` is non-empty (the scope-based hygiene path), this code is skipped. But if `definition_scopes` is empty, the fallback logic is broken.

---

### Gap 5: Scope Origin Not Tracked ~~(LOW)~~ ✅ FIXED

**Status**: ✅ **IMPLEMENTED** (2025-11-26)

**Implementation**:
- Added `ScopeId::fresh_with_origin()` in `crates/patina-core/src/scope.rs`
- Added `ScopeId::origin()` to retrieve origin string
- Added `ScopeSet::format_with_origins()` for debugging display
- Thread-local storage for origin tracking

**API**:
```rust
// Create scope with origin tracking
let scope = ScopeId::fresh_with_origin("macro:my-or");

// Get origin later
if let Some(origin) = scope.origin() {
    println!("Scope {} came from {}", scope, origin);
}

// Display scopes with origins
println!("{}", scope_set.format_with_origins());
// Output: {S1(lambda), S2(let-syntax), S3(macro:my-or)}
```

---

## Data Flow Analysis

### How Hygiene Should Work for the Test Case

```
1. User code parsed:
   (let ((temp 8) (let odd?) (if even?)) ...)
   - temp, let, if are Symbol values (no scopes yet)

2. Entering letrec-syntax:
   - Fresh scope S1 created for letrec-syntax
   - Macro my-or compiled with definition_scopes = {S1}
   - Template identifiers (let, temp, if) tagged with {S1}

3. Macro expansion of (my-or x (let temp) (if y) y):
   - Fresh macro scope S2 created
   - Flip S2 on input: x{S2}, let{S2}, temp{S2}, if{S2}, y{S2}
   - Template expansion: let{S1}, temp{S1}, if{S1} (from definition)
   - Flip S2 on output:
     * Use-site: x{}, let{}, temp{}, if{}, y{} (S2 removed)
     * Introduced: let{S1,S2}, temp{S1,S2}, if{S1,S2} (S2 added)

4. Variable lookup for temp{S1,S2}:
   - User's temp has scopes {} (or from outer let)
   - Macro's temp has scopes {S1,S2}
   - {} ⊈ {S1,S2}, so user's temp does NOT match!
   - Macro's temp resolves to its own binding (the let-bound temp)

5. CURRENT BUG: let and if become Symbol, not Identifier
   - Template produces Symbol("let"), Symbol("if")
   - No scope discrimination possible
   - User's (let odd?) captured incorrectly!
```

---

## Recommended Improvements

### Priority 1: Fix `is_special_form()` Issue

**Effort**: Medium
**Impact**: Fixes the core hygiene bug
**Status**: 🔴 **NOT STARTED** - This is the remaining bug to fix

Modify `rename_identifier()` to only skip renaming for special forms when they're in operator position (first element of a list being evaluated as code).

### ~~Priority 2: Add Scope-Aware Debug Output~~ ✅ DONE

**Status**: ✅ **COMPLETED** (2025-11-26)

See Gap 2 above for implementation details.

### ~~Priority 3: Add Environment Lookup Tracing~~ ✅ DONE

**Status**: ✅ **COMPLETED** (2025-11-26)

See Gap 3 above for implementation details.

### ~~Priority 4: Scope Origin Tracking~~ ✅ DONE

**Status**: ✅ **COMPLETED** (2025-11-26)

See Gap 5 above for implementation details.

---

## Testing Recommendations

### Unit Tests for Hygiene

Add focused tests in `crates/patina-tests/tests/`:

```rust
#[test]
fn test_hygiene_special_form_names_as_variables() {
    // Verify that let, if, etc. can be used as variable names
    // without capturing macro-introduced identifiers
    let code = r#"
        (let ((let 1) (if 2))
          (let-syntax ((m (syntax-rules ()
                           ((m) (let ((x 3)) x)))))
            (+ let if (m))))
    "#;
    assert_eval_to(code, "6");  // 1 + 2 + 3
}

#[test]
fn test_hygiene_temp_variable_shadowing() {
    // The classic hygiene test
    let code = r#"
        (let ((temp 'outer))
          (let-syntax ((swap! (syntax-rules ()
                               ((swap! a b)
                                (let ((temp a))
                                  (set! a b)
                                  (set! b temp))))))
            (let ((x 1) (y 2))
              (swap! x y)
              (list x y temp))))
    "#;
    assert_eval_to(code, "(2 1 outer)");
}
```

### Debug Workflow ✅ NOW AVAILABLE

Enable unified macro/hygiene debugging in your Scheme code:

```scheme
(macro-debug-mode 'on)   ; Enable all macro + hygiene tracing

;; Your code here...

(macro-debug-mode 'off)  ; Disable when done
```

This will show:
- `[MACRO]` - Macro expansion steps with scope sets on identifiers
- `[ENV]` - Environment binding/lookup operations with subset matching
- `[SCOPE]` - Scope creation (when using `fresh_with_origin`)

Example debug session:
```scheme
(macro-debug-mode 'on)
(define-syntax my-or ...)
(let ((x #f) (y 7))
  (my-or x y))
```

For selective tracing in Rust tests:
```rust
MacroTracer::enable_for(&["my-or"]);
MacroTracer::set_max_depth(10);
// ... run test ...
MacroTracer::print_history();
```

---

## References

- Flatt, Matthew. "Binding as Sets of Scopes." POPL 2016.
- R7RS-small Section 4.3 (Macros)
- Gauche Scheme macro implementation: `src/macro.c`
- Chibi Scheme: `lib/init-7.scm`, `tests/r7rs-tests.scm`

---

## Appendix: Key Code Locations

| Component | Location |
|-----------|----------|
| ScopeSet implementation | `crates/patina-core/src/scope.rs` |
| Environment with scoped lookup | `crates/patina-core/src/environment.rs` |
| Macro expansion entry point | `crates/patina-macros/src/macro_expander/mod.rs` |
| Template compilation | `crates/patina-macros/src/macro_expander/compiler.rs` |
| Template expansion | `crates/patina-macros/src/macro_expander/expander.rs` |
| Desugarer (let-syntax handling) | `crates/patina-frontend/src/desugarer/mod.rs` |
| CoreExpr evaluation | `crates/patina-tree-walker/src/eval/core_eval.rs` |
| Debug flags (unified) | `crates/patina-core/src/macro_debug.rs` (re-exported by patina-runtime) |
| Debug formatting with scopes | `crates/patina-core/src/debug_format.rs` |
| Macro tracer | `crates/patina-macros/src/tracer.rs` |
| Debug primitives | `crates/patina-tree-walker/src/eval/primitives/debug.rs` |
