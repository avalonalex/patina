# Hygiene Unification: Migrating to WrappedIdentifier Only

**Status**: Research
**Created**: 2025-11-23
**Complexity**: Medium-High

## Executive Summary

Patina currently uses **two different hygiene mechanisms** (`Identifier` with captured environments and `WrappedIdentifier` with marks), which causes confusion and maintenance burden. This document outlines a migration path to use **only `WrappedIdentifier`** (marks-and-ribs hygiene), aligning with industry-standard Scheme implementations.

## Current State Analysis

### Two Hygiene Mechanisms

#### 1. `Value::Identifier` - Environment-based hygiene
```rust
Identifier {
    name: Rc<str>,
    env: Rc<dyn Any>,  // Captured lexical environment
}
```

**Usage**: 16 occurrences across codebase
**Created by**: Template expander for **free variables** (expander.rs:594-597)
**Lookup**: Uses captured environment (eval/mod.rs:490-493)

```rust
// In template expander (expander.rs:585-597)
if id.has_captured_env() {
    // This is a FREE VARIABLE from macro definition time
    return Value::Identifier {
        name: name.clone(),
        env: id.captured_env().unwrap().clone(),
    };
}
```

```rust
// In evaluator (eval/mod.rs:490-493)
if let Some(captured_env) = captured_env.downcast_ref::<Environment>() {
    // Look up in the CAPTURED environment (definition-time binding)
    if let Some(value) = captured_env.get(name) {
        return Ok(EvalResult::Value(value));
    }
}
```

#### 2. `Value::WrappedIdentifier` - Marks-based hygiene
```rust
WrappedIdentifier {
    name: Rc<str>,
    marks: Vec<usize>,  // Expansion history
}
```

**Usage**: 21 occurrences across codebase
**Created by**: Template expander for **introduced identifiers** (expander.rs:608-611)
**Lookup**: Uses current environment (eval/mod.rs:519-520)

```rust
// In template expander (expander.rs:600-611)
// No captured environment - this is an INTRODUCED IDENTIFIER
return Value::WrappedIdentifier {
    name: name.clone(),
    marks: vec![self.expansion_mark],
};
```

```rust
// In evaluator (eval/mod.rs:519-520)
// TODO: Full implementation would use marks to determine correct binding
if let Some(value) = env.get(name) {
    return Ok(EvalResult::Value(value));
}
```

### The Problem

**Key distinction**:
- **Free variables** (from macro definition): Use `Identifier` with captured environment
- **Introduced identifiers** (from macro template): Use `WrappedIdentifier` with marks

**Why this is problematic**:
1. **Confusion**: No clear rule for which variant to use
2. **Duplication**: Every pattern match needs to handle both
3. **Incomplete**: WrappedIdentifier lookup doesn't actually use marks (see TODO at eval/mod.rs:506)
4. **Non-standard**: Standard Scheme implementations use marks-and-ribs for everything

## Theoretical Foundation: Why Marks-and-Ribs Can Replace Captured Environments

### Key Insight

**Marks encode lexical context!**

In marks-and-ribs hygiene, identifiers don't need to explicitly capture environments because:

1. **Marks track provenance**: An identifier's mark list tells you where it came from
2. **Binding resolution uses marks**: Two identifiers match if they have the same `(name, marks)` pair
3. **Free variables naturally work**: Identifiers from macro definition time have marks from that context

### Example: Free Variable Capture

Consider this macro:
```scheme
(define x 10)

(define-syntax use-x
  (syntax-rules ()
    ((use-x) x)))  ; x is a free variable

; In different context:
(let ((x 20))
  (use-x))  ; Should return 10, not 20!
```

**How Identifier (environment-based) works**:
- `x` in template captures the environment where `x = 10`
- When `(use-x)` expands, `x` looks up in captured environment
- Returns `10` ✓

**How WrappedIdentifier (marks-based) should work**:
1. **Macro definition** (mark 0 = definition context):
   - Template contains `x` with marks `[]` (no expansion marks yet)
   - But `x` is bound at definition time, so it has *definition context marks*

2. **Macro expansion** (mark 1 = expansion of use-x):
   - Expand `(use-x)` → `x`
   - Pattern variables get new marks, but free variables keep definition marks
   - Result: `x` with marks from definition context

3. **Lookup**:
   - Search for binding with name `x` and matching marks
   - `x` from `(let ((x 20)) ...)` has different marks (local binding)
   - `x` from `(define x 10)` has matching marks (definition-time binding)
   - Returns `10` ✓

**The trick**: Free variables must preserve their marks from the macro definition environment!

## Migration Strategy

### Phase 1: Understand Current Free Variable Handling

**Current code path** (expander.rs:585-597):
```rust
if id.has_captured_env() {
    // FREE VARIABLE - bound at macro definition time
    return Value::Identifier {
        name: name.clone(),
        env: id.captured_env().unwrap().clone(),
    };
}
```

**Question**: How does the compiler know `id.has_captured_env()`?

**Answer** (from template.rs): The `Identifier` struct tracks whether it was found in the definition environment during compilation.

### Phase 2: Track Definition-Time Marks

**Key change**: Instead of capturing the environment, capture the **marks** that the identifier should have.

**New approach**:
1. When compiling a macro template, check if an identifier is free
2. If free, determine what marks it should have (definition context marks)
3. Store those marks in the `Identifier` struct (template.rs)
4. During expansion, create `WrappedIdentifier` with those marks

### Phase 3: Implement Mark-Aware Lookup

**Current limitation** (eval/mod.rs:506-520):
```rust
// TODO: Full marks-and-ribs will need to use marks for proper hygiene
Value::WrappedIdentifier { name, marks } => {
    // For now, just look up by name (WRONG!)
    if let Some(value) = env.get(name) {
        return Ok(EvalResult::Value(value));
    }
}
```

**Required change**: Implement proper marks-aware binding resolution:
1. Search environment for bindings with matching name
2. Check if marks match (accounting for macro introduction scopes)
3. Return the correctly-scoped binding

**Algorithm** (from Dybvig et al.):
```rust
fn resolve_identifier(name: &str, marks: &[Mark], env: &Environment) -> Option<Value> {
    // 1. Search for all bindings with this name
    let candidates = env.find_all(name);

    // 2. Find binding introduced with matching marks
    for (binding_marks, value) in candidates {
        if marks_match(marks, binding_marks) {
            return Some(value);
        }
    }

    None
}

fn marks_match(id_marks: &[Mark], binding_marks: &[Mark]) -> bool {
    // Simplified: exact match
    // Full algorithm accounts for mark anti-marks, substitution marks, etc.
    id_marks == binding_marks
}
```

## Step-by-Step Migration Plan

### Step 1: Add Mark Tracking to Template Identifiers

**File**: `crates/patina-macros/src/macro_expander/template.rs`

**Current**:
```rust
pub struct Identifier {
    name: Rc<str>,
    captured_env: Option<Rc<dyn std::any::Any>>,
}
```

**New**:
```rust
pub struct Identifier {
    name: Rc<str>,
    /// Marks this identifier should have (for free variables)
    /// None = introduced identifier (gets expansion mark)
    /// Some(marks) = free variable (keeps definition marks)
    definition_marks: Option<Vec<Mark>>,
}
```

**Changes needed**:
1. During macro compilation, when finding a free variable, store its marks
2. Remove `captured_env` field and all related code
3. Update `has_captured_env()` to `is_free_variable()`

**Complexity**: Low - straightforward struct change

### Step 2: Update Template Expander

**File**: `crates/patina-macros/src/macro_expander/expander.rs`

**Current** (lines 585-611):
```rust
if id.has_captured_env() {
    return Value::Identifier {
        name: name.clone(),
        env: id.captured_env().unwrap().clone(),
    };
}

return Value::WrappedIdentifier {
    name: name.clone(),
    marks: vec![self.expansion_mark],
};
```

**New**:
```rust
let marks = if let Some(def_marks) = id.definition_marks() {
    // FREE VARIABLE - preserve definition-time marks
    def_marks.clone()
} else {
    // INTRODUCED IDENTIFIER - add expansion mark
    vec![self.expansion_mark]
};

Value::WrappedIdentifier {
    name: name.clone(),
    marks,
}
```

**Changes needed**:
1. Always return `WrappedIdentifier`
2. Use definition marks for free variables
3. Use expansion mark for introduced identifiers

**Complexity**: Low - simple logic change

### Step 3: Implement Mark-Aware Binding Resolution

**File**: `crates/patina-tree-walker/src/eval/mod.rs`

**Current** (lines 507-527):
```rust
Value::WrappedIdentifier { name, marks } => {
    // TODO: Full implementation would use marks
    if let Some(value) = env.get(name) {
        return Ok(EvalResult::Value(value));
    }

    Err(EvalError::UndefinedVariable(...))
}
```

**New**:
```rust
Value::WrappedIdentifier { name, marks } => {
    // Mark-aware lookup
    if let Some(value) = env.get_with_marks(name, marks) {
        return Ok(EvalResult::Value(value));
    }

    // Fallback: try unmarked lookup for built-ins
    if marks.is_empty() {
        if let Some(value) = env.get(name) {
            return Ok(EvalResult::Value(value));
        }
    }

    Err(EvalError::UndefinedVariable(...))
}
```

**Changes needed**:
1. Implement `Environment::get_with_marks(name, marks)` method
2. Environment needs to track marks for each binding
3. Update `define` to store marks with bindings

**Complexity**: High - requires Environment changes

### Step 4: Update Environment to Track Marks

**File**: `crates/patina-runtime/src/environment.rs`

**Current**:
```rust
pub struct Environment {
    bindings: RefCell<HashMap<String, Value>>,
    parent: Option<Rc<Environment>>,
}
```

**New**:
```rust
pub struct Environment {
    // Map from (name, marks) to value
    bindings: RefCell<HashMap<(String, MarkList), Value>>,
    parent: Option<Rc<Environment>>,
}

pub type MarkList = Vec<usize>;
```

**OR** (simpler intermediate approach):
```rust
pub struct Environment {
    // Keep simple string lookup for unmarked bindings (built-ins, top-level)
    bindings: RefCell<HashMap<String, Value>>,
    // Add separate map for marked bindings (from macros)
    marked_bindings: RefCell<HashMap<(String, MarkList), Value>>,
    parent: Option<Rc<Environment>>,
}
```

**Changes needed**:
1. Add `get_with_marks(name, marks)` method
2. Update `define` to accept optional marks
3. Maintain backwards compatibility for unmarked bindings

**Complexity**: High - affects all environment usage

### Step 5: Remove Identifier Variant

**Files**: Multiple files across codebase

**Changes needed**:
1. Remove `Value::Identifier` variant from `value.rs`
2. Remove all pattern matches on `Identifier` (16 occurrences)
3. Update all code paths to use `WrappedIdentifier` only
4. Remove captured environment downcast code

**Complexity**: Medium - mechanical but thorough

### Step 6: Test Migration

**Test categories**:
1. **Basic hygiene**: Macro-introduced identifiers don't capture
2. **Free variables**: Macros can reference definition-time bindings
3. **Nested macros**: Marks stack correctly
4. **Pattern variables**: Preserve use-site marks
5. **Existing tests**: All 802 tests still pass

**Key test cases**:
```scheme
; Test 1: Free variable capture
(define x 10)
(define-syntax use-x (syntax-rules () ((use-x) x)))
(let ((x 20)) (use-x))  ; Must be 10

; Test 2: Hygiene (no capture)
(define-syntax swap!
  (syntax-rules ()
    ((swap! a b)
     (let ((temp a)) (set! a b) (set! b temp)))))
(define temp 99)
(swap! x y)  ; temp should not be visible

; Test 3: Nested macros
(define-syntax outer (syntax-rules () ((outer x) (inner x))))
(define-syntax inner (syntax-rules () ((inner y) (cons y y))))
(outer 42)  ; Should work
```

## Complexity Assessment

### Easy Parts (1-2 days)
- [x] Research and documentation (this document)
- [ ] Step 1: Add mark tracking to template.rs
- [ ] Step 2: Update template expander
- [ ] Step 5: Remove Identifier variant (mechanical)

### Hard Parts (3-5 days)
- [ ] Step 3: Implement mark-aware lookup algorithm
- [ ] Step 4: Update Environment to track marks
  - Design decision: Full marks-based or hybrid?
  - Need to handle built-ins (no marks)
  - Need to handle top-level defines
- [ ] Step 6: Comprehensive testing
  - Ensure no hygiene regressions
  - Test all edge cases

### Critical Design Decision: Environment Mark Tracking

**Option A: Full marks-based** (Chez Scheme approach)
```rust
bindings: HashMap<(String, MarkList), Value>
```
- **Pro**: Theoretically pure, matches Chez exactly
- **Con**: All bindings need marks (including built-ins)
- **Con**: More complex migration

**Option B: Hybrid** (pragmatic approach)
```rust
bindings: HashMap<String, Value>,           // Unmarked (built-ins, top-level)
marked_bindings: HashMap<(String, MarkList), Value>,  // From macros
```
- **Pro**: Backwards compatible
- **Pro**: Built-ins don't need marks
- **Pro**: Easier migration
- **Con**: Two lookup paths

**Recommendation**: Start with **Option B (hybrid)**, migrate to Option A later if needed.

## Migration Order

### Suggested Phases

**Phase 1: Foundation** (Week 1)
1. Add mark tracking to template.rs
2. Update template expander to always use WrappedIdentifier
3. Ensure no regressions (Identifier still exists as fallback)

**Phase 2: Environment** (Week 2)
1. Add marked_bindings to Environment
2. Implement get_with_marks() with hybrid approach
3. Update WrappedIdentifier lookup to use marks

**Phase 3: Cleanup** (Week 3)
1. Remove Identifier variant
2. Remove all Identifier pattern matches
3. Comprehensive testing
4. Update documentation

## Testing Strategy

### Unit Tests
- Mark generation (unique, monotonic)
- Mark matching (same marks = same binding)
- Template identifier creation (free vs introduced)

### Integration Tests
- Free variable capture (macro references definition-time binding)
- Hygiene (macro-introduced identifiers don't capture)
- Nested macros (marks stack correctly)
- Pattern variables (preserve marks from use site)

### Regression Tests
- All existing 802 tests must pass
- All chibi tests must pass
- No hygiene violations

## Risks and Mitigations

### Risk 1: Subtle hygiene bugs
**Mitigation**: Extensive testing, compare with reference implementations

### Risk 2: Performance impact
**Mitigation**: Benchmark before/after, optimize Environment lookup

### Risk 3: Built-ins broken by mark tracking
**Mitigation**: Hybrid approach (unmarked built-ins)

### Risk 4: Existing macros break
**Mitigation**: Gradual rollout, keep Identifier during transition

## Success Criteria

- [ ] All code uses `WrappedIdentifier` only
- [ ] `Value::Identifier` variant removed
- [ ] All 802 cargo tests passing
- [ ] All chibi tests passing (>88)
- [ ] Hygiene tests pass (free variables, no capture, nested macros)
- [ ] No pattern matches on `Identifier` remain
- [ ] Documentation updated

## Future Work

After migration complete:

1. **Full marks-and-ribs**: Migrate from hybrid to full marks-based environment
2. **Syntax objects**: Add source location tracking
3. **Phase separation**: Separate macro-time and runtime environments
4. **Module system**: Use marks for module scoping

## References

- **Dybvig et al. (1993)**: "Syntactic abstraction in Scheme" - Original marks-and-ribs paper
- **Chez Scheme**: `syntax.ss` implementation
- **Patina marks_and_ribs.rs**: Current implementation with detailed algorithm documentation
- **Flatt (2016)**: "Binding as sets of scopes" - Evolution to scope sets (Racket)

## Questions for Further Research

1. **How does Chez handle built-in primitives?**
   - Do they have marks?
   - Special case in lookup?

2. **How are top-level defines handled?**
   - What marks do they get?
   - Empty mark list?

3. **What about `let-syntax` scoping?**
   - Local macros have local marks?
   - How do marks interact with lexical scopes?

4. **Performance implications?**
   - How expensive is mark-aware lookup?
   - Can we optimize with caching?

## Appendix: Code Locations

### Files to Modify

1. **Template.rs** (patina-macros/src/macro_expander/template.rs)
   - Line 30-42: `Identifier` struct definition
   - Add `definition_marks` field

2. **Expander.rs** (patina-macros/src/macro_expander/expander.rs)
   - Lines 585-611: Free variable vs introduced identifier logic
   - Always return `WrappedIdentifier`

3. **Eval/mod.rs** (patina-tree-walker/src/eval/mod.rs)
   - Lines 477-502: `Identifier` lookup (remove)
   - Lines 507-527: `WrappedIdentifier` lookup (enhance with marks)

4. **Environment.rs** (patina-runtime/src/environment.rs)
   - Add `marked_bindings` field
   - Add `get_with_marks()` method
   - Update `define()` to accept marks

5. **Value.rs** (patina-runtime/src/value.rs)
   - Lines 38-43: `Identifier` variant (remove after migration)
   - Keep `WrappedIdentifier` only

### Pattern Match Locations (16 total)

```bash
# Find all Identifier pattern matches:
grep -rn "Value::Identifier {" crates/
```

All 16 need to be updated or removed.
