# Marks-and-Ribs Hygiene Implementation

**Date:** 2025-11-23
**Status:** Foundation Complete ✅
**Tests:** All 443 tests passing

## Summary

Implemented the foundation for marks-and-ribs hygiene system based on Chez Scheme's approach. This provides a pluggable hygiene algorithm that can replace the current gensym-based system when fully activated.

## What Was Implemented

### 1. Core Data Structures (`crates/patina-macros/src/marks_and_ribs.rs`)

**File:** 600+ lines with comprehensive documentation

**Key Types:**

- **`Mark`** - A unique integer identifying an expansion phase
  ```rust
  pub type Mark = usize;
  ```

- **`MarkList`** - List of marks tracking expansion history
  ```rust
  pub type MarkList = Vec<Mark>;
  ```

- **`WrappedIdentifier`** - Identifier with hygiene metadata
  ```rust
  pub struct WrappedIdentifier {
      pub name: Rc<str>,
      pub marks: MarkList,
  }
  ```

- **`Rib`** - Local substitution environment for pattern variables
  ```rust
  pub struct Rib {
      bindings: HashMap<Rc<str>, MarkList>,
  }
  ```

- **`MarksAndRibsHygiene`** - Implementation of `HygieneSystem` trait
  ```rust
  pub struct MarksAndRibsHygiene;
  ```

**References Implemented:**
1. Mark allocation with global counter (like Chez's `new-mark`)
2. Wrapped identifiers with mark lists (like Chez's syntax objects)
3. Ribs for pattern variable tracking (like Chez's ribs)
4. Identifier equivalence checking (`binds_same_as` = Chez's `bound-identifier=?`)

### 2. Runtime Support (`crates/patina-runtime/src/value.rs`)

**Added New Variant:**
```rust
Value::WrappedIdentifier {
    name: Rc<str>,
    /// List of expansion marks (most recent first)
    marks: Vec<usize>,
}
```

**Display Implementation:**
- Debug mode: Shows marks as `name@mark1@mark2`
- Production mode: Shows just the name

**Type Name:** Returns `"identifier"` for consistency

### 3. Integration (`crates/patina-frontend`, `crates/patina-tree-walker`)

**Desugarer Support:**
```rust
// Treat WrappedIdentifier as a variable reference
Value::WrappedIdentifier { name, .. } => Ok(CoreExpr::Var(name.clone()))
```

**Evaluator Support:**
```rust
// No expansion needed for WrappedIdentifiers
Value::WrappedIdentifier { .. } => Ok(expr.clone())
```

### 4. HygieneSystem Trait Implementation

**Current Implementation:**
- Delegates to gensym hygiene for compatibility
- Fully implements the trait interface
- Returns `"marks-and-ribs"` as name
- Ready to be activated when full integration is complete

**Future Activation:**
When ready to use actual marks:
1. Uncomment the mark allocation code
2. Enable `add_mark_to_expr` function
3. Update macro expander to use marks instead of gensym

## Files Created/Modified

### Created
1. **`crates/patina-macros/src/marks_and_ribs.rs`** (600+ lines)
   - Complete marks-and-ribs data structures
   - 8 comprehensive unit tests
   - Extensive documentation with references

### Modified
1. **`crates/patina-runtime/src/value.rs`**
   - Added `Value::WrappedIdentifier` variant
   - Updated `type_name()` method
   - Updated `Display` implementation

2. **`crates/patina-frontend/src/desugarer/mod.rs`**
   - Handle `WrappedIdentifier` in desugaring

3. **`crates/patina-tree-walker/src/eval/mod.rs`**
   - Handle `WrappedIdentifier` in macro expansion

4. **`crates/patina-macros/src/lib.rs`**
   - Export marks_and_ribs module
   - Export key types (Mark, MarkList, WrappedIdentifier, Rib, MarksAndRibsHygiene)

## Academic References

The implementation is based on:

### 1. Primary References

**Chez Scheme Implementation:**
- File: `s/syntax.ss` in Chez Scheme source
- Functions: `new-mark`, `add-mark`, `make-syntax`, `bound-identifier=?`
- Chez Scheme Version 9 User's Guide, Chapter 11

**Academic Papers:**
1. **Dybvig, Friedman, and Haynes (1992)**
   - "Expansion-passing style: A general macro mechanism"
   - Lisp and Symbolic Computation
   - Original marks paper

2. **Dybvig, Hieb, and Bruggeman (1993)**
   - "Syntactic abstraction in Scheme"
   - Lisp and Symbolic Computation
   - Full marks-and-ribs algorithm
   - https://www.cs.indiana.edu/~dyb/pubs/LaSC-5-4-pp295-326.pdf

3. **Flatt (2016)**
   - "Binding as sets of scopes"
   - POPL 2016
   - Evolution to Racket's scope sets (mentioned for completeness)

## How Marks-and-Ribs Works

### Algorithm Overview

**Core Insight:** Track expansion phases with marks instead of renaming identifiers.

### Expansion Process

```text
1. Allocate fresh mark M for this expansion
2. Add mark M to all identifiers in the INPUT form
3. Match pattern against marked input
4. Expand template, substituting pattern variables
5. Add mark M to all identifiers in OUTPUT (except pattern variables)
```

### Name Resolution

```text
1. Find identifier with (name, marks)
2. Search environment for matching binding
3. Two identifiers match if they have same name AND same marks
```

### Example

**Macro:**
```scheme
(define-syntax swap!
  (syntax-rules ()
    ((swap! a b)
     (let ((temp a))
       (set! a b)
       (set! b temp)))))
```

**Expansion of `(swap! x y)`:**

1. **Allocate mark 1**

2. **Mark input:**
   - Input: `(swap! x y)`
   - Marked: `(swap!@1 x@1 y@1)`

3. **Match pattern:** `(swap! a b)` against `(swap!@1 x@1 y@1)`
   - `a` binds to `x@1`
   - `b` binds to `y@1`

4. **Expand template:** `(let ((temp a)) (set! a b) (set! b temp))`
   - Substitute: `a` → `x@1`, `b` → `y@1`
   - Result: `(let ((temp x@1)) (set! x@1 y@1) (set! y@1 temp))`

5. **Mark output:**
   - Template identifiers get mark 1: `let@1`, `temp@1`, `set!@1`
   - Pattern variables keep marks: `x@1`, `y@1`
   - **Final:** `(let@1 ((temp@1 x@1)) (set!@1 x@1 y@1) (set!@1 y@1 temp@1))`

6. **Hygiene achieved:**
   - User's `x` and `y` have marks `[1]`
   - Macro's `temp` has marks `[1]`
   - If user also has `temp`, it would have marks `[]`
   - These are **different identifiers** - no capture!

### Nested Macros

Marks stack for nested expansions:

```scheme
(define-syntax outer
  (syntax-rules ()
    ((outer x) (inner x))))

(define-syntax inner
  (syntax-rules ()
    ((inner y) (let ((temp y)) temp))))

(outer foo)
```

**Expansion trace:**
1. Expand `(outer foo)` with mark 1 → `(inner@1 foo@1)`
2. Expand `(inner@1 foo@1)` with mark 2 → `(let@2 ((temp@2 foo@1)) temp@2)`

**Result:**
- `foo@1` has marks `[1]`
- `temp@2` has marks `[2, 1]` (mark 2 added during inner expansion)
- Different mark lists = different identifiers

## Current Status: Foundation Only

### What's Working ✅

1. **Data structures** - All core types implemented
2. **Trait integration** - Implements `HygieneSystem` trait
3. **Runtime support** - `Value::WrappedIdentifier` variant exists
4. **Pipeline integration** - Desugarer and evaluator handle wrapped identifiers
5. **Tests** - All 443 workspace tests pass
6. **Documentation** - Comprehensive comments with academic references

### What's Not Yet Active ⏸️

1. **Actual marking** - Currently delegates to gensym for compatibility
2. **Mark-aware lookup** - Evaluator doesn't check marks yet
3. **Rib tracking** - Pattern variable substitution doesn't use ribs yet
4. **Macro expander integration** - Expansion doesn't allocate/add marks yet

### Why This Approach?

**Incremental Migration Strategy:**

1. ✅ **Phase 1: Foundation** (DONE)
   - Add data structures
   - Implement trait interface
   - Integrate with pipeline
   - Keep existing behavior

2. ⏭️ **Phase 2: Activation** (FUTURE)
   - Enable mark allocation in `MarksAndRibsHygiene::apply_hygiene`
   - Update macro expander to use marks
   - Add mark-aware name resolution in evaluator
   - Test with feature flag

3. ⏭️ **Phase 3: Migration** (FUTURE)
   - Run R7RS tests with marks-and-ribs
   - Fix any hygiene issues discovered
   - Make marks-and-ribs the default
   - Deprecate gensym hygiene

## Testing

### Unit Tests (8 tests in marks_and_ribs.rs)

```rust
test_wrapped_identifier_creation  // Basic creation
test_wrapped_identifier_with_marks  // With marks list
test_add_mark  // Mark addition
test_binds_same_as  // Identifier equivalence
test_rib_operations  // Rib bindings
test_fresh_mark_uniqueness  // Mark generation
test_hygiene_system_interface  // Trait implementation
test_clone_box  // Cloning support
```

**All tests passing ✅**

### Integration Tests

All 443 workspace tests pass with the new infrastructure:
- 88 tests in patina-macros (includes 8 new marks-and-ribs tests)
- 144 tests in patina-frontend
- 13 tests in patina-core-ir
- 38 tests in patina-interpreter
- And many more...

**No regressions ✅**

## Future Activation Plan

When ready to enable marks-and-ribs (estimated 2-3 weeks):

### Step 1: Enable Mark Allocation

In `marks_and_ribs.rs`, uncomment the actual implementation:

```rust
impl HygieneSystem for MarksAndRibsHygiene {
    fn apply_hygiene(&self, expr: &Value, pattern_vars: &HashSet<Rc<str>>,
                     expansion_env: &Rc<Environment>) -> Value {
        // ENABLE THIS:
        let mark = Self::fresh_mark();
        Self::add_mark_to_expr(expr, mark, pattern_vars)
    }
}
```

### Step 2: Update Macro Expander

Modify `expand_macro` to add marks to input:

```rust
pub fn expand_macro(compiled_macro: &CompiledMacro, args: &Value,
                    expansion_env: &Rc<Environment>) -> Result<Value, MacroError> {
    let mark = MarksAndRibsHygiene::fresh_mark();

    // Mark the input
    let marked_args = add_mark_to_value(args, mark);

    // ... pattern matching ...

    // Mark the output (except pattern vars)
    let marked_output = add_mark_to_value(&expanded, mark);

    Ok(marked_output)
}
```

### Step 3: Mark-Aware Name Resolution

Update evaluator to check marks during lookup:

```rust
// In evaluator, when resolving WrappedIdentifier:
Value::WrappedIdentifier { name, marks } => {
    // Look for binding with same name AND same marks
    env.get_with_marks(name, marks)
}
```

### Step 4: Feature Flag

```toml
[features]
default = ["gensym-hygiene"]
marks-and-ribs = []
```

```rust
#[cfg(feature = "marks-and-ribs")]
pub fn default_hygiene() -> Box<dyn HygieneSystem> {
    Box::new(MarksAndRibsHygiene::new())
}
```

### Step 5: Testing

```bash
# Test with marks-and-ribs
cargo test --features marks-and-ribs

# Run R7RS compliance
./scripts/run_chibi_tests.sh

# Compare with gensym
diff <(cargo test 2>&1) <(cargo test --features marks-and-ribs 2>&1)
```

### Step 6: Make Default

Once all tests pass:
```toml
[features]
default = ["marks-and-ribs"]
```

## Benefits of This Implementation

### Immediate Benefits

1. **Clean abstraction** - Hygiene system is pluggable via trait
2. **Documented algorithm** - 600+ lines with academic references
3. **No regressions** - All existing tests pass
4. **Future-ready** - Infrastructure for better hygiene

### Future Benefits (When Activated)

1. **Better R7RS compliance** - Handles edge cases gensym misses
2. **Nested macro support** - Marks stack correctly
3. **Module system foundation** - Phase tracking for imports/exports
4. **Research opportunities** - Easy to experiment with variations

## Comparison with Gensym

| Feature | Gensym (Current) | Marks-and-Ribs (This Implementation) |
|---------|------------------|--------------------------------------|
| **Algorithm** | Post-expansion renaming | Phase tracking with marks |
| **Hygiene** | Rename free identifiers | Discriminate by marks |
| **Nested macros** | Can fail in edge cases | Marks stack correctly |
| **Phase tracking** | No | Yes (via marks) |
| **Implementation** | ~200 lines | ~600 lines (with docs) |
| **Complexity** | Simple | Moderate |
| **R7RS compliance** | Good enough | Better (when activated) |
| **Module support** | Limited | Better (phase-aware) |

## Documentation Quality

### Code Comments

- Every struct/function has detailed rustdoc
- Academic references cited inline
- Examples showing how marks work
- Comparisons to Chez Scheme implementation

### References Included

1. **Academic papers** - Full citations with URLs
2. **Chez Scheme** - Specific file and function references
3. **Algorithm description** - Step-by-step explanation
4. **Worked examples** - `swap!` macro expansion trace

## Conclusion

This implementation provides a **solid foundation** for marks-and-ribs hygiene:

✅ **Complete data structures** with all necessary types
✅ **Trait integration** works with existing infrastructure
✅ **Comprehensive documentation** with academic references
✅ **All tests passing** - no regressions
✅ **Clear activation path** - documented steps for future work

**Current status:** Foundation complete, ready for activation when needed.

**Next step:** When R7RS compliance testing reveals hygiene issues, follow the activation plan above to enable actual mark tracking.

**Timeline:** 2-3 weeks to fully activate and test marks-and-ribs hygiene.

## References

1. **Implementation:**
   - `crates/patina-macros/src/marks_and_ribs.rs`
   - `crates/patina-runtime/src/value.rs`

2. **Documentation:**
   - `docs/HYGIENE_SYSTEM_DESIGN.md` - How to swap hygiene implementations
   - `internal/MACRO_HYGIENE_APPROACHES.md` - Research on hygiene algorithms
   - `docs/HYGIENE_COMPLIANCE_ANALYSIS.md` - R7RS compliance analysis

3. **Related Work:**
   - `~/Project/reference/chez-scheme` - Chez implementation
   - Dybvig et al. papers (referenced in code comments)
