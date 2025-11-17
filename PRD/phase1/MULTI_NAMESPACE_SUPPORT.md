# Multi-Namespace Library Support

**Status:** Phase 1 Planning
**Date:** 2025-11-16
**Priority:** HIGH - Blocks R7RS compliance
**Estimated Effort:** 8-12 hours

## Executive Summary

Patina currently has a **single-namespace limitation** that prevents using procedures from libraries other than `(scheme base)`. While the `import` special form is fully implemented, and the library loading infrastructure exists, all primitive procedure lookups are hardcoded to the `"scheme.base/"` namespace.

This document describes how to implement multi-namespace support for **procedures only** (special forms will be handled separately in a future phase). The key insight is that the REPL auto-imports `(scheme base)` per R7RS spec, so if we fix the namespace lookup and implement non-base libraries, users can simply use `(import (scheme inexact))` to access `sin`, `cos`, etc.

---

## Table of Contents

1. [Current State Analysis](#1-current-state-analysis)
2. [R7RS Requirements](#2-r7rs-requirements)
3. [Problem Statement](#3-problem-statement)
4. [Solution Design](#4-solution-design)
5. [How Import Works](#5-how-import-works)
6. [REPL Behavior](#6-repl-behavior)
7. [Implementation Plan](#7-implementation-plan)
8. [Testing Strategy](#8-testing-strategy)
9. [Future Work](#9-future-work)

---

## 1. Current State Analysis

### 1.1 What Works ✅

**Import Infrastructure:**
- ✅ `import` special form fully implemented (`special_forms/import.rs`)
- ✅ Import modifiers work: `only`, `except`, `prefix`, `rename`
- ✅ Library loading pipeline complete
- ✅ `LibraryRegistry` manages loaded libraries
- ✅ `LibraryLoader` supports Rust and Scheme libraries
- ✅ Search paths and circular dependency detection

**Library Organization:**
- ✅ 13 R7RS libraries registered (see `eval/mod.rs:102-159`)
- ✅ Libraries have separate environments
- ✅ Libraries track exports
- ✅ REPL auto-imports `(scheme base)` per spec

### 1.2 What's Broken ❌

**Single Namespace Lookup:**

```rust
// From primitives/mod.rs:54-58
// TODO: This hardcodes "scheme.base" which is a temporary hack!
let qualified_name = format!("scheme.base/{}", name);
if let Ok(result) = self.primitive_registry.apply(&qualified_name, args.clone(), ...) {
    return Ok(result);
}
```

**Impact:**
- Primitives from `(scheme inexact)`, `(scheme char)`, `(scheme write)`, etc. **cannot be called**
- Even though libraries are loaded and have exports, primitives aren't accessible
- Most registered libraries are stubs returning empty exports

**Library Stubs:**

8 out of 13 libraries are empty stubs:
- `(scheme lazy)`, `(scheme time)`, `(scheme file)`
- `(scheme read)`, `(scheme write)`, `(scheme eval)`
- `(scheme process-context)`, `(scheme case-lambda)`, `(scheme r5rs)`

---

## 2. R7RS Requirements

### 2.1 Programs vs REPL

**Programs** (`r7rs-small-spec/prog.tex:31-32`):
> "The initial environment of a program is empty, so at least one import declaration is needed to introduce initial bindings."

**REPL** (`r7rs-small-spec/prog.tex:639-647`):
> "For convenience and ease of use, the global Scheme environment in a REPL must not be empty, but must start out with at least the bindings provided by the base library."

**Key Point:**
- Programs: Start empty, must import everything
- REPL: Starts with `(scheme base)` pre-imported

### 2.2 Library Separation

From the spec example (`r7rs-small-spec/overview-body.tex:600-604`):

```scheme
> (sin 4)
Undefined variable: sin
> (import (scheme inexact))
> (sin 4)
-0.756802495307928
```

This shows:
- `sin` is in `(scheme inexact)`, **NOT** `(scheme base)`
- Even in REPL, you must import non-base libraries explicitly
- Libraries must properly namespace their procedures

### 2.3 Standard Libraries

R7RS defines these standard libraries (partial list):

| Library | Purpose | Status in Patina |
|---------|---------|------------------|
| `(scheme base)` | Core procedures and syntax | ✅ Implemented |
| `(scheme inexact)` | Inexact arithmetic (sin, cos, etc.) | ❌ Stub |
| `(scheme complex)` | Complex numbers | ❌ Stub |
| `(scheme char)` | Character operations | ❌ Stub |
| `(scheme read)` | Read operations | ❌ Stub |
| `(scheme write)` | Write operations | ❌ Stub |
| `(scheme file)` | File I/O | ❌ Stub |
| `(scheme lazy)` | Promises and delays | ❌ Stub |
| `(scheme eval)` | Runtime evaluation | ❌ Stub |
| `(scheme process-context)` | Environment variables | ❌ Stub |
| `(scheme case-lambda)` | Variable arity | ❌ Stub |

---

## 3. Problem Statement

### 3.1 The Core Issue

Primitives don't carry library namespace information:

```rust
// Current (WRONG)
pub enum Procedure {
    Primitive {
        name: &'static str,
        arity: Arity,
        // Missing: library namespace!
    },
    Lambda { /* ... */ },
}
```

When applying a primitive, we hardcode `"scheme.base/"`:

```rust
// Current (WRONG)
let qualified_name = format!("scheme.base/{}", name);
self.primitive_registry.apply(&qualified_name, ...)
```

This means:
- `(sin 4)` looks up `"scheme.base/sin"` → not found → error
- Even if we register `"scheme.inexact/sin"`, it's never called
- Libraries can't export their primitives properly

### 3.2 Why Import Alone Doesn't Fix It

Even though `import` works correctly:

1. User does: `(import (scheme inexact))`
2. Import loads library and adds exports to environment
3. Environment now has `sin` bound to `Procedure::Primitive { name: "sin", ... }`
4. User calls: `(sin 4)`
5. Evaluator applies primitive with hardcoded `"scheme.base/sin"` lookup
6. Registry doesn't find it → error!

**The fix:** Primitives must store their library, and lookup must use it.

---

## 4. Solution Design

### 4.1 Add Library Field to Primitives

```rust
// In patina-runtime/src/value/mod.rs

pub enum Procedure {
    Primitive {
        name: &'static str,
        arity: Arity,
        library: Vec<String>,  // NEW: e.g., ["scheme", "inexact"]
    },
    Lambda { /* ... */ },
}
```

**Impact:**
- All `Procedure::Primitive` constructions must add `library` field
- All pattern matches must handle the new field

### 4.2 Update Primitive Registry

The primitive registry already uses namespaced keys like `"scheme.base/+"`. We just need to ensure primitives carry their library when created.

**Current registration** (in `primitives/registry.rs`):

```rust
registry.register(
    "scheme.base/+",  // Qualified name
    Arity::Min(0),
    Box::new(arithmetic::add),
);
```

**New primitive creation** (in `stdlib/scheme_base.rs`):

```rust
pub fn build_scheme_base(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let library_name = vec!["scheme".to_string(), "base".to_string()];

    let primitives = [
        ("+", Arity::Min(0)),
        ("-", Arity::Min(1)),
        // ... etc
    ];

    for (name, arity) in &primitives {
        env.define(
            name.to_string(),
            Value::Procedure(Procedure::Primitive {
                name,
                arity: arity.clone(),
                library: library_name.clone(),  // NEW
            }),
        );
    }

    // Return export list
    primitives.iter().map(|(name, _)| name.to_string()).collect()
}
```

### 4.3 Update Primitive Application

**Current** (`primitives/mod.rs:45-64`):

```rust
pub(super) fn apply_primitive(
    &self,
    name: &str,
    args: Vec<Value>,
    in_tail_position: bool,
) -> Result<EvalResult, EvalError> {
    // WRONG: Hardcoded namespace
    let qualified_name = format!("scheme.base/{}", name);
    self.primitive_registry.apply(&qualified_name, args, self, in_tail_position)
}
```

**New approach:**

```rust
pub(super) fn apply_primitive(
    &self,
    proc: &Procedure,
    args: Vec<Value>,
    in_tail_position: bool,
) -> Result<EvalResult, EvalError> {
    match proc {
        Procedure::Primitive { name, arity, library } => {
            // Use library from primitive
            let qualified_name = format!("{}/{}", library.join("."), name);
            self.primitive_registry.apply(&qualified_name, args, self, in_tail_position)
        }
        Procedure::Lambda { /* ... */ } => {
            // Handle lambda application
            self.apply_lambda(/* ... */)
        }
    }
}
```

### 4.4 Update Application Site

**In eval dispatch** (`eval/mod.rs`):

```rust
// When we have a procedure to call
match procedure_value {
    Value::Procedure(proc) => {
        let args = self.eval_args(args_list, env)?;
        // NEW: Pass entire procedure, not just name
        self.apply_primitive(&proc, args, in_tail_position)
    }
    _ => Err(EvalError::NotCallable(/* ... */))
}
```

---

## 5. How Import Works

### 5.1 Current Implementation

The `import` special form is already fully implemented in `special_forms/import.rs`.

**When you do:** `(import (scheme inexact))`

**What happens:**

1. **Parse import set** - `LibraryDefinition::parse_import_set()` parses the syntax
2. **Load library** - `evaluator.load_library(&["scheme", "inexact"])` loads it
3. **Process import** - `evaluator.process_import_for_eval(&import_set, env)` adds exports to environment
4. **Bindings available** - Procedures from library are now in current environment

### 5.2 Import Modifiers

All R7RS import modifiers work:

```scheme
;; Import all
(import (scheme inexact))

;; Import specific identifiers
(import (only (scheme inexact) sin cos tan))

;; Import all except
(import (except (scheme inexact) atan))

;; Add prefix
(import (prefix (scheme inexact) math:))
;; Use as: (math:sin 4)

;; Rename
(import (rename (scheme inexact) (sin sine) (cos cosine)))
;; Use as: (sine 4)
```

**Implementation:** `process_import_for_eval()` in `eval/mod.rs:997-1089`

### 5.3 Library Loading Pipeline

```
User: (import (scheme inexact))
  ↓
parse_import_set()
  ↓
load_library(&["scheme", "inexact"])
  ↓
├─ Check if already loaded (cache hit) → return cached
│
├─ Try simple loaders (RustLibraryLoader)
│  ├─ find builder: build_scheme_inexact
│  ├─ Create library environment
│  ├─ Call builder to populate environment
│  └─ Register library in registry
│
├─ Try evaluating loaders (SchemeLibraryLoader)
│  ├─ Find .sld file: lib/scheme/inexact.sld
│  ├─ Parse define-library form
│  ├─ Evaluate library body
│  └─ Collect exports
│
└─ Register and return library
  ↓
process_import_for_eval(&import_set, current_env)
  ↓
├─ Get library exports
├─ Apply modifiers (only, except, prefix, rename)
└─ Define bindings in current environment
```

---

## 6. REPL Behavior

### 6.1 R7RS Requirement

From spec: "The global Scheme environment in a REPL must not be empty, but must start out with at least the bindings provided by the base library."

### 6.2 Current Implementation

**In `eval/mod.rs:169-192` (`load_bootstrap` method):**

```rust
fn load_bootstrap(&self) {
    // Load (scheme base) library first
    let _ = self.load_library(&["scheme".to_string(), "base".to_string()]);

    // Load Scheme-implemented extras into (scheme base)
    self.load_scheme_base_extras();

    // Load test framework
    let _ = self.load_library(&["chibi".to_string(), "test".to_string()]);

    // AUTO-IMPORT (scheme base) into global environment
    // This makes primitives and macros available without explicit import
    if let Some(lib) = self.library_registry.borrow()
        .get(&["scheme".to_string(), "base".to_string()])
    {
        for (name, value) in lib.exports.iter() {
            self.global_env.define(name.clone(), value.clone());
        }
    }
}
```

**This is CORRECT per R7RS spec!**

### 6.3 Why This Makes Import Simple

Because REPL auto-imports `(scheme base)`:

1. User starts REPL
2. `(scheme base)` already imported → `+`, `-`, `define`, `lambda`, etc. work
3. User wants `sin` → `(import (scheme inexact))`
4. Now `sin` works too!

**For other libraries:**
- Just implement the library builder
- Register it in `init_loaders()`
- Users can `(import ...)` it

No special REPL logic needed!

---

## 7. Implementation Plan

### Phase 1: Extend Type System (2 hours)

**File:** `crates/patina-runtime/src/value/mod.rs`

**Changes:**
1. Add `library: Vec<String>` field to `Procedure::Primitive`
2. Update `Display` implementation to show library (for debugging)
3. Update `Clone`, `Debug` derives (automatic)

**Example:**
```rust
Procedure::Primitive {
    name: "sin",
    arity: Arity::Exact(1),
    library: vec!["scheme".to_string(), "inexact".to_string()],
}
```

### Phase 2: Update Library Builders (3-4 hours)

**Files:**
- `crates/patina-runtime/src/stdlib/*.rs`

**For each library builder (`build_scheme_base`, `build_chibi_test`, etc.):**

1. Add library name construction at top
2. Update all `Procedure::Primitive` creations to include `library` field

**Example diff:**
```diff
 pub fn build_scheme_base(_name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
+    let library_name = vec!["scheme".to_string(), "base".to_string()];
+
     let primitives = [
         ("+", Arity::Min(0)),
         // ...
     ];

     for (name, arity) in &primitives {
         env.define(
             name.to_string(),
             Value::Procedure(Procedure::Primitive {
                 name,
                 arity: arity.clone(),
+                library: library_name.clone(),
             }),
         );
     }
     // ...
 }
```

**Libraries to update:**
- `scheme_base.rs` (141 primitives)
- `chibi_test.rs`
- `scheme_char.rs`
- `scheme_complex.rs`
- `scheme_inexact.rs`
- All stubs in `scheme_stubs.rs`

### Phase 3: Update Primitive Application (2-3 hours)

**File:** `crates/patina-tree-walker/src/eval/primitives/mod.rs`

**Changes:**

1. Change `apply_primitive` signature:
```rust
// OLD
pub(super) fn apply_primitive(
    &self,
    name: &str,
    args: Vec<Value>,
    in_tail_position: bool,
) -> Result<EvalResult, EvalError>

// NEW
pub(super) fn apply_primitive(
    &self,
    proc: &Procedure,
    args: Vec<Value>,
    in_tail_position: bool,
) -> Result<EvalResult, EvalError>
```

2. Update implementation to use `proc.library`:
```rust
match proc {
    Procedure::Primitive { name, library, .. } => {
        let qualified_name = format!("{}/{}", library.join("."), name);
        self.primitive_registry.apply(&qualified_name, args, self, in_tail_position)
    }
    Procedure::Lambda { /* ... */ } => {
        // Handle lambda
    }
}
```

3. Update call sites in `eval/mod.rs` to pass procedure instead of name

### Phase 4: Update Application Call Sites (1-2 hours)

**File:** `crates/patina-tree-walker/src/eval/application.rs`

**Find all calls to `apply_primitive` and update them:**

```diff
- self.apply_primitive(name, args, in_tail_position)
+ self.apply_primitive(&procedure, args, in_tail_position)
```

**Also check:**
- Special form `apply`
- Any other direct primitive invocations

### Phase 5: Testing (2 hours)

**Create tests in `crates/patina-tests/tests/multi_namespace_test.rs`:**

```rust
#[test]
fn test_scheme_base_primitives() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    // Should work - (scheme base) auto-imported in REPL
    let result = interp.eval_str("(+ 1 2 3)").unwrap();
    assert_eq!(result.to_string(), "6");
}

#[test]
fn test_import_scheme_inexact() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (import (scheme inexact))
        (sin 0.0)
        "#
    ).unwrap();

    assert_eq!(result.to_string(), "0.0");
}

#[test]
fn test_scheme_char_predicates() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (import (scheme char))
        (char-alphabetic? #\a)
        "#
    ).unwrap();

    assert_eq!(result.to_string(), "#t");
}

#[test]
fn test_import_only_modifier() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (import (only (scheme inexact) sin cos))
        (sin 0.0)
        "#
    ).unwrap();

    assert_eq!(result.to_string(), "0.0");

    // tan should not be available
    let err = interp.eval_str("(tan 0.0)");
    assert!(err.is_err());
}

#[test]
fn test_import_prefix_modifier() {
    let interp = TreeWalkInterpreter::new_tree_walker();

    let result = interp.eval_program(
        r#"
        (import (prefix (scheme inexact) math:))
        (math:sin 0.0)
        "#
    ).unwrap();

    assert_eq!(result.to_string(), "0.0");
}
```

---

## 8. Testing Strategy

### 8.1 Unit Tests

**After each phase, verify:**
- Phase 1: Value enum compiles, primitives have library field
- Phase 2: Each library builder creates primitives with correct library
- Phase 3: `apply_primitive` uses library from primitive
- Phase 4: All call sites compile and pass

### 8.2 Integration Tests

**After all phases:**
- Test each R7RS library separately
- Test import modifiers with each library
- Test error messages mention correct library
- Test REPL auto-import of `(scheme base)`

### 8.3 Regression Tests

**Ensure existing tests still pass:**
```bash
cargo test --package patina-tests
```

**Expected:**
- Most tests should continue passing (they use `scheme base`)
- Some may need `(import ...)` statements added
- Tests using primitives directly may need updates

---

## 9. Future Work

### 9.1 Special Forms / Syntax

After multi-namespace primitives work, handle special forms similarly:

1. Add `Value::Syntax { name, library }` variant
2. Export special forms from libraries (e.g., `lambda` from `scheme base`)
3. Make special form lookup environment-based, not just global registry
4. Keep certain forms always available: `import`, `define-library`

**See future doc:** `SPECIAL_FORM_NAMESPACING.md` (to be created)

### 9.2 Implement Missing Libraries

**Priority order:**

1. **`(scheme inexact)`** - `sin`, `cos`, `tan`, `exp`, `log`, `sqrt`, etc.
   - Wrapper around Rust `f64` methods
   - ~15 procedures, 2-3 hours

2. **`(scheme char)`** - Character predicates and operations
   - `char-alphabetic?`, `char-upper-case?`, `char-downcase`, etc.
   - ~20 procedures, 3-4 hours

3. **`(scheme write)`** - Writing operations
   - Most already in `(scheme base)`, just need exports
   - 1 hour

4. **`(scheme complex)`** - Complex number operations
   - `make-rectangular`, `make-polar`, `real-part`, `imag-part`, etc.
   - Already have complex support, just need exports
   - 2 hours

5. **`(scheme read)`** - Read operations
   - `read`, `read-char`, `peek-char`, etc.
   - Needs port support first
   - 4-6 hours

6. **`(scheme file)`** - File I/O
   - `open-input-file`, `close-port`, etc.
   - Needs port support
   - 6-8 hours

### 9.3 Program vs REPL Distinction

Currently, REPL always auto-imports `(scheme base)`. For strict compliance:

1. Add `ExecutionContext` enum: `Program` | `REPL`
2. `Interpreter::new()` → REPL mode (auto-import base)
3. `Interpreter::new_program()` → Program mode (empty start)
4. Tests should use program mode to verify explicit imports

**Estimated:** 2-3 hours

---

## Summary

### The Big Picture

**What we have:**
- ✅ Import works perfectly
- ✅ Library loading works perfectly
- ✅ REPL auto-imports `(scheme base)` per spec

**What we're fixing:**
- ❌ Primitives don't carry library information
- ❌ Lookup hardcoded to `scheme.base/`

**The fix:**
1. Add `library` field to primitives
2. Use stored library in lookup
3. Implement non-base libraries

**Result:**
- Users can `(import (scheme inexact))` and use `sin`
- Each library works in its own namespace
- R7RS compliance unblocked

**Effort:** 8-12 hours for multi-namespace primitives

**After this:** Implement missing R7RS libraries (~20-40 hours depending on scope)

---

## References

- R7RS spec: `spec/r7rs-small-spec/`
- Library system redesign: `PRD/phase1/LIBRARY_SYSTEM_REDESIGN.md`
- Import implementation: `crates/patina-tree-walker/src/eval/special_forms/import.rs`
- Primitive registry: `crates/patina-tree-walker/src/eval/primitives/registry.rs`
- Library loaders: `crates/patina-runtime/src/library_loader.rs`
