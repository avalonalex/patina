# Evaluator Module Refactoring - Phase 2: Primitives Split

**Status:** Ready for Implementation
**Created:** 2025-11-07
**Phase:** 2 of 3
**Parent Document:** `EVAL_MODULE_REFACTORING.md`

---

## Executive Summary

**Phase 1 is COMPLETE!** ✅  The evaluator was successfully split from a monolithic 2,711-line file into:
- `error.rs` (29 lines)
- `mod.rs` (125 lines)
- `application.rs` (113 lines)
- `special_forms.rs` (1,006 lines)
- `primitives.rs` (2,697 lines) ⚠️

However, **primitives.rs has grown to 2,697 lines** (double the original projection of 1,300 lines) due to:
- Added 22 string primitives (including UTF-8 support)
- Added 18 vector primitives (full R7RS section 6.8)
- Total primitives: **61** (up from 51)

**Phase 2 Goal:** Split `primitives.rs` into logical, maintainable modules of ~300-500 lines each.

---

## Current State Analysis

### primitives.rs Breakdown (2,697 lines)

| Category | Primitives | Est. Lines | Complexity |
|----------|-----------|------------|------------|
| **Arithmetic** | 9 | ~600 | Medium |
| **Numeric Comparisons** | 5 | ~100 | Low |
| **Numeric Operations** | 6 | ~200 | Medium |
| **Pairs/Lists** | 3 | ~60 | Low |
| **List Operations** | 6 | ~300 | Medium |
| **List Search** | 6 | ~200 | Medium |
| **Higher-Order** | 2 | ~150 | High |
| **Type Predicates** | 13 | ~200 | Low |
| **Equality** | 3 | ~150 | High |
| **Multiple Values** | 2 | ~50 | Low |
| **Strings** | 22 | ~600 | Medium |
| **Vectors** | 18 | ~500 | Medium |
| **Helpers/Dispatcher** | - | ~200 | Medium |
| **Installation** | - | ~100 | Low |

### Existing Logical Sections (from comments)

The code already has clear section markers:
```rust
// ========== Helper Functions for Primitives ==========
// ===== Arithmetic Primitives =====
// ===== Comparison Primitives =====
// ===== Pair/List Primitives =====
// ===== List Search Primitives =====
// ===== Equality Primitives =====
// ===== Multiple Values Primitives =====
// ===== Numeric Operations =====
// ===== String Primitives =====
// ===== Vector Primitives =====
// ===== Vector Conversion Primitives =====
// ===== Vector Operation Primitives =====
```

This makes the split straightforward - the organization already exists!

---

## Proposed Structure: primitives/ Directory

### Target Organization

```
src/eval/
├── mod.rs                      (125 lines) - Core evaluator
├── error.rs                    (29 lines)  - Error types
├── application.rs              (113 lines) - Apply logic
├── special_forms.rs            (1,006 lines) - Special forms
│
└── primitives/
    ├── mod.rs                  (~150 lines) - Dispatcher, installer, helpers
    ├── arithmetic.rs           (~500 lines) - Numbers: +, -, *, /, =, <, >, <=, >=, abs, max, min, quotient, remainder, modulo
    ├── lists.rs                (~450 lines) - Lists: cons, car, cdr, list, length, append, reverse, list-ref, list-tail, memq, memv, member, assq, assv, assoc
    ├── higher_order.rs         (~150 lines) - map, for-each
    ├── predicates.rs           (~200 lines) - Type predicates: 13 functions (number?, integer?, boolean?, string?, symbol?, char?, exact?, inexact?, null?, pair?, list?, vector?, procedure?)
    ├── equality.rs             (~200 lines) - eq?, eqv?, equal? with helper functions
    ├── values.rs               (~50 lines)  - values, call-with-values
    ├── strings.rs              (~600 lines) - String operations: 22 primitives with UTF-8 support
    └── vectors.rs              (~500 lines) - Vector operations: 18 primitives with conversions
```

**Total:** 9 modules, largest ~600 lines (strings), average ~311 lines

---

## Detailed Module Specifications

### 1. `primitives/mod.rs` (~150 lines)

**Purpose:** Central dispatcher and primitive registration

**Contents:**
- `apply_primitive()` - Main dispatcher (60-80 lines with match statement for 61 primitives)
- `install_primitives()` - Registration function (~80 lines)
- Re-exports of all primitive modules
- Helper functions used across multiple modules:
  - `check_arity_exact()`
  - `check_arity_min()`
  - `list_to_vec()`
  - `list_from_vec()`
  - `make_type_predicate()`

**Example:**
```rust
//! Primitive procedures dispatcher and installation
//!
//! This module coordinates all primitive operations across different categories.

mod arithmetic;
mod lists;
mod higher_order;
mod predicates;
mod equality;
mod values;
mod strings;
mod vectors;

use crate::env::Environment;
use crate::value::{Arity, Procedure, Value};
use std::rc::Rc;
use super::error::EvalError;
use super::Evaluator;

impl Evaluator {
    pub(super) fn apply_primitive(&self, name: &str, args: Vec<Value>) -> Result<Value, EvalError> {
        match name {
            // Arithmetic
            "+" => arithmetic::add(self, args),
            "-" => arithmetic::subtract(self, args),
            // ... all 61 primitives

            _ => Err(EvalError::InvalidSyntax(format!("Unknown primitive: {}", name))),
        }
    }

    pub(super) fn install_primitives(env: &Rc<Environment>) {
        let primitives = [
            // Include all 61 primitives with their arities
            ("+", Arity::Min(0)),
            // ...
        ];

        for (name, arity) in primitives {
            env.define(name.to_string(), Value::Procedure(Procedure::Primitive { name, arity }));
        }
    }

    // Helper functions
    fn check_arity_exact(&self, args: &[Value], expected: usize, fn_name: &str) -> Result<(), EvalError> { ... }
    fn check_arity_min(&self, args: &[Value], min: usize, fn_name: &str) -> Result<(), EvalError> { ... }
    fn list_to_vec(&self, list: Value, fn_name: &str) -> Result<Vec<Value>, EvalError> { ... }
    pub(in crate::eval) fn list_from_vec(&self, items: Vec<Value>) -> Value { ... }
    fn make_type_predicate<F>(&self, args: Vec<Value>, predicate: F) -> Result<Value, EvalError>
        where F: Fn(&Value) -> bool { ... }
}
```

---

### 2. `primitives/arithmetic.rs` (~500 lines)

**Purpose:** All numeric operations

**Contents:**
- **Arithmetic:** `+`, `-`, `*`, `/` (with overflow detection, inexact contagion)
- **Comparisons:** `=`, `<`, `>`, `<=`, `>=`
- **Operations:** `quotient`, `remainder`, `modulo`, `abs`, `max`, `min`
- **Helpers:** `NumericValue` enum, conversion functions, comparison helpers

**Example:**
```rust
//! Arithmetic and numeric primitive operations
//!
//! Implements R7RS numeric tower operations including:
//! - Basic arithmetic with overflow detection
//! - Numeric comparisons
//! - Integer division operations
//! - Numeric utilities (abs, max, min)

use super::EvalError;
use crate::value::Value;
use super::Evaluator;
// ... imports for BigInt, BigRational, etc.

/// Internal numeric value representation for operations
enum NumericValue {
    Integer(i64),
    BigInteger(BigInt),
    Rational(BigRational),
    Real(f64),
    Complex(Box<(NumericValue, NumericValue)>),
}

impl NumericValue {
    // Conversion, promotion, arithmetic methods...
}

pub(super) fn add(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    // Implementation...
}

pub(super) fn subtract(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    // Implementation...
}

// ... rest of arithmetic primitives
```

---

### 3. `primitives/lists.rs` (~450 lines)

**Purpose:** List and pair operations

**Contents:**
- **Pairs:** `cons`, `car`, `cdr`
- **List ops:** `list`, `length`, `append`, `reverse`, `list-ref`, `list-tail`
- **Search:** `memq`, `memv`, `member`, `assq`, `assv`, `assoc`

**Example:**
```rust
//! List and pair primitive operations
//!
//! Implements R7RS list operations including:
//! - Pair construction and access
//! - List manipulation
//! - List search and association lists

use super::{EvalError, Evaluator};
use crate::value::Value;
use std::rc::Rc;

pub(super) fn cons(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    // Implementation...
}

// ... rest of list primitives
```

---

### 4. `primitives/higher_order.rs` (~150 lines)

**Purpose:** Higher-order functions

**Contents:**
- `map` - Apply function to lists/vectors
- `for-each` - Iterate for side effects

These are more complex because they call `apply()`:

```rust
//! Higher-order function primitives
//!
//! Implements map and for-each for lists and vectors

use super::{EvalError, Evaluator};
use crate::value::Value;

pub(super) fn map(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.check_arity_min(&args, 2, "map")?;

    let proc = &args[0];
    let lists = &args[1..];

    // Implementation using evaluator.apply()
}

pub(super) fn for_each(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    // Implementation...
}
```

---

### 5. `primitives/predicates.rs` (~200 lines)

**Purpose:** Type checking predicates

**Contents:**
- `number?`, `integer?`, `boolean?`, `string?`, `symbol?`, `char?`
- `exact?`, `inexact?`
- `null?`, `pair?`, `list?`, `vector?`
- `procedure?`
- `boolean=?` (variadic boolean equality)

All of these are simple and use `make_type_predicate()` helper:

```rust
//! Type predicate primitives

use super::{EvalError, Evaluator};
use crate::value::Value;

pub(super) fn number_p(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.make_type_predicate(args, |v| matches!(v,
        Value::Integer(_) | Value::BigInteger(_) | Value::Rational(_) | Value::Real(_) | Value::Complex(_, _)))
}

// ... rest of predicates
```

---

### 6. `primitives/equality.rs` (~200 lines)

**Purpose:** Equality operations

**Contents:**
- `eq?` - Reference equality
- `eqv?` - Value equality
- `equal?` - Structural equality
- Helper functions: `values_eq()`, `values_eqv()`, `values_equal()`

These are more complex due to recursive structural comparison:

```rust
//! Equality primitive operations
//!
//! Implements R7RS equality predicates with proper semantics:
//! - eq?: Reference equality (pointer comparison)
//! - eqv?: Value equality (same type, same value)
//! - equal?: Structural equality (deep comparison)

use super::{EvalError, Evaluator};
use crate::value::Value;

pub(super) fn eq(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    // Implementation with values_eq helper
}

// Recursive helper for equal?
fn values_equal(a: &Value, b: &Value) -> Result<bool, EvalError> {
    // Deep structural comparison for pairs, vectors, etc.
}
```

---

### 7. `primitives/values.rs` (~50 lines)

**Purpose:** Multiple values support

**Contents:**
- `values` - Return multiple values
- `call-with-values` - Receive multiple values

Simple module:

```rust
//! Multiple values primitives (R7RS Section 6.10)

use super::{EvalError, Evaluator};
use crate::value::Value;

pub(super) fn values(_evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    match args.len() {
        0 => Ok(Value::Values(vec![])),
        1 => Ok(args.into_iter().next().unwrap()),
        _ => Ok(Value::Values(args)),
    }
}

pub(super) fn call_with_values(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    // Implementation using evaluator.apply()
}
```

---

### 8. `primitives/strings.rs` (~600 lines)

**Purpose:** String operations with full UTF-8 support

**Contents:** 22 string primitives
- **Basic:** `string-length`, `string-ref`, `string-set!`, `make-string`, `string`
- **Comparison:** `string=?`, `string<?`, `string>?`, `string<=?`, `string>=?`
- **Case-insensitive:** `string-ci=?`, `string-ci<?`, `string-ci>?`, `string-ci<=?`, `string-ci>=?`
- **Operations:** `string-append`, `substring`, `string-copy`
- **Conversions:** `string->list`, `list->string`

```rust
//! String primitive operations with UTF-8 support
//!
//! Implements R7RS string operations (Section 6.7):
//! - Character-based indexing (O(n) as allowed by R7RS)
//! - Full Unicode support
//! - Mutable strings via string-set!

use super::{EvalError, Evaluator};
use crate::value::Value;
use std::cell::RefCell;
use std::rc::Rc;

pub(super) fn string_length(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    // Implementation using .chars().count()
}

// ... all 22 string primitives
```

---

### 9. `primitives/vectors.rs` (~500 lines)

**Purpose:** Vector operations

**Contents:** 18 vector primitives
- **Basic:** `vector?`, `make-vector`, `vector`, `vector-length`, `vector-ref`, `vector-set!`
- **Conversions:** `vector->list`, `list->vector`, `vector->string`, `string->vector`
- **Operations:** `vector-copy`, `vector-copy!`, `vector-append`, `vector-fill!`
- **Higher-order:** `vector-map`, `vector-for-each`

```rust
//! Vector primitive operations
//!
//! Implements R7RS vector operations (Section 6.8):
//! - O(1) random access
//! - Mutable via vector-set!
//! - Full conversion support

use super::{EvalError, Evaluator};
use crate::value::Value;
use std::cell::RefCell;
use std::rc::Rc;

pub(super) fn vector_p(evaluator: &Evaluator, args: Vec<Value>) -> Result<Value, EvalError> {
    evaluator.make_type_predicate(args, |v| matches!(v, Value::Vector(_)))
}

// ... all 18 vector primitives
```

---

## Implementation Plan

### Phase 2.1: Create Directory Structure (30 minutes)

1. Create `src/eval/primitives/` directory
2. Create empty module files:
   ```bash
   mkdir src/eval/primitives
   touch src/eval/primitives/{mod,arithmetic,lists,higher_order,predicates,equality,values,strings,vectors}.rs
   ```

3. Set up `primitives/mod.rs` with basic structure:
   ```rust
   mod arithmetic;
   mod lists;
   mod higher_order;
   mod predicates;
   mod equality;
   mod values;
   mod strings;
   mod vectors;

   // Re-export helpers as pub(in crate::eval)
   ```

### Phase 2.2: Extract Modules (2-3 hours)

**Order of extraction** (from simplest to most complex):

1. **values.rs** (easiest, ~50 lines, no dependencies)
   - Extract `primitive_values()`, `primitive_call_with_values()`
   - Test: `cargo test values`

2. **predicates.rs** (~200 lines, uses `make_type_predicate` helper)
   - Extract all 13 type predicates
   - Test: `cargo test predicates`

3. **equality.rs** (~200 lines, self-contained recursive functions)
   - Extract `primitive_eq()`, `primitive_eqv()`, `primitive_equal()`
   - Extract helpers: `values_eq()`, `values_eqv()`, `values_equal()`
   - Test: `cargo test equality`

4. **higher_order.rs** (~150 lines, needs `apply()` access)
   - Extract `primitive_map()`, `primitive_for_each()`
   - These call `self.apply()` so need `&Evaluator`
   - Test: `cargo test map for-each`

5. **lists.rs** (~450 lines, moderate complexity)
   - Extract all pair and list operations
   - Test: `cargo test lists`

6. **arithmetic.rs** (~500 lines, has `NumericValue` helper type)
   - Extract `NumericValue` enum and all arithmetic
   - Test: `cargo test numbers arithmetic`

7. **strings.rs** (~600 lines, most recently added)
   - Extract all 22 string primitives
   - Test: `cargo test strings`

8. **vectors.rs** (~500 lines, most recently added)
   - Extract all 18 vector primitives
   - Test: `cargo test vectors`

### Phase 2.3: Update Dispatcher (1 hour)

1. Update `primitives/mod.rs::apply_primitive()` to call functions from submodules:
   ```rust
   match name {
       "+" => arithmetic::add(self, args),
       "-" => arithmetic::subtract(self, args),
       "cons" => lists::cons(self, args),
       // ... etc
   }
   ```

2. Move helper functions to `primitives/mod.rs` as `pub(in crate::eval)` or `pub(super)`

3. Update `install_primitives()` if needed

### Phase 2.4: Clean Up Imports (30 minutes)

1. Each module needs:
   ```rust
   use super::{EvalError, Evaluator};
   use crate::value::Value;
   // ... module-specific imports
   ```

2. Update `src/eval/mod.rs` to reference `primitives::apply_primitive` correctly

### Phase 2.5: Verification (30 minutes)

1. Run full test suite:
   ```bash
   cargo test
   cargo test --test compliance
   cargo test --test integration
   ```

2. Check clippy:
   ```bash
   cargo clippy --all-targets --all-features -- -D warnings
   ```

3. Check formatting:
   ```bash
   cargo fmt --check
   ```

4. Verify no performance regression (if benchmarks exist)

---

## Success Criteria

- ✅ `primitives.rs` deleted
- ✅ No file in `primitives/` > 600 lines
- ✅ All tests pass (237/237)
- ✅ No clippy warnings
- ✅ Clear module organization mirroring R7RS spec sections
- ✅ Easy to add new primitive categories in future

---

## Benefits

1. **Maintainability:** Each category in its own 300-500 line file
2. **Navigability:** Easy to find string operations in `strings.rs`
3. **Extensibility:** Future I/O primitives go in new `io.rs` module
4. **Testing:** Can test categories independently
5. **Documentation:** Module-level docs explain each category
6. **R7RS alignment:** Structure mirrors spec (6.2 = arithmetic, 6.4 = lists, 6.7 = strings, 6.8 = vectors)

---

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| Breaking tests during extraction | High | Extract one module at a time, test after each |
| Incorrect visibility modifiers | Medium | Use `pub(super)` for primitives, `pub(in crate::eval)` for helpers |
| Circular dependencies | Medium | Keep helpers in `mod.rs`, only data flows to submodules |
| Import complexity | Low | Clear re-export strategy in `mod.rs` |

---

## Timeline Estimate

| Phase | Task | Time | Cumulative |
|-------|------|------|------------|
| 2.1 | Create directory structure | 30min | 30min |
| 2.2.1 | Extract values.rs | 15min | 45min |
| 2.2.2 | Extract predicates.rs | 20min | 1h 05min |
| 2.2.3 | Extract equality.rs | 20min | 1h 25min |
| 2.2.4 | Extract higher_order.rs | 20min | 1h 45min |
| 2.2.5 | Extract lists.rs | 30min | 2h 15min |
| 2.2.6 | Extract arithmetic.rs | 30min | 2h 45min |
| 2.2.7 | Extract strings.rs | 30min | 3h 15min |
| 2.2.8 | Extract vectors.rs | 30min | 3h 45min |
| 2.3 | Update dispatcher | 1h | 4h 45min |
| 2.4 | Clean up imports | 30min | 5h 15min |
| 2.5 | Verification & testing | 30min | 5h 45min |

**Total Estimated Time:** ~6 hours (can be done in one focused session)

---

## Next Steps

1. Review this PRD with user
2. Get approval to proceed
3. Start with Phase 2.1 (directory structure)
4. Extract modules one at a time
5. Test after each extraction
6. Final verification
7. Update `CLAUDE.md` with new structure
8. Mark Phase 2 complete, consider Phase 3 (special_forms split)

---

## Appendix: Primitive Count by Category

| Category | Count | Files |
|----------|-------|-------|
| Arithmetic operations | 9 | arithmetic.rs |
| Numeric comparisons | 5 | arithmetic.rs |
| Numeric operations | 6 | arithmetic.rs |
| Pair operations | 3 | lists.rs |
| List operations | 6 | lists.rs |
| List search | 6 | lists.rs |
| Higher-order | 2 | higher_order.rs |
| Type predicates | 13 | predicates.rs |
| Equality | 3 | equality.rs |
| Multiple values | 2 | values.rs |
| Strings | 22 | strings.rs |
| Vectors | 18 | vectors.rs |
| **Total** | **95** | **8 files** |

*(Note: Some primitives like `boolean=?` could be in either predicates or equality - organizational choice)*

---

**Status:** Ready to implement
**Recommended:** Proceed with Phase 2 refactoring
