# Library System Redesign

**Status:** Phase 1 Complete ✅
**Date Started:** 2025-11-13
**Phase 1 Completed:** 2025-11-15
**Target:** Phase 1 - R7RS Library System Completion
**Estimated Effort:** 22-32 hours over 4 phases
**Actual Effort (Phase 1):** ~3 hours

## Implementation Status

### ✅ Phase 1: Decouple Bootstrap (COMPLETE)
**Completed:** 2025-11-15
**Effort:** ~3 hours

**What was accomplished:**
- ✅ Created `lib/scheme/base-extras.scm` (346 lines) with all R7RS-required macros and derived functions
- ✅ Created `lib/chibi/test-extras.scm` (127 lines) with test framework macros
- ✅ Implemented `load_scheme_base_extras()` to load extras into `(scheme base)` library environment
- ✅ Updated evaluator initialization to load libraries before importing to global environment
- ✅ **Deleted `bootstrap.scm` entirely** - no longer needed!
- ✅ All 483 tests passing with library-based system

**Key Files Changed:**
- `crates/patina-tree-walker/src/eval/mod.rs:169-273` - New library loading system
- `lib/scheme/base-extras.scm` - Created (R7RS base library extras)
- `lib/chibi/test-extras.scm` - Created (test framework)
- `lib/bootstrap.scm` - **DELETED**

**Benefits Achieved:**
- Clean separation: primitives (Rust) + derived functions/macros (Scheme)
- Proper R7RS library structure
- Foundation for mixed Rust/Scheme libraries
- Bootstrap eliminated, reducing global namespace pollution

### 🚧 Phase 2: Registry-Aware Builders (NOT STARTED)
**Status:** Planned
**Estimated Effort:** 6-8 hours

**Objective:** Eliminate duplication between primitive registry and library builders

**Current Issue:** 141 primitives listed twice:
- Once in primitive registry (with full metadata)
- Again in `build_scheme_base()` (duplicate arity specifications)

**Planned Solution:** Update `RustLibraryBuilder` signature to accept `&PrimitiveRegistry` parameter

### 🚧 Phase 3: Namespace-Aware Primitives (NOT STARTED)
**Status:** Planned
**Estimated Effort:** 4-6 hours

**Objective:** Remove hardcoded `"scheme.base/"` namespace limitation

**Current Issue:** Only primitives from `scheme.base` can be called (hardcoded in `apply_primitive`)

**Planned Solution:** Add `library` field to `Procedure::Primitive` variant

### 🚧 Phase 4: Special Libraries (NOT STARTED)
**Status:** Planned
**Estimated Effort:** 8-12 hours

**Objective:** Implement special libraries with custom semantics
- `(scheme lazy)` - delays and promises
- `(scheme eval)` - runtime evaluation
- `(scheme case-lambda)` - variable arity syntax
- `(scheme process-context)` - environment variables

---

## Executive Summary

This document describes a comprehensive redesign of Patina's library system to:
1. Eliminate duplication between primitive registry and library definitions
2. Support mixed Rust/Scheme libraries for flexible implementation
3. Enable special libraries like `(scheme lazy)`, `(scheme eval)`, `(scheme case-lambda)`
4. Achieve full R7RS compliance for the library system
5. Provide a clean foundation for user-defined libraries

**Key Innovation:** Mixed Rust/Scheme libraries allow performance-critical code in Rust while keeping derived functions and utilities in Scheme, enabling rapid library development without sacrificing performance.

---

## Table of Contents

1. [Current Architecture Analysis](#1-current-architecture-analysis)
2. [Design Goals](#2-design-goals)
3. [Proposed Architecture](#3-proposed-architecture)
4. [Implementation Phases](#4-implementation-phases)
5. [Mixed Library Pattern](#5-mixed-library-pattern)
6. [Special Library Support](#6-special-library-support)
7. [Migration Strategy](#7-migration-strategy)
8. [Decision Points](#8-decision-points)
9. [Future Extensions](#9-future-extensions)

---

## 1. Current Architecture Analysis

### 1.1 Current Components

**Primitive Registry** (`crates/patina-tree-walker/src/eval/primitives/registry.rs`):
```rust
pub struct PrimitiveRegistry {
    primitives: HashMap<String, PrimitiveFn>,  // "scheme.base/+" -> function
}

pub struct PrimitiveFn {
    pub library: &'static str,  // "scheme.base"
    pub name: &'static str,      // "+"
    pub arity: Arity,
    pub help: &'static str,
    pub handler: PrimitiveHandler,
}
```

**Library Loaders** (`crates/patina-runtime/src/library_loader.rs`):
- `LibraryLoader` trait: For self-contained libraries (Rust)
- `EvaluatingLibraryLoader` trait: For libraries needing evaluation (Scheme)

**Rust Library Builder** (`crates/patina-runtime/src/rust_library_loader.rs`):
```rust
pub type RustLibraryBuilder = fn(Vec<String>, Rc<Environment>) -> Vec<String>;

// Example: build_scheme_base manually lists all primitives
pub fn build_scheme_base(name: Vec<String>, env: Rc<Environment>) -> Vec<String> {
    let primitives = [
        ("+", Arity::Min(0)),
        ("-", Arity::Min(1)),
        // ... 100+ more manual entries
    ];
    // ...
}
```

**Bootstrap** (`lib/bootstrap.scm`):
- Loaded directly at evaluator startup
- Contains macros (`let`, `cond`, `case`) and derived functions (`caar`, `not`)
- Not part of any library - injected into global scope

### 1.2 Problems

| Problem | Impact | Example |
|---------|--------|---------|
| **Duplicate primitive lists** | Maintenance burden, inconsistency risk | Registry has `"scheme.base/+"`, builder lists `"+"` again |
| **Bootstrap confusion** | Not R7RS compliant | `caar` comes from bootstrap, not `(scheme base)` |
| **Hardcoded namespace** | Can't use non-base primitives | `apply_primitive` only checks `"scheme.base/*"` |
| **No mixed library support** | Can't combine Rust + Scheme easily | Must choose: pure Rust OR pure Scheme |
| **No special library strategy** | Blocks R7RS compliance | Can't implement `(scheme lazy)`, `(scheme eval)` |

### 1.3 File Locations Reference

```
crates/
├── patina-runtime/src/
│   ├── library_loader.rs          # Loader traits
│   ├── rust_library_loader.rs     # Rust library implementation
│   └── stdlib/                     # Library builders
│       ├── scheme_base.rs          # build_scheme_base()
│       ├── scheme_char.rs
│       └── ...
├── patina-tree-walker/src/
│   ├── eval/
│   │   ├── primitives/registry.rs  # Primitive registry
│   │   └── mod.rs                  # Evaluator initialization
│   └── library_support.rs          # Scheme library loader
└── patina-frontend/src/
    └── library_parser.rs            # Parse define-library forms

lib/
├── scheme/
│   └── base-extras.scm              # R7RS base library extras (NEW)
└── chibi/
    └── test-extras.scm              # Test framework (NEW)
```

**Note:** `bootstrap.scm` has been eliminated (Phase 1 complete).

---

## 2. Design Goals

### 2.1 Primary Goals

1. **Single Source of Truth**
   - Registry defines primitives with library namespaces
   - Library builders query registry, no duplication

2. **Mixed Rust/Scheme Libraries**
   - Performance-critical code in Rust
   - Derived functions and utilities in Scheme
   - Single library combines both seamlessly

3. **R7RS Compliance**
   - Bootstrap minimal or removed
   - All standard procedures come from libraries
   - Support all R7RS standard libraries

4. **Extensibility**
   - Easy to add new libraries
   - Clear pattern for user-defined libraries
   - Support special semantics when needed

### 2.2 Non-Goals (Future Work)

- Pure `.sld` library format (Rust builders are fine for now)
- Hot reloading of libraries
- Library versioning
- Cross-implementation compatibility

### 2.3 Success Criteria

**Phase 1 (COMPLETE ✅):**
- [ ] No duplicate primitive lists *(Deferred to Phase 2)*
- [x] **`bootstrap.scm` minimal or empty** ✅ **DELETED ENTIRELY!**
- [x] **All existing tests pass** ✅ **483 tests passing**
- [x] **Mixed library foundation** ✅ **`(scheme base)` = Rust primitives + Scheme extras**

**Remaining Phases:**
- [ ] No duplicate primitive lists *(Phase 2: Registry-Aware Builders)*
- [ ] Can implement `(scheme lazy)`, `(scheme eval)`, `(scheme case-lambda)`, `(scheme process-context)` *(Phase 4)*
- [ ] Can access primitives from all namespaces *(Phase 3: Namespace-Aware Primitives)*

---

## 3. Proposed Architecture

### 3.1 Registry-Aware Library Builders

**New signature:**
```rust
pub type RustLibraryBuilder = fn(
    Vec<String>,           // Library name: ["scheme", "base"]
    Rc<Environment>,       // Library environment to populate
    &PrimitiveRegistry,    // Access to all registered primitives
) -> Vec<String>;          // Export list
```

**Usage:**
```rust
pub fn build_scheme_base(
    name: Vec<String>,
    env: Rc<Environment>,
    registry: &PrimitiveRegistry,
) -> Vec<String> {
    // Get all primitives for this library from registry
    let primitives = registry.get_library_primitives("scheme.base");

    // Install them into library environment
    for prim in primitives {
        env.define(
            prim.name.to_string(),
            Value::Procedure(Procedure::Primitive {
                library: prim.library,
                name: prim.name,
                arity: prim.arity.clone(),
            }),
        );
    }

    // Include Scheme supplements (derived functions)
    let scheme_code = include_str!("../../../../lib/scheme/base-extras.scm");
    evaluate_in_library_env(scheme_code, &env);

    // Return combined exports: primitives + Scheme definitions
    let mut exports: Vec<String> = primitives
        .iter()
        .map(|p| p.name.to_string())
        .collect();

    exports.extend(vec![
        "caar".to_string(),
        "cadr".to_string(),
        "not".to_string(),
        // ... other Scheme-defined exports
    ]);

    exports
}
```

**Benefits:**
- ✅ Single source of truth (registry)
- ✅ Automatic discovery of primitives
- ✅ Supports mixed Rust + Scheme
- ✅ Easy to maintain

### 3.2 Namespace-Aware Primitives

**Updated Value::Primitive:**
```rust
pub enum Procedure {
    Primitive {
        library: &'static str,  // NEW: "scheme.base", "scheme.char", etc.
        name: &'static str,
        arity: Arity,
    },
    Lambda { /* ... */ },
}
```

**Updated apply_primitive:**
```rust
pub(super) fn apply_primitive(
    &self,
    proc: &Procedure,
    args: Vec<Value>,
    in_tail: bool,
) -> Result<EvalResult, EvalError> {
    match proc {
        Procedure::Primitive { library, name, .. } => {
            // Use stored namespace, not hardcoded "scheme.base"
            let qualified = format!("{}/{}", library, name);
            self.primitive_registry.apply(&qualified, args, self, in_tail)
        }
        // ...
    }
}
```

**Benefits:**
- ✅ Primitives from any library work correctly
- ✅ No hardcoded namespace assumptions
- ✅ Registry lookup matches primitive definition

### 3.3 Minimal Bootstrap

**Goal:** Remove or minimize `lib/bootstrap.scm`

**Current bootstrap.scm contains:**
- Macros: `let`, `let*`, `letrec`, `cond`, `case`, `and`, `or`, `do`
- Derived: `caar`, `cadr`, `cadar`, `not`, etc.

**Migration:**
- **Macros:** Move to `lib/scheme/base-extras.scm` (loaded by `build_scheme_base`)
- **Derived functions:** Move to `lib/scheme/base-extras.scm`
- **Bootstrap:** Keep only absolute essentials (likely nothing)

**New bootstrap.scm:**
```scheme
;; Bootstrap for library system
;; Most functionality moved to (scheme base)

;; Currently empty - library system is self-sufficient
```

**Initialization sequence:**
```rust
impl Evaluator {
    pub fn new() -> Self {
        // 1. Create global environment
        let global_env = Rc::new(Environment::new());

        // 2. Create registries
        let primitive_registry = primitives::PrimitiveRegistry::new();
        let special_form_registry = special_forms::build_registry();

        // 3. Initialize library loaders
        self.init_loaders();

        // 4. Load minimal bootstrap (if any)
        self.load_bootstrap();  // Might be empty now!

        // 5. Import (scheme base) into global env for REPL convenience
        self.import_into_global("scheme", "base");

        // Done - no more bootstrap needed!
    }
}
```

### 3.4 Directory Structure

```
lib/
├── bootstrap.scm                # Minimal or empty
└── scheme/
    ├── base-extras.scm         # Scheme supplements for (scheme base)
    ├── char-extras.scm         # Scheme supplements for (scheme char)
    ├── lazy.scm                # Pure Scheme for (scheme lazy)
    ├── eval.scm                # Scheme helpers for (scheme eval)
    └── ...

crates/patina-runtime/src/stdlib/
├── mod.rs                      # Re-export all builders
├── scheme_base.rs              # build_scheme_base()
├── scheme_char.rs              # build_scheme_char()
├── scheme_lazy.rs              # build_scheme_lazy()
├── scheme_eval.rs              # build_scheme_eval()
├── scheme_case_lambda.rs       # build_scheme_case_lambda()
├── scheme_process_context.rs  # build_scheme_process_context()
└── ...

crates/patina-tree-walker/src/eval/primitives/
├── registry.rs                 # PrimitiveRegistry
├── arithmetic.rs               # Register with "scheme.base"
├── lists.rs                    # Register with "scheme.base"
├── strings.rs                  # Register with "scheme.base"
├── chars.rs                    # Register with "scheme.char"
├── lazy.rs                     # Register with "scheme.lazy"
└── ...
```

---

## 4. Implementation Phases

### Phase 1: Decouple Bootstrap (4-6 hours)

**Objective:** Move bootstrap content to libraries, minimize bootstrap file

**Tasks:**
1. **Audit `bootstrap.scm`** - categorize all content:
   ```
   Macros:
   - let, let*, letrec, letrec* → Move to base-extras.scm
   - cond, case → Move to base-extras.scm
   - and, or, do → Move to base-extras.scm

   Derived Functions:
   - caar, cadr, caddr, ... → Move to base-extras.scm
   - not, list, append, ... → Move to base-extras.scm (if not primitives)
   ```

2. **Create `lib/scheme/base-extras.scm`:**
   ```scheme
   ;; Scheme supplements for (scheme base)
   ;; Loaded by build_scheme_base()

   ;; Derived list accessors
   (define (caar x) (car (car x)))
   (define (cadr x) (car (cdr x)))
   ;; ... etc

   ;; Boolean utilities
   (define (not x) (if x #f #t))

   ;; Macros
   (define-syntax let
     (syntax-rules ()
       ;; ... existing definition
     ))
   ;; ... other macros
   ```

3. **Update `build_scheme_base`** to include extras:
   ```rust
   pub fn build_scheme_base(
       name: Vec<String>,
       env: Rc<Environment>,
       registry: &PrimitiveRegistry,
   ) -> Vec<String> {
       // Install primitives from registry
       let mut exports = install_library_primitives(env, registry, "scheme.base");

       // Load Scheme supplements
       let extras = include_str!("../../../../lib/scheme/base-extras.scm");
       evaluate_scheme_in_library(extras, &env)?;

       // Add Scheme-defined exports
       exports.extend(vec![
           "caar", "cadr", "let", "let*", "cond", "case", "not", // ...
       ]);

       exports
   }
   ```

4. **Minimize `bootstrap.scm`:**
   ```scheme
   ;; Bootstrap - minimal or empty
   ;; All functionality moved to libraries
   ```

5. **Update `Evaluator::load_bootstrap()`:**
   ```rust
   fn load_bootstrap(&self) {
       // Option A: Load minimal bootstrap (if anything remains)
       const BOOTSTRAP: &str = include_str!("../../../../lib/bootstrap.scm");
       if !BOOTSTRAP.trim().is_empty() {
           // Parse and evaluate
       }

       // Option B: Skip entirely if bootstrap is empty
       // (nothing to do!)
   }
   ```

6. **Import `(scheme base)` into global environment:**
   ```rust
   fn import_into_global(&self, lib_parts: &[&str]) {
       let lib_name: Vec<String> = lib_parts.iter()
           .map(|s| s.to_string())
           .collect();

       let library = self.load_library(&lib_name)?;

       // Import all exports into global environment
       for (name, value) in &library.exports {
           self.global_env.define(name.clone(), value.clone());
       }
   }

   // In Evaluator::new():
   self.import_into_global(&["scheme", "base"]);
   ```

**Testing:**
- Run all existing tests
- Verify macros work (`let`, `cond`, etc.)
- Verify derived functions work (`caar`, `not`, etc.)
- REPL should have `(scheme base)` available immediately

**What breaks:**
- Code expecting bootstrap macros before library loading
- Fix: Library loading now happens before bootstrap, so macros available earlier

**Rollback plan:**
- Keep old `bootstrap.scm` as `bootstrap.scm.bak`
- Can revert `load_bootstrap()` if issues arise

---

### Phase 2: Registry-Aware Builders (6-8 hours)

**Objective:** Eliminate duplicate primitive lists

**Tasks:**

1. **Update `RustLibraryBuilder` signature:**
   ```rust
   // In patina-runtime/src/library_loader.rs

   pub type RustLibraryBuilder = fn(
       Vec<String>,           // Library name
       Rc<Environment>,       // Library environment
       &PrimitiveRegistry,    // NEW: Access to registry
   ) -> Vec<String>;          // Export list
   ```

2. **Update `RustLibraryLoader::load()`:**
   ```rust
   // In patina-runtime/src/rust_library_loader.rs

   impl LibraryLoader for RustLibraryLoader {
       fn load(&self, name: &[String], search_paths: &[PathBuf])
           -> Result<Library, LibraryError>
       {
           let builder = self.builders.get(name)?;
           let env = Rc::new(Environment::new());

           // NEW: Get registry reference from evaluator context
           // (Requires passing evaluator or registry to loader)
           let exports = builder(name.to_vec(), env.clone(), registry);

           // Create library with exports
           let mut lib = Library::with_env(name.to_vec(), env);
           for export_name in exports {
               if let Some(value) = env.get(&export_name) {
                   lib.export(export_name, value);
               }
           }

           Ok(lib)
       }
   }
   ```

3. **Problem: How does loader access registry?**

   **Solution: Pass registry through initialization:**
   ```rust
   // In patina-tree-walker/src/eval/mod.rs

   fn init_loaders(&self) {
       let mut rust_loader = RustLibraryLoader::new();

       // Register libraries
       rust_loader.register(
           vec!["scheme".into(), "base".into()],
           stdlib::build_scheme_base,
       );
       // ... more libraries

       // Add loader to registry
       let mut loaders = self.loader_registry.borrow_mut();
       loaders.add_loader_with_registry(
           Box::new(rust_loader),
           &self.primitive_registry,  // Pass registry reference
       );
   }
   ```

   **Alternative: Make registry part of evaluator context available to loaders**

4. **Create helper function:**
   ```rust
   // In patina-runtime/src/rust_library_loader.rs

   /// Install all primitives for a library from the registry
   pub fn install_library_primitives(
       env: &Rc<Environment>,
       registry: &PrimitiveRegistry,
       library_namespace: &str,
   ) -> Vec<String> {
       let primitives = registry.get_library_primitives(library_namespace);
       let mut exports = Vec::new();

       for prim in primitives {
           env.define(
               prim.name.to_string(),
               Value::Procedure(Procedure::Primitive {
                   library: prim.library,
                   name: prim.name,
                   arity: prim.arity.clone(),
               }),
           );
           exports.push(prim.name.to_string());
       }

       exports
   }
   ```

5. **Refactor `build_scheme_base`:**
   ```rust
   pub fn build_scheme_base(
       _name: Vec<String>,
       env: Rc<Environment>,
       registry: &PrimitiveRegistry,
   ) -> Vec<String> {
       // Install primitives from registry (automatic!)
       let mut exports = install_library_primitives(&env, registry, "scheme.base");

       // Load Scheme supplements
       let extras = include_str!("../../../../lib/scheme/base-extras.scm");
       evaluate_scheme_in_library(extras, &env)
           .expect("Failed to load base-extras.scm");

       // Add Scheme-defined exports
       exports.extend(vec![
           // Derived functions
           "caar", "cadr", "cdar", "cddr",
           "caaar", "caadr", /* ... */

           // Utilities
           "not",

           // Macros
           "let", "let*", "letrec", "letrec*",
           "cond", "case", "and", "or", "do",

           // ... any other Scheme-defined exports
       ]);

       exports
   }
   ```

6. **Remove manual primitive lists:**
   - Delete all hardcoded primitive arrays from `scheme_base.rs`
   - Delete from `scheme_char.rs`, `scheme_complex.rs`, etc.
   - Verify via code review

7. **Update other library builders:**
   - `build_scheme_char` - use `install_library_primitives` for `"scheme.char"`
   - `build_scheme_complex` - use `install_library_primitives` for `"scheme.complex"`
   - Continue for all standard libraries

**Testing:**
- Run all library loading tests
- Verify all exports present in libraries
- Check that primitives from different libraries work
- Test library introspection (list exports, check membership)

**What breaks:**
- Library loader initialization (needs registry access)
- Fix: Update `init_loaders()` to pass registry

**Rollback plan:**
- Keep old builder implementations commented out
- Can switch back to manual lists if issues

---

### Phase 3: Namespace-Aware Primitives (4-6 hours)

**Objective:** Support primitives from all libraries, not just `scheme.base`

**Tasks:**

1. **Update `Procedure::Primitive` to store library:**
   ```rust
   // In patina-runtime/src/value/mod.rs

   pub enum Procedure {
       Primitive {
           library: &'static str,  // NEW: "scheme.base", "scheme.char", etc.
           name: &'static str,
           arity: Arity,
       },
       Lambda {
           params: Vec<String>,
           variadic: Option<String>,
           body: Vec<Value>,
           env: Rc<Environment>,
       },
   }
   ```

2. **Update all code that creates `Procedure::Primitive`:**

   In `install_library_primitives`:
   ```rust
   Value::Procedure(Procedure::Primitive {
       library: prim.library,  // From registry
       name: prim.name,
       arity: prim.arity.clone(),
   })
   ```

   Search codebase for `Procedure::Primitive` creations:
   ```bash
   rg "Procedure::Primitive" --type rust
   ```

   Update each occurrence to include `library` field.

3. **Update `apply_primitive` to use stored library:**
   ```rust
   // In patina-tree-walker/src/eval/application.rs

   pub(super) fn apply_primitive(
       &self,
       proc: &Procedure,
       args: Vec<Value>,
       in_tail: bool,
   ) -> Result<EvalResult, EvalError> {
       let Procedure::Primitive { library, name, .. } = proc else {
           panic!("apply_primitive called on non-primitive");
       };

       // Build qualified name using stored library
       let qualified = format!("{}/{}", library, name);

       // Look up in registry
       self.primitive_registry.apply(&qualified, args, self, in_tail)
   }
   ```

4. **Remove hardcoded `"scheme.base/"` prefix:**
   ```rust
   // OLD (in apply_primitive):
   let qualified = format!("scheme.base/{}", name);  // ❌ Hardcoded!

   // NEW:
   let qualified = format!("{}/{}", library, name);  // ✅ From primitive itself
   ```

5. **Update primitive creation in tests:**
   ```rust
   // Tests that create primitives directly
   Value::Procedure(Procedure::Primitive {
       library: "scheme.base",  // Specify library
       name: "car",
       arity: Arity::Exactly(1),
   })
   ```

**Testing:**
- Test primitives from `(scheme char)` work
- Test primitives from `(scheme complex)` work
- Test primitives from `(scheme base)` still work
- Test error messages show correct library namespace
- Run full test suite

**What breaks:**
- All code creating `Procedure::Primitive` values
- Pattern matches on `Procedure::Primitive`
- Fix: Add `library` field to all occurrences

**Rollback plan:**
- Can temporarily make `library` optional with default `"scheme.base"`
- Migrate incrementally

---

### Phase 4: Special Libraries (8-12 hours)

**Objective:** Implement `(scheme lazy)`, `(scheme eval)`, `(scheme case-lambda)`, `(scheme process-context)`

#### 4.1 `(scheme lazy)` - 2-3 hours

**Implementation:**

1. **Create special form for `delay`:**
   ```rust
   // In crates/patina-tree-walker/src/eval/special_forms/delay.rs

   pub struct DelayForm;

   impl SpecialForm for DelayForm {
       fn name(&self) -> &str { "delay" }

       fn help(&self) -> &str {
           "(delay <expression>) - Create a promise without evaluating expression"
       }

       fn eval(&self, _eval: &Evaluator, args: &Value, env: &Rc<Environment>, _tail: bool)
           -> Result<EvalResult, EvalError>
       {
           // delay takes exactly one argument, unevaluated
           let expr = expect_single_arg(args, "delay")?;

           // Create a promise (thunk)
           let promise = Value::Promise(Promise {
               state: RefCell::new(PromiseState::Delayed {
                   expr: expr.clone(),
                   env: env.clone(),
               }),
           });

           Ok(EvalResult::Value(promise))
       }
   }
   ```

2. **Add `Promise` to `Value` enum:**
   ```rust
   // In patina-runtime/src/value/mod.rs

   pub enum Value {
       // ... existing variants
       Promise(Rc<Promise>),
   }

   pub struct Promise {
       state: RefCell<PromiseState>,
   }

   pub enum PromiseState {
       Delayed {
           expr: Value,
           env: Rc<Environment>,
       },
       Forced(Value),
   }
   ```

3. **Implement `force` and `promise?` primitives:**
   ```rust
   // In crates/patina-tree-walker/src/eval/primitives/lazy.rs

   pub fn force(
       eval: &Evaluator,
       args: Vec<Value>,
       _tail: bool,
   ) -> Result<EvalResult, EvalError> {
       let promise = expect_single_arg(&args, "force")?;

       match promise {
           Value::Promise(p) => {
               let mut state = p.state.borrow_mut();
               match &*state {
                   PromiseState::Delayed { expr, env } => {
                       // Evaluate the delayed expression
                       let result = eval.eval_in_env(expr, env)?;

                       // Memoize the result
                       *state = PromiseState::Forced(result.clone());

                       Ok(EvalResult::Value(result))
                   }
                   PromiseState::Forced(value) => {
                       Ok(EvalResult::Value(value.clone()))
                   }
               }
           }
           other => {
               // force on non-promise returns the value
               Ok(EvalResult::Value(other))
           }
       }
   }

   pub fn promise_p(args: Vec<Value>) -> Result<Value, EvalError> {
       let arg = expect_single_arg(&args, "promise?")?;
       Ok(Value::Boolean(matches!(arg, Value::Promise(_))))
   }

   pub fn register_lazy_primitives(registry: &mut PrimitiveRegistry) {
       registry.register(PrimitiveFn {
           library: "scheme.lazy",
           name: "force",
           arity: Arity::Exactly(1),
           help: "(force promise) - Force evaluation of a delayed promise",
           handler: PrimitiveHandler::WithEvaluator(force),
       });

       registry.register(PrimitiveFn {
           library: "scheme.lazy",
           name: "promise?",
           arity: Arity::Exactly(1),
           help: "(promise? obj) - Test if obj is a promise",
           handler: PrimitiveHandler::Simple(promise_p),
       });
   }
   ```

4. **Create library builder:**
   ```rust
   // In crates/patina-runtime/src/stdlib/scheme_lazy.rs

   pub fn build_scheme_lazy(
       _name: Vec<String>,
       env: Rc<Environment>,
       registry: &PrimitiveRegistry,
   ) -> Vec<String> {
       // Install primitives from registry
       let mut exports = install_library_primitives(&env, registry, "scheme.lazy");

       // delay is a special form, manually add
       env.define(
           "delay".to_string(),
           Value::SpecialForm("delay"),  // Reference to special form
       );
       exports.push("delay".to_string());

       // Optional: Load Scheme supplements
       // let extras = include_str!("../../../../lib/scheme/lazy.scm");
       // evaluate_scheme_in_library(extras, &env)?;

       exports
   }
   ```

5. **Register in evaluator:**
   ```rust
   // In init_loaders():
   rust_loader.register(
       vec!["scheme".into(), "lazy".into()],
       stdlib::build_scheme_lazy,
   );
   ```

**Testing:**
```scheme
(import (scheme lazy))

(define p (delay (+ 1 2)))
(promise? p)  ; => #t
(force p)     ; => 3
(force p)     ; => 3 (memoized)
```

#### 4.2 `(scheme eval)` - 2-3 hours

**Implementation:**

1. **Implement `eval` primitive:**
   ```rust
   // In crates/patina-tree-walker/src/eval/primitives/eval.rs

   pub fn eval_primitive(
       evaluator: &Evaluator,
       args: Vec<Value>,
       _tail: bool,
   ) -> Result<EvalResult, EvalError> {
       expect_arg_count(&args, 2, "eval")?;
       let expr = &args[0];
       let env_spec = &args[1];

       // Construct environment from specification
       let env = construct_environment(evaluator, env_spec)?;

       // Evaluate expression in that environment
       evaluator.eval_in_env(expr, &env).map(EvalResult::Value)
   }

   fn construct_environment(
       evaluator: &Evaluator,
       spec: &Value,
   ) -> Result<Rc<Environment>, EvalError> {
       // spec can be:
       // - (environment (import-set ...))
       // - A library name list

       match spec {
           Value::Pair(_) => {
               // Parse as (environment ...) form
               // Load specified libraries and create environment
               let env = Rc::new(Environment::new());

               // Process import sets
               // (Similar to library import processing)

               Ok(env)
           }
           _ => Err(EvalError::TypeError(
               "eval: environment spec must be a list".to_string()
           )),
       }
   }

   pub fn register_eval_primitives(registry: &mut PrimitiveRegistry) {
       registry.register(PrimitiveFn {
           library: "scheme.eval",
           name: "eval",
           arity: Arity::Exactly(2),
           help: "(eval expr env) - Evaluate expression in environment",
           handler: PrimitiveHandler::WithEvaluator(eval_primitive),
       });
   }
   ```

2. **Implement `environment` primitive:**
   ```rust
   pub fn environment_primitive(
       evaluator: &Evaluator,
       args: Vec<Value>,
       _tail: bool,
   ) -> Result<EvalResult, EvalError> {
       // (environment import-set ...)
       // Returns an environment constructed from import sets

       let env = Rc::new(Environment::new());

       for import_set in args {
           // Process each import set
           process_import_set(evaluator, &import_set, &env)?;
       }

       Ok(EvalResult::Value(Value::Environment(env)))
   }
   ```

3. **Add `Environment` variant to `Value`:**
   ```rust
   pub enum Value {
       // ... existing variants
       Environment(Rc<Environment>),
   }
   ```

**Testing:**
```scheme
(import (scheme base) (scheme eval))

(define env (environment '(scheme base)))
(eval '(+ 1 2) env)  ; => 3

(eval '(cons 1 2) (environment '(scheme base)))  ; => (1 . 2)
```

#### 4.3 `(scheme case-lambda)` - 2-3 hours

**Implementation:**

1. **Create `case-lambda` special form:**
   ```rust
   // In crates/patina-tree-walker/src/eval/special_forms/case_lambda.rs

   pub struct CaseLambdaForm;

   impl SpecialForm for CaseLambdaForm {
       fn name(&self) -> &str { "case-lambda" }

       fn help(&self) -> &str {
           "(case-lambda clause ...) - Create multi-arity procedure"
       }

       fn eval(&self, _eval: &Evaluator, args: &Value, env: &Rc<Environment>, _tail: bool)
           -> Result<EvalResult, EvalError>
       {
           // (case-lambda
           //   [(args1) body1 ...]
           //   [(args2) body2 ...]
           //   ...)

           let clauses = parse_case_lambda_clauses(args)?;

           let proc = Value::Procedure(Procedure::CaseLambda {
               clauses: clauses,
               env: env.clone(),
           });

           Ok(EvalResult::Value(proc))
       }
   }

   #[derive(Clone)]
   pub struct CaseLambdaClause {
       pub params: Vec<String>,
       pub variadic: Option<String>,
       pub body: Vec<Value>,
   }
   ```

2. **Add `CaseLambda` variant to `Procedure`:**
   ```rust
   pub enum Procedure {
       Primitive { /* ... */ },
       Lambda { /* ... */ },
       CaseLambda {
           clauses: Vec<CaseLambdaClause>,
           env: Rc<Environment>,
       },
   }
   ```

3. **Update procedure application:**
   ```rust
   // In application.rs

   fn apply_case_lambda(
       &self,
       clauses: &[CaseLambdaClause],
       closure_env: &Rc<Environment>,
       args: Vec<Value>,
   ) -> Result<EvalResult, EvalError> {
       // Try each clause in order
       for clause in clauses {
           if matches_arity(clause, args.len()) {
               // Bind parameters and evaluate body
               let local_env = Rc::new(Environment::with_parent(closure_env.clone()));
               bind_parameters(clause, &args, &local_env)?;

               return self.eval_sequence(&clause.body, &local_env, true);
           }
       }

       Err(EvalError::ArityMismatch(format!(
           "case-lambda: no clause matches {} arguments",
           args.len()
       )))
   }
   ```

**Testing:**
```scheme
(import (scheme base) (scheme case-lambda))

(define plus
  (case-lambda
    [() 0]
    [(x) x]
    [(x y) (+ x y)]
    [(x y . rest) (apply + x y rest)]))

(plus)        ; => 0
(plus 5)      ; => 5
(plus 2 3)    ; => 5
(plus 1 2 3)  ; => 6
```

#### 4.4 `(scheme process-context)` - 2-3 hours

**Implementation:**

1. **Implement OS primitives:**
   ```rust
   // In crates/patina-tree-walker/src/eval/primitives/process_context.rs

   use std::env;

   pub fn command_line(_args: Vec<Value>) -> Result<Value, EvalError> {
       let args: Vec<Value> = env::args()
           .map(|s| Value::String(Rc::new(RefCell::new(s))))
           .collect();

       Ok(list_to_value(args))
   }

   pub fn exit(args: Vec<Value>) -> Result<Value, EvalError> {
       let code = if args.is_empty() {
           0
       } else {
           expect_integer(&args[0], "exit")? as i32
       };

       std::process::exit(code);
   }

   pub fn get_environment_variable(args: Vec<Value>) -> Result<Value, EvalError> {
       let name = expect_string(&args[0], "get-environment-variable")?;

       match env::var(name) {
           Ok(value) => Ok(Value::String(Rc::new(RefCell::new(value)))),
           Err(_) => Ok(Value::Boolean(false)),
       }
   }

   pub fn get_environment_variables(_args: Vec<Value>) -> Result<Value, EvalError> {
       let vars: Vec<Value> = env::vars()
           .map(|(k, v)| {
               // Return as pairs: (name . value)
               Value::Pair(Rc::new(Pair {
                   car: Value::String(Rc::new(RefCell::new(k))),
                   cdr: Value::String(Rc::new(RefCell::new(v))),
               }))
           })
           .collect();

       Ok(list_to_value(vars))
   }

   pub fn register_process_context_primitives(registry: &mut PrimitiveRegistry) {
       registry.register(PrimitiveFn {
           library: "scheme.process-context",
           name: "command-line",
           arity: Arity::Exactly(0),
           help: "(command-line) - Get command-line arguments as list",
           handler: PrimitiveHandler::Simple(command_line),
       });

       registry.register(PrimitiveFn {
           library: "scheme.process-context",
           name: "exit",
           arity: Arity::Range(0, 1),
           help: "(exit [code]) - Exit program with optional code",
           handler: PrimitiveHandler::Simple(exit),
       });

       registry.register(PrimitiveFn {
           library: "scheme.process-context",
           name: "get-environment-variable",
           arity: Arity::Exactly(1),
           help: "(get-environment-variable name) - Get environment variable",
           handler: PrimitiveHandler::Simple(get_environment_variable),
       });

       registry.register(PrimitiveFn {
           library: "scheme.process-context",
           name: "get-environment-variables",
           arity: Arity::Exactly(0),
           help: "(get-environment-variables) - Get all environment variables",
           handler: PrimitiveHandler::Simple(get_environment_variables),
       });
   }
   ```

2. **Create library builder:**
   ```rust
   pub fn build_scheme_process_context(
       _name: Vec<String>,
       env: Rc<Environment>,
       registry: &PrimitiveRegistry,
   ) -> Vec<String> {
       // All primitives, no Scheme supplements needed
       install_library_primitives(&env, registry, "scheme.process-context")
   }
   ```

**Testing:**
```scheme
(import (scheme base) (scheme process-context))

(command-line)  ; => ("patina" ...)
(get-environment-variable "PATH")  ; => "/usr/bin:..."
(get-environment-variables)  ; => (("PATH" . "...") ...)
```

---

## 5. Mixed Library Pattern

### 5.1 Pattern Overview

**Mixed libraries combine:**
- **Rust primitives** - Performance-critical operations
- **Scheme code** - Derived functions, utilities, syntactic sugar

**Benefits:**
- ⚡ Performance where it matters (Rust)
- 🚀 Rapid development where it doesn't (Scheme)
- 📦 Single coherent library
- 🎯 Clear separation of concerns

### 5.2 Example: `(scheme base)`

**Structure:**
```
crates/patina-tree-walker/src/eval/primitives/
├── arithmetic.rs     # +, -, *, / (Rust - performance)
├── lists.rs          # cons, car, cdr (Rust - fundamental)
└── ...

lib/scheme/
└── base-extras.scm   # Derived functions (Scheme - convenience)
```

**Rust primitives (performance-critical):**
```rust
// In primitives/arithmetic.rs
pub fn add(args: Vec<Value>) -> Result<Value, EvalError> {
    // Fast numeric addition with type coercion
    // Handles integers, rationals, reals, complex
}

pub fn cons(args: Vec<Value>) -> Result<Value, EvalError> {
    // Fundamental list construction
}
```

**Scheme supplements (derived convenience):**
```scheme
;; In lib/scheme/base-extras.scm

;; Derived list accessors (built on car/cdr primitives)
(define (caar x) (car (car x)))
(define (cadr x) (car (cdr x)))
(define (cdar x) (cdr (car x)))
(define (cddr x) (cdr (cdr x)))

;; Higher-order list utilities
(define (fold-right f init lst)
  (if (null? lst)
      init
      (f (car lst)
         (fold-right f init (cdr lst)))))

;; Boolean utilities
(define (not x) (if x #f #t))

;; Macros for control flow
(define-syntax let
  (syntax-rules ()
    [(let () body ...)
     ((lambda () body ...))]
    [(let ((var val) ...) body ...)
     ((lambda (var ...) body ...) val ...)]))
```

**Library builder combines both:**
```rust
pub fn build_scheme_base(
    _name: Vec<String>,
    env: Rc<Environment>,
    registry: &PrimitiveRegistry,
) -> Vec<String> {
    // 1. Install Rust primitives from registry
    let mut exports = install_library_primitives(&env, registry, "scheme.base");

    // 2. Load Scheme supplements
    let extras = include_str!("../../../../lib/scheme/base-extras.scm");
    evaluate_scheme_in_library(extras, &env)
        .expect("Failed to load base-extras.scm");

    // 3. Add Scheme-defined exports
    exports.extend(vec![
        // Derived accessors
        "caar", "cadr", "cdar", "cddr",
        "caaar", "caadr", /* ... */

        // Utilities
        "not", "fold-right", "fold-left",

        // Macros
        "let", "let*", "letrec", "cond", "case",
    ]);

    exports
}
```

### 5.3 Example: User-Defined Trait Library

**Use case:** Implementing a trait system with Rust performance + Scheme expressiveness

**Rust side (performance):**
```rust
// In crates/patina-tree-walker/src/eval/primitives/traits.rs

pub fn make_trait(args: Vec<Value>) -> Result<Value, EvalError> {
    // Create trait object with methods map
    let name = expect_symbol(&args[0], "make-trait")?;
    let methods = parse_method_specs(&args[1])?;

    Ok(Value::Trait(Trait {
        name: name.to_string(),
        methods: Rc::new(RefCell::new(methods)),
        instances: Rc::new(RefCell::new(HashMap::new())),
    }))
}

pub fn implement_trait(args: Vec<Value>) -> Result<Value, EvalError> {
    // Register trait implementation for a type
    // Fast HashMap lookup for dispatch
}

pub fn trait_call(eval: &Evaluator, args: Vec<Value>) -> Result<EvalResult, EvalError> {
    // Dispatch to trait method implementation
    // Performance-critical: uses HashMap for O(1) lookup
}

pub fn register_trait_primitives(registry: &mut PrimitiveRegistry) {
    registry.register(PrimitiveFn {
        library: "patina.traits",  // User library namespace
        name: "make-trait",
        arity: Arity::Exactly(2),
        help: "(make-trait name methods) - Define a new trait",
        handler: PrimitiveHandler::Simple(make_trait),
    });

    registry.register(PrimitiveFn {
        library: "patina.traits",
        name: "implement!",
        arity: Arity::Exactly(3),
        help: "(implement! trait type impl) - Implement trait for type",
        handler: PrimitiveHandler::Simple(implement_trait),
    });

    registry.register(PrimitiveFn {
        library: "patina.traits",
        name: "trait-call",
        arity: Arity::Min(2),
        help: "(trait-call trait method obj ...) - Call trait method",
        handler: PrimitiveHandler::WithEvaluator(trait_call),
    });
}
```

**Scheme side (expressiveness):**
```scheme
;; In lib/patina/traits.scm

;; Syntactic sugar for trait definition
(define-syntax define-trait
  (syntax-rules ()
    [(define-trait name (method ...))
     (define name (make-trait 'name '(method ...)))]))

;; Syntactic sugar for implementation
(define-syntax implement
  (syntax-rules ()
    [(implement trait-name for type-pred
       [(method args ...) body ...] ...)
     (implement! trait-name
                 type-pred
                 (lambda (method obj)
                   (case method
                     [(method) (lambda (args ...) body ...)]
                     ...
                     [else (error "Method not found")])))]))

;; Generic dispatch macro
(define-syntax generic
  (syntax-rules ()
    [(generic method obj args ...)
     (let ([trait-impl (trait-call current-trait 'method obj)])
       (trait-impl obj args ...))]))

;; Example utility: derive default implementations
(define (derive-debug-trait obj)
  (implement! Debug-trait
              (lambda (x) #t)  ; Matches any type
              (lambda (method obj)
                (case method
                  [(debug) (lambda () (display obj))]))))

;; Trait composition
(define (compose-traits . traits)
  (make-trait 'composed
              (apply append (map trait-methods traits))))
```

**Library builder:**
```rust
pub fn build_patina_traits(
    _name: Vec<String>,
    env: Rc<Environment>,
    registry: &PrimitiveRegistry,
) -> Vec<String> {
    // Install Rust primitives (performance-critical)
    let mut exports = install_library_primitives(&env, registry, "patina.traits");

    // Load Scheme utilities (syntactic sugar and conveniences)
    let scheme_code = include_str!("../../../../lib/patina/traits.scm");
    evaluate_scheme_in_library(scheme_code, &env)
        .expect("Failed to load traits.scm");

    // Export both Rust and Scheme definitions
    exports.extend(vec![
        // Scheme macros
        "define-trait",
        "implement",
        "generic",

        // Scheme utilities
        "derive-debug-trait",
        "compose-traits",
    ]);

    exports
}
```

**Usage:**
```scheme
(import (patina traits))

;; Define trait (uses macro -> expands to make-trait primitive)
(define-trait Show
  (show))

;; Implement trait (uses macro -> expands to implement! primitive)
(implement Show for integer?
  [(show n) (number->string n)])

(implement Show for pair?
  [(show p) (string-append "(" (generic show (car p)) " . " (generic show (cdr p)) ")")])

;; Use trait (trait-call primitive handles dispatch)
(define (display-any x)
  (display (generic show x)))

(display-any 42)        ; "42"
(display-any '(1 . 2))  ; "(1 . 2)"
```

### 5.4 Guidelines for Mixed Libraries

**When to use Rust:**
- Performance-critical operations (tight loops, numeric computation)
- Low-level operations (memory, I/O, OS interaction)
- Type-dependent behavior (requires Rust pattern matching)
- Core data structure operations

**When to use Scheme:**
- Derived functions (built on primitives)
- Syntactic sugar (macros)
- Higher-order utilities (map, fold, compose)
- Domain-specific logic
- Convenience wrappers

**Directory structure:**
```
lib/
└── <namespace>/
    ├── <library>.scm       # Scheme supplements
    └── tests/              # Library-specific tests
        └── <library>_test.scm

crates/patina-tree-walker/src/eval/primitives/
└── <library>.rs            # Rust primitives

crates/patina-runtime/src/stdlib/
└── <library>.rs            # Library builder
```

---

## 6. Special Library Support

### 6.1 Categories of Special Libraries

**Category 1: Special Evaluation Semantics**
- `(scheme lazy)` - Delayed evaluation
- `(scheme case-lambda)` - Multi-arity dispatch

**Strategy:** Special forms or primitive with evaluator access

**Category 2: Meta-Programming**
- `(scheme eval)` - Runtime evaluation
- `(scheme repl)` - REPL interaction (future)

**Strategy:** Primitives that access evaluator context

**Category 3: System Integration**
- `(scheme process-context)` - OS environment
- `(scheme file)` - File I/O
- `(scheme time)` - Time operations

**Strategy:** Standard primitives using Rust `std` library

**Category 4: Performance Alternatives**
- `(scheme inexact)` - Floating-point operations
- `(scheme complex)` - Complex numbers

**Strategy:** Primitives with specialized implementations

### 6.2 Implementation Checklist

For each special library:

1. **Identify requirements:**
   - [ ] Needs special evaluation? → Special form
   - [ ] Needs evaluator access? → Primitive with `WithEvaluator` handler
   - [ ] Needs OS/system access? → Primitive using `std`
   - [ ] Just exports existing forms? → Library builder only

2. **Implement in appropriate layer:**
   - [ ] Special forms in `special_forms/`
   - [ ] Primitives in `primitives/`
   - [ ] Scheme supplements in `lib/scheme/`
   - [ ] Library builder in `stdlib/`

3. **Register components:**
   - [ ] Special form in `SpecialFormRegistry`
   - [ ] Primitives in `PrimitiveRegistry` with correct namespace
   - [ ] Library in `RustLibraryLoader`

4. **Test thoroughly:**
   - [ ] Unit tests for primitives
   - [ ] Integration tests for library
   - [ ] R7RS compliance tests
   - [ ] Example usage in REPL

---

## 7. Migration Strategy

### 7.1 Risk Mitigation

**Low-risk changes first:**
1. Phase 1 (Bootstrap) - Most isolated, can revert easily
2. Phase 2 (Registry) - Additive, old code paths remain
3. Phase 3 (Namespace) - Touches many files, but type-safe
4. Phase 4 (Special libs) - New functionality, doesn't break existing

**Continuous testing:**
- Run full test suite after each commit
- Keep CI green throughout migration
- Tag releases at stable milestones

**Rollback points:**
```
v0.1.0 - Current state (pre-migration)
v0.1.1 - Phase 1 complete (bootstrap minimal)
v0.1.2 - Phase 2 complete (registry-aware)
v0.1.3 - Phase 3 complete (namespace-aware)
v0.2.0 - Phase 4 complete (special libraries)
```

### 7.2 Incremental Rollout

**Phase 1 (Bootstrap):**
- Create `lib/scheme/base-extras.scm` (additive)
- Update `build_scheme_base` to include it (backward compatible)
- Minimize `bootstrap.scm` (tests pass with either version)
- Only remove bootstrap when confident

**Phase 2 (Registry):**
- Add `&PrimitiveRegistry` parameter (old signature still compiles if unused)
- Update builders one at a time (can mix old and new)
- Remove old code only when all builders migrated

**Phase 3 (Namespace):**
- Add `library` field to `Procedure::Primitive`
- Update creation sites incrementally
- Use default value for unmigrated code
- Remove default when all sites migrated

**Phase 4 (Special libs):**
- Each library is independent
- Implement one at a time
- No dependencies between them
- Can ship partial set

### 7.3 Testing Strategy

**Test levels:**

1. **Unit tests** - Test individual components
   - Primitive functions
   - Special forms
   - Library builders

2. **Integration tests** - Test library loading
   - Load each library
   - Verify exports
   - Test primitive calls

3. **Compliance tests** - R7RS test suite
   - Run chibi-scheme tests
   - Track pass rate improvement

4. **Manual testing** - REPL usage
   - Import libraries
   - Use mixed Rust/Scheme features
   - Test special libraries

**Test coverage targets:**
- Phase 1: All existing tests pass
- Phase 2: All existing tests + library introspection tests pass
- Phase 3: All existing tests + namespace tests pass
- Phase 4: All existing tests + special library tests pass

---

## 8. Decision Points

### 8.1 Bootstrap Removal

**Decision:** Should `bootstrap.scm` be completely removed?

**Option A: Complete Removal (Recommended)**
- Cleanest architecture
- Forces proper library usage
- R7RS compliant

**Option B: Minimal Bootstrap**
- Keep safety net for unforeseen issues
- Easier rollback if problems arise

**Recommendation:** Start with Option B (minimal), move to Option A when stable.

**Implementation:**
```scheme
;; bootstrap.scm - Phase 1 (minimal)
;; Contains fallbacks only

;; Phase 2 (eventually empty)
;; (empty file or removed entirely)
```

### 8.2 Registry Access Method

**Decision:** How should library builders access the primitive registry?

**Option A: Pass through loader (Recommended)**
```rust
// Loader receives registry reference
loaders.add_loader_with_registry(
    Box::new(rust_loader),
    &self.primitive_registry,
);

// Builder receives it
pub type RustLibraryBuilder = fn(
    Vec<String>,
    Rc<Environment>,
    &PrimitiveRegistry,  // From loader
) -> Vec<String>;
```

**Option B: Global registry singleton**
```rust
// Not recommended - breaks testability
static REGISTRY: OnceCell<PrimitiveRegistry> = OnceCell::new();
```

**Option C: Builder stores registry reference**
```rust
// Complex ownership
struct BuilderContext {
    registry: Rc<PrimitiveRegistry>,
}
```

**Recommendation:** Option A - explicit passing, clear ownership.

### 8.3 Scheme Evaluation in Libraries

**Decision:** How to evaluate Scheme code in library context?

**Option A: Include and evaluate at build time**
```rust
let code = include_str!("lib/scheme/base-extras.scm");
evaluate_scheme_in_library(code, &env)?;
```

**Option B: Load from filesystem at runtime**
```rust
let path = find_library_supplement("scheme", "base")?;
let code = fs::read_to_string(path)?;
evaluate_scheme_in_library(code, &env)?;
```

**Option C: Hybrid (include for distribution, filesystem for development)**
```rust
#[cfg(debug_assertions)]
let code = fs::read_to_string("lib/scheme/base-extras.scm")?;

#[cfg(not(debug_assertions))]
let code = include_str!("lib/scheme/base-extras.scm");
```

**Recommendation:** Option A for simplicity initially, Option C for best of both worlds.

### 8.4 Special Form Library Membership

**Decision:** Should special forms be library-scoped?

**Option A: Global special forms (Current)**
- Simpler implementation
- Special forms rarely conflict
- Not strictly R7RS but pragmatic

**Option B: Library-scoped special forms**
- `(scheme base)` exports `lambda`, `if`
- `(scheme case-lambda)` exports `case-lambda`
- More R7RS compliant

**Recommendation:** Option A initially, design for Option B:
- Keep global special form registry
- Allow libraries to "claim" special forms in metadata
- Can add true scoping later if needed

### 8.5 REPL Default Environment

**Decision:** What's available in REPL without imports?

**Option A: Auto-import (scheme base) (Recommended)**
```rust
// In Evaluator::new() for REPL mode
if is_repl_mode {
    self.import_into_global(&["scheme", "base"]);
}
```

**Option B: Empty environment**
```scheme
;; User must explicitly import
(import (scheme base))
```

**Recommendation:** Option A for REPL, Option B for script files.

**Configuration:**
```rust
pub struct EvaluatorConfig {
    pub auto_import_base: bool,  // true for REPL, false for scripts
}
```

---

## 9. Future Extensions

### 9.1 User-Defined Libraries

**Goal:** Allow users to create their own mixed Rust/Scheme libraries

**Pattern:**
```
my-project/
├── src/
│   └── lib.rs              # Rust primitives
├── scm/
│   └── my-lib.scm          # Scheme supplements
└── Cargo.toml
```

**Registration API:**
```rust
use patina_runtime::{PrimitiveRegistry, RustLibraryLoader};

pub fn register_my_library(
    registry: &mut PrimitiveRegistry,
    loader: &mut RustLibraryLoader,
) {
    // Register primitives
    register_my_primitives(registry);

    // Register library builder
    loader.register(
        vec!["my".into(), "awesome".into(), "lib".into()],
        build_my_library,
    );
}

pub fn build_my_library(
    _name: Vec<String>,
    env: Rc<Environment>,
    registry: &PrimitiveRegistry,
) -> Vec<String> {
    // Install primitives
    let mut exports = install_library_primitives(&env, registry, "my.awesome.lib");

    // Load Scheme code
    let scheme = include_str!("../scm/my-lib.scm");
    evaluate_scheme_in_library(scheme, &env).unwrap();

    exports.extend(vec!["my-macro", "my-utility"]);
    exports
}
```

**Documentation:** Provide template and guide in `docs/CUSTOM_LIBRARIES.md`

### 9.2 Pure `.sld` Libraries

**Goal:** Support standard R7RS library format

**Example `lib/scheme/base.sld`:**
```scheme
(define-library (scheme base)
  ;; Import Rust primitives (special directive)
  (import-primitives scheme.base)

  ;; Import other libraries
  (import (scheme char))  ; For char operations

  ;; Include Scheme definitions
  (include "base-extras.scm")

  ;; Export everything
  (export + - * / cons car cdr
          caar cadr let let* cond
          ...))
```

**Implementation:**
- Add `import-primitives` directive to library parser
- Extend `SchemeLibraryLoader` to handle directive
- Migrate library builders to `.sld` format gradually

**Timeline:** Post-Phase 4 (after mixed libraries proven)

### 9.3 Library Metadata and Introspection

**Goal:** Rich metadata for documentation and tooling

**Extended library structure:**
```rust
pub struct Library {
    pub name: Vec<String>,
    pub exports: HashMap<String, Value>,
    pub env: Rc<Environment>,
    pub source: Option<PathBuf>,

    // NEW: Metadata
    pub metadata: LibraryMetadata,
}

pub struct LibraryMetadata {
    pub version: String,
    pub description: String,
    pub author: String,
    pub dependencies: Vec<Vec<String>>,
    pub keywords: Vec<String>,
}
```

**Introspection API:**
```scheme
(library-exports '(scheme base))  ; => (+ - * cons ...)
(library-version '(scheme base))  ; => "R7RS-small"
(library-dependencies '(scheme base))  ; => ()
(library-search "string")  ; => ((scheme base) (srfi 13) ...)
```

**Timeline:** Future enhancement (not blocking R7RS compliance)

### 9.4 Optimization Opportunities

**After migration complete:**

1. **Lazy library loading**
   - Load libraries on-demand
   - Cache loaded libraries across REPL sessions

2. **Primitive inlining**
   - Inline frequently-used primitives (car, cdr, +, -)
   - Compiler optimization pass

3. **Library compilation**
   - Pre-compile Scheme code to IR
   - Ship compiled libraries for faster loading

4. **Parallel library loading**
   - Load independent libraries in parallel
   - Dependency graph analysis

**Note:** These are performance optimizations, not architectural changes.

---

## Appendix A: Code Examples

### A.1 Complete Mixed Library Example

**File: `crates/patina-tree-walker/src/eval/primitives/vectors.rs`**
```rust
use super::registry::{PrimitiveFn, PrimitiveRegistry, Arity, PrimitiveHandler};
use crate::eval::EvalError;
use patina_runtime::{Value, Vector};

pub fn vector_length(args: Vec<Value>) -> Result<Value, EvalError> {
    let vec = expect_vector(&args[0], "vector-length")?;
    Ok(Value::Integer(vec.borrow().len() as i64))
}

pub fn vector_ref(args: Vec<Value>) -> Result<Value, EvalError> {
    let vec = expect_vector(&args[0], "vector-ref")?;
    let idx = expect_integer(&args[1], "vector-ref")? as usize;

    vec.borrow()
        .get(idx)
        .cloned()
        .ok_or_else(|| EvalError::IndexOutOfBounds(idx, vec.borrow().len()))
}

pub fn register_vector_primitives(registry: &mut PrimitiveRegistry) {
    registry.register(PrimitiveFn {
        library: "scheme.base",
        name: "vector",
        arity: Arity::Min(0),
        help: "(vector obj ...) - Create vector from arguments",
        handler: PrimitiveHandler::Simple(vector_create),
    });

    registry.register(PrimitiveFn {
        library: "scheme.base",
        name: "vector-length",
        arity: Arity::Exactly(1),
        help: "(vector-length vec) - Get vector length",
        handler: PrimitiveHandler::Simple(vector_length),
    });

    registry.register(PrimitiveFn {
        library: "scheme.base",
        name: "vector-ref",
        arity: Arity::Exactly(2),
        help: "(vector-ref vec k) - Get vector element at index k",
        handler: PrimitiveHandler::Simple(vector_ref),
    });

    // ... more vector primitives
}
```

**File: `lib/scheme/base-extras.scm`**
```scheme
;; Derived vector operations (built on primitives)

(define (vector-copy vec)
  (let* ([len (vector-length vec)]
         [new-vec (make-vector len)])
    (let loop ([i 0])
      (when (< i len)
        (vector-set! new-vec i (vector-ref vec i))
        (loop (+ i 1))))
    new-vec))

(define (vector-map f vec)
  (let* ([len (vector-length vec)]
         [result (make-vector len)])
    (let loop ([i 0])
      (when (< i len)
        (vector-set! result i (f (vector-ref vec i)))
        (loop (+ i 1))))
    result))

(define (vector-for-each f vec)
  (let ([len (vector-length vec)])
    (let loop ([i 0])
      (when (< i len)
        (f (vector-ref vec i))
        (loop (+ i 1))))))

(define (vector->list vec)
  (let loop ([i (- (vector-length vec) 1)]
             [result '()])
    (if (< i 0)
        result
        (loop (- i 1) (cons (vector-ref vec i) result)))))

(define (list->vector lst)
  (let* ([len (length lst)]
         [vec (make-vector len)])
    (let loop ([i 0] [lst lst])
      (if (null? lst)
          vec
          (begin
            (vector-set! vec i (car lst))
            (loop (+ i 1) (cdr lst)))))))
```

**File: `crates/patina-runtime/src/stdlib/scheme_base.rs`**
```rust
use super::install_library_primitives;
use patina_runtime::{Environment, PrimitiveRegistry};
use std::rc::Rc;

pub fn build_scheme_base(
    _name: Vec<String>,
    env: Rc<Environment>,
    registry: &PrimitiveRegistry,
) -> Vec<String> {
    // Install all Rust primitives for scheme.base
    let mut exports = install_library_primitives(&env, registry, "scheme.base");

    // Load Scheme-defined utilities
    let extras = include_str!("../../../../lib/scheme/base-extras.scm");
    crate::evaluate_scheme_in_library(extras, &env)
        .expect("Failed to load base-extras.scm");

    // Add Scheme-defined exports
    exports.extend(vec![
        // Vector utilities
        "vector-copy", "vector-map", "vector-for-each",
        "vector->list", "list->vector",

        // List utilities
        "caar", "cadr", "cdar", "cddr",
        "fold-left", "fold-right",

        // Macros
        "let", "let*", "letrec", "letrec*",
        "cond", "case", "and", "or", "do",

        // ... all other Scheme-defined exports
    ]);

    exports
}
```

### A.2 Helper Functions

**File: `crates/patina-runtime/src/stdlib/helpers.rs`**
```rust
use crate::{Environment, Value, PrimitiveRegistry};
use crate::library_registry::LibraryError;
use std::rc::Rc;

/// Install all primitives for a library from the registry
pub fn install_library_primitives(
    env: &Rc<Environment>,
    registry: &PrimitiveRegistry,
    library_namespace: &str,
) -> Vec<String> {
    let primitives = registry.get_library_primitives(library_namespace);
    let mut exports = Vec::new();

    for prim in primitives {
        // Create primitive value
        let value = Value::Procedure(Procedure::Primitive {
            library: prim.library,
            name: prim.name,
            arity: prim.arity.clone(),
        });

        // Install in library environment
        env.define(prim.name.to_string(), value);

        // Add to exports
        exports.push(prim.name.to_string());
    }

    exports
}

/// Evaluate Scheme code in a library environment
pub fn evaluate_scheme_in_library(
    code: &str,
    env: &Rc<Environment>,
) -> Result<(), LibraryError> {
    use patina_frontend::Parser;
    use patina_tree_walker::Evaluator;

    // Parse the code
    let mut parser = Parser::new(code)
        .map_err(|e| LibraryError::ParseError {
            file: String::new(),
            message: format!("Parse error: {:?}", e),
        })?;

    // Create temporary evaluator for library code
    // (Or get from context if available)
    let eval = Evaluator::new();

    // Evaluate all expressions in library environment
    loop {
        match parser.parse() {
            Ok(expr) => {
                eval.eval_in_env(&expr, env)
                    .map_err(|e| LibraryError::EvaluationError {
                        message: format!("Evaluation error: {:?}", e),
                    })?;
            }
            Err(patina_frontend::ParseError::UnexpectedEof) => break,
            Err(e) => {
                return Err(LibraryError::ParseError {
                    file: String::new(),
                    message: format!("Parse error: {:?}", e),
                });
            }
        }
    }

    Ok(())
}
```

---

## Appendix B: Testing Checklist

### B.1 Phase 1 Testing

- [ ] All existing tests pass
- [ ] Bootstrap macros work (`let`, `cond`, `case`)
- [ ] Derived functions work (`caar`, `not`, etc.)
- [ ] `(scheme base)` available in REPL without import
- [ ] Can still `(import (scheme base))` explicitly
- [ ] No regressions in primitive functionality

### B.2 Phase 2 Testing

- [ ] All libraries load successfully
- [ ] All primitives accessible from libraries
- [ ] Library exports match expected
- [ ] No duplicate primitives
- [ ] Registry introspection works
- [ ] Library loading performance acceptable

### B.3 Phase 3 Testing

- [ ] Primitives from `(scheme char)` work
- [ ] Primitives from `(scheme complex)` work
- [ ] Primitives from all standard libraries work
- [ ] Error messages show correct library
- [ ] No namespace conflicts
- [ ] Primitive application performance acceptable

### B.4 Phase 4 Testing

**For each special library:**

- [ ] Library loads without error
- [ ] All exports present
- [ ] Basic functionality works
- [ ] Edge cases handled
- [ ] Error messages clear
- [ ] R7RS examples work

**Specific tests:**

`(scheme lazy)`:
```scheme
(define p (delay (+ 1 2)))
(promise? p)  ; => #t
(force p)     ; => 3
```

`(scheme eval)`:
```scheme
(eval '(+ 1 2) (environment '(scheme base)))  ; => 3
```

`(scheme case-lambda)`:
```scheme
(define f (case-lambda [() 0] [(x) x] [(x y) (+ x y)]))
(f)      ; => 0
(f 5)    ; => 5
(f 2 3)  ; => 5
```

`(scheme process-context)`:
```scheme
(command-line)                    ; => ("patina" ...)
(get-environment-variable "PATH") ; => "/usr/bin:..."
```

---

## Appendix C: Performance Considerations

### C.1 Library Loading Performance

**Current baseline:**
- Registry lookup: O(1) HashMap
- Library load: One-time cost at startup
- Primitive installation: O(n) where n = number of primitives

**After migration:**
- Registry lookup: Still O(1)
- Library load: May increase due to Scheme evaluation
- Mitigation: Cache compiled Scheme code (future)

**Measurement:**
```rust
let start = Instant::now();
let lib = evaluator.load_library(&["scheme", "base"])?;
println!("Loaded (scheme base) in {:?}", start.elapsed());
```

### C.2 Primitive Call Performance

**Current:**
- Qualified name lookup: `"scheme.base/+"` → O(1)
- Dispatch: Direct function call

**After migration:**
- Stored library in primitive → No extra lookup
- Dispatch: Still O(1), no performance change

**Benchmark:**
```scheme
(define (test n)
  (let loop ([i 0] [sum 0])
    (if (< i n)
        (loop (+ i 1) (+ sum i))
        sum)))

(test 1000000)  ; Measure time
```

### C.3 Memory Usage

**Registry:**
- Before: ~100 entries × ~100 bytes = ~10KB
- After: Same (no change)

**Libraries:**
- Before: Environment + exports
- After: Environment + exports + Scheme code AST
- Increase: ~5-10KB per library (acceptable)

**Total:** Minimal impact (<100KB for all standard libraries)

---

## Summary

This design provides a comprehensive blueprint for refactoring Patina's library system to:

1. ✅ Eliminate duplication between registry and library builders
2. ✅ Support mixed Rust/Scheme libraries for optimal performance and flexibility
3. ✅ Enable special libraries with custom semantics
4. ✅ Achieve R7RS compliance
5. ✅ Provide foundation for user-defined libraries

**Key Innovation:** The mixed library pattern allows developers to write performance-critical code in Rust while keeping derived functions and syntactic sugar in Scheme, offering the best of both worlds.

**Implementation Timeline:**
- Phase 1 (Bootstrap): 4-6 hours
- Phase 2 (Registry): 6-8 hours
- Phase 3 (Namespace): 4-6 hours
- Phase 4 (Special libs): 8-12 hours
- **Total: 22-32 hours**

**Next Step:** Begin Phase 1 implementation (bootstrap decoupling).

---

## Appendix D: System Architecture Diagrams

### D.1 Current Architecture (Before Migration)

```
┌─────────────────────────────────────────────────────────────────────────┐
│                              EVALUATOR                                  │
│                                                                         │
│  ┌──────────────────┐  ┌──────────────────┐  ┌────────────────────┐  │
│  │ Global Env       │  │ Primitive        │  │ Special Form       │  │
│  │                  │  │ Registry         │  │ Registry           │  │
│  │ +-primitives     │  │                  │  │                    │  │
│  │ +-macros         │  │ "scheme.base/+"  │  │ "if"  → IfForm     │  │
│  │ +-from bootstrap │  │ "scheme.base/-"  │  │ "let" → LetForm    │  │
│  │                  │  │ "scheme.char/..."│  │ "lambda" → ...     │  │
│  └──────────────────┘  │                  │  └────────────────────┘  │
│           ▲            │ ⚠️ Has namespace  │                          │
│           │            │ but not used!    │                          │
│           │            └──────────────────┘                          │
│           │                                                           │
│  ┌────────┴─────────────────────────────────────────────┐            │
│  │        BOOTSTRAP.SCM (loaded directly)                │            │
│  │  - Macros: let, let*, cond, case, and, or, do        │            │
│  │  - Derived: caar, cadr, not, fold-left, ...          │            │
│  │  - ❌ Not part of any library!                        │            │
│  │  - ❌ Injected into global scope                      │            │
│  └───────────────────────────────────────────────────────┘            │
│                                                                         │
│  ┌──────────────────────────────────────────────────────────────────┐ │
│  │              LIBRARY LOADING SYSTEM                              │ │
│  │                                                                  │ │
│  │  ┌────────────────────┐        ┌──────────────────────┐        │ │
│  │  │ RustLibraryLoader  │        │ SchemeLibraryLoader  │        │ │
│  │  │                    │        │                      │        │ │
│  │  │ Builders:          │        │ Parses .sld files    │        │ │
│  │  │ • build_scheme_base│        │                      │        │ │
│  │  │   - Lists all      │        │ ⚠️ Uses unsafe ptr   │        │ │
│  │  │     primitives     │        │    to evaluator!     │        │ │
│  │  │     MANUALLY! 😱    │        │                      │        │ │
│  │  │   - Duplicates     │        └──────────────────────┘        │ │
│  │  │     registry info  │                                        │ │
│  │  │                    │                                        │ │
│  │  │ • build_scheme_char│                                        │ │
│  │  │   - More manual    │                                        │ │
│  │  │     lists...       │                                        │ │
│  │  └────────────────────┘                                        │ │
│  │                                                                  │ │
│  └──────────────────────────────────────────────────────────────────┘ │
│                                                                         │
│  apply_primitive() {                                                   │
│    let qualified = format!("scheme.base/{}", name);  ❌ HARDCODED!     │
│    registry.apply(qualified, ...)                                      │
│  }                                                                      │
└─────────────────────────────────────────────────────────────────────────┘

PROBLEMS:
❌ Bootstrap not part of library system
❌ Duplicate primitive lists (registry vs builders)
❌ Hardcoded "scheme.base/" prefix
❌ Unsafe pointer in SchemeLibraryLoader
❌ Can't access primitives from other namespaces
```

### D.2 New Architecture (After Migration)

```
┌─────────────────────────────────────────────────────────────────────────┐
│                              EVALUATOR                                  │
│                                                                         │
│  ┌──────────────────┐  ┌──────────────────┐  ┌────────────────────┐  │
│  │ Global Env       │  │ Primitive        │  │ Special Form       │  │
│  │                  │  │ Registry         │  │ Registry           │  │
│  │ Imports from:    │  │ ✅ SINGLE SOURCE │  │                    │  │
│  │ • (scheme base)  │  │    OF TRUTH      │  │ "if"  → IfForm     │  │
│  │                  │  │                  │  │ "delay" → DelayForm│  │
│  │ (auto for REPL)  │  │ "scheme.base/+"  │  │ "case-lambda" → ...│  │
│  └──────────────────┘  │ "scheme.base/-"  │  └────────────────────┘  │
│           ▲            │ "scheme.char/..."│           ▲              │
│           │            │ "scheme.lazy/..."│           │              │
│           │            └──────────┬───────┘           │              │
│           │                       │                   │              │
│           │                       │                   │              │
│  ┌────────┴───────────────────────┼───────────────────┼────────────┐ │
│  │        LIBRARY SYSTEM          │                   │            │ │
│  │                                ▼                   │            │ │
│  │  ┌─────────────────────────────────────────────────┴──────────┐ │ │
│  │  │              LibraryLoaderRegistry                         │ │ │
│  │  │                                                            │ │ │
│  │  │  ┌──────────────────────┐  ┌──────────────────────────┐  │ │ │
│  │  │  │ Simple Loaders       │  │ Evaluating Loaders       │  │ │ │
│  │  │  │ (RustLibraryLoader)  │  │ (SchemeLibraryLoader)    │  │ │ │
│  │  │  │                      │  │                          │  │ │ │
│  │  │  │ Receives registry ─┐ │  │ ✅ Stateless!            │  │ │ │
│  │  │  │                    │ │  │ ✅ No unsafe pointer     │  │ │ │
│  │  │  │ Builders query it: │ │  │                          │  │ │ │
│  │  │  │                    ▼ │  │ Returns ParsedLibrary    │  │ │ │
│  │  │  │ build_scheme_base( │  │ Evaluator handles eval   │  │ │ │
│  │  │  │   env,             │  │                          │  │ │ │
│  │  │  │   registry ◄───────┼──┼─────────┐                │  │ │ │
│  │  │  │ ) {                │  │         │                │  │ │ │
│  │  │  │   // Auto-query!   │  │         │                │  │ │ │
│  │  │  │   let prims =      │  │         │                │  │ │ │
│  │  │  │     registry       │  │         │                │  │ │ │
│  │  │  │     .get_library(  │  │         │                │  │ │ │
│  │  │  │      "scheme.base")│  │         │                │  │ │ │
│  │  │  │                    │  │         │                │  │ │ │
│  │  │  │   // Include Scheme│  │         │                │  │ │ │
│  │  │  │   include_str!(    │  │         │                │  │ │ │
│  │  │  │    "base-extras")  │  │         │                │  │ │ │
│  │  │  │ }                  │  │         │                │  │ │ │
│  │  │  └──────────────────────┘  └─────────┼────────────────┘  │ │ │
│  │  └────────────────────────────────────────┼──────────────────┘ │ │
│  │                                           │                    │ │
│  └───────────────────────────────────────────┼────────────────────┘ │
│                                              │                      │
│  apply_primitive(proc) {                     │                      │
│    let qualified = format!("{}/{}", ◄────────┘                      │
│                   proc.library,  ✅ From primitive itself!           │
│                   proc.name);                                        │
│    registry.apply(qualified, ...)                                   │
│  }                                                                   │
└─────────────────────────────────────────────────────────────────────────┘

                               ┌──────────────────────┐
                               │  File System         │
                               │                      │
                               │  lib/                │
                               │  ├── bootstrap.scm   │
                               │  │   (empty/minimal) │
                               │  └── scheme/         │
                               │      ├── base-extras.scm
                               │      ├── char-extras.scm
                               │      ├── lazy.scm    │
                               │      └── eval.scm    │
                               └──────────────────────┘

IMPROVEMENTS:
✅ Registry is single source of truth
✅ Library builders query registry (no duplication)
✅ Namespace stored in primitives
✅ Mixed Rust/Scheme libraries
✅ Bootstrap minimal/empty
✅ No unsafe code
```

### D.3 Mixed Library Data Flow

```
┌─────────────────────────────────────────────────────────────────────────┐
│                   (scheme base) - Mixed Library                         │
└─────────────────────────────────────────────────────────────────────────┘
                                    │
                                    │ User: (import (scheme base))
                                    │
                                    ▼
              ┌─────────────────────────────────────┐
              │   Evaluator.load_library()          │
              │   name: ["scheme", "base"]          │
              └──────────────┬──────────────────────┘
                             │
                             ▼
              ┌──────────────────────────────────────┐
              │  LibraryLoaderRegistry.try_simple()  │
              │  → RustLibraryLoader found!          │
              └──────────────┬───────────────────────┘
                             │
                             ▼
              ┌──────────────────────────────────────────────────┐
              │  build_scheme_base(                              │
              │    name: ["scheme", "base"],                     │
              │    env: <fresh environment>,                     │
              │    registry: &PrimitiveRegistry  ◄───────────────┼──┐
              │  )                                               │  │
              └──────────────┬───────────────────────────────────┘  │
                             │                                      │
                 ┌───────────┴───────────┐                          │
                 │                       │                          │
                 ▼                       ▼                          │
      ┌──────────────────┐   ┌──────────────────────┐              │
      │  RUST PART       │   │  SCHEME PART         │              │
      │  (Performance)   │   │  (Convenience)       │              │
      └──────────────────┘   └──────────────────────┘              │
                │                       │                          │
                │ Step 1                │ Step 2                   │
                ▼                       ▼                          │
   ┌─────────────────────────┐  ┌──────────────────────┐          │
   │ Query registry:         │  │ Include Scheme file: │          │
   │                         │  │                      │          │
   │ registry                │  │ include_str!(        │          │
   │  .get_library_primitives│  │  "lib/scheme/       │          │
   │  ("scheme.base")        │  │   base-extras.scm") │          │
   │                         │  │                      │          │
   │ Returns:                │  │ Contains:            │          │
   │ • +, -, *, /            │  │ • (define (caar x)   │          │
   │ • cons, car, cdr        │  │     (car (car x)))   │          │
   │ • list, append          │  │ • (define (not x)    │          │
   │ • number?, pair?        │  │     (if x #f #t))    │          │
   │ • ... 100+ primitives   │  │ • (define-syntax let │          │
   │                         │  │     ...)             │          │
   └───────────┬─────────────┘  └───────────┬──────────┘          │
               │                            │                     │
               │ Step 3                     │ Step 4              │
               ▼                            ▼                     │
   ┌───────────────────────┐    ┌────────────────────────┐       │
   │ Install in env:       │    │ Evaluate in env:       │       │
   │                       │    │                        │       │
   │ env.define(           │    │ evaluator.eval_in_env( │       │
   │   "+",                │    │   scheme_code,         │       │
   │   Primitive {         │    │   &env)                │       │
   │     library: "s.base",│    │                        │       │
   │     name: "+",        │    │ Creates bindings:      │       │
   │     arity: Min(0)     │    │ • caar → <lambda>      │       │
   │   }                   │    │ • not → <lambda>       │       │
   │ )                     │    │ • let → <syntax>       │       │
   │                       │    │                        │       │
   │ ... repeat for all    │    └────────────────────────┘       │
   └───────────────────────┘                                     │
               │                            │                     │
               │                            │                     │
               └────────────┬───────────────┘                     │
                            │                                     │
                            ▼                                     │
                ┌───────────────────────────┐                     │
                │ Return export list:       │                     │
                │                           │                     │
                │ ["+", "-", "*", "/",      │                     │
                │  "cons", "car", "cdr",    │                     │
                │  "caar", "cadr", "not",   │                     │
                │  "let", "let*", "cond",   │                     │
                │  ... all exports]         │                     │
                └───────────┬───────────────┘                     │
                            │                                     │
                            ▼                                     │
                ┌───────────────────────────┐                     │
                │ Library object created:   │                     │
                │                           │                     │
                │ Library {                 │                     │
                │   name: ["scheme","base"] │                     │
                │   env: <populated env>    │                     │
                │   exports: HashMap {      │                     │
                │     "+": Primitive{...}   │                     │
                │     "caar": Lambda{...}   │                     │
                │     "let": Syntax{...}    │                     │
                │     ...                   │                     │
                │   }                       │                     │
                │ }                         │                     │
                └───────────┬───────────────┘                     │
                            │                                     │
                            ▼                                     │
                ┌───────────────────────────────────────────┐     │
                │ Import into user environment:             │     │
                │                                           │     │
                │ for (name, value) in library.exports {    │     │
                │   user_env.define(name, value);           │     │
                │ }                                         │     │
                │                                           │     │
                │ User now has access to:                   │     │
                │ • Fast Rust primitives: +, cons, car, ... │     │
                │ • Scheme derived: caar, not, ...          │     │
                │ • Scheme macros: let, cond, ...           │     │
                └───────────────────────────────────────────┘     │
                                                                  │
KEY INSIGHT:                                                      │
═══════════                                                       │
                                                                  │
Single library combines:                                         │
• Rust primitives (from registry) ───────────────────────────────┘
• Scheme code (from .scm file)
• Both exported through unified interface
• User sees seamless integration!
```

### D.4 Special Library: (scheme lazy) Architecture

```
                        ┌─────────────────────────────┐
                        │  (import (scheme lazy))     │
                        └──────────────┬──────────────┘
                                       │
                                       ▼
                        ┌──────────────────────────────────────┐
                        │  build_scheme_lazy(env, registry)    │
                        └──────────────┬───────────────────────┘
                                       │
                  ┌────────────────────┴──────────────────┐
                  │                                       │
                  ▼                                       ▼
     ┌────────────────────────┐              ┌──────────────────────┐
     │  Install primitives:   │              │  Export special form:│
     │                        │              │                      │
     │  registry              │              │  "delay" →           │
     │   .get_library(        │              │    DelayForm         │
     │     "scheme.lazy")     │              │    (from registry)   │
     │                        │              │                      │
     │  Returns:              │              └──────────────────────┘
     │  • force               │
     │  • promise?            │
     │  • make-promise        │
     └────────────────────────┘
                  │
                  │
                  ▼
        ┌─────────────────────────────┐
        │  Library exports:           │
        │  • delay (special form)     │
        │  • force (primitive)        │
        │  • promise? (primitive)     │
        │  • make-promise (primitive) │
        └─────────────┬───────────────┘
                      │
                      │ User code: (delay (+ 1 2))
                      │
                      ▼
        ┌──────────────────────────────────────┐
        │  SpecialFormRegistry.eval()          │
        │  form: "delay"                       │
        │  args: [(+ 1 2)]  ← UNEVALUATED!     │
        └──────────────┬───────────────────────┘
                       │
                       ▼
        ┌──────────────────────────────────────┐
        │  DelayForm::eval()                   │
        │                                      │
        │  Creates:                            │
        │  Value::Promise(Promise {            │
        │    state: Delayed {                  │
        │      expr: (+ 1 2),                  │
        │      env: <current env>              │
        │    }                                 │
        │  })                                  │
        │                                      │
        │  ✅ Expression NOT evaluated yet!    │
        └──────────────┬───────────────────────┘
                       │
                       │ Returns promise
                       │
                       ▼
        ┌──────────────────────────────────────┐
        │  User: (force promise)               │
        └──────────────┬───────────────────────┘
                       │
                       ▼
        ┌──────────────────────────────────────┐
        │  PrimitiveRegistry.apply()           │
        │  qualified: "scheme.lazy/force"      │
        └──────────────┬───────────────────────┘
                       │
                       ▼
        ┌──────────────────────────────────────┐
        │  force_primitive(eval, args, ...)   │
        │                                      │
        │  match promise.state {               │
        │    Delayed { expr, env } => {        │
        │      // NOW evaluate!                │
        │      result = eval.eval_in_env(      │
        │        expr, env)                    │
        │                                      │
        │      // Memoize                      │
        │      promise.state = Forced(result)  │
        │                                      │
        │      result                          │
        │    }                                 │
        │    Forced(value) => value            │
        │  }                                   │
        └──────────────────────────────────────┘

SPECIAL FEATURE: Delayed Evaluation
═════════════════════════════════════
• delay = special form (doesn't evaluate argument)
• force = primitive (has evaluator access)
• Together they implement lazy semantics
• Demonstrates special form + primitive cooperation
```

### D.5 Component Interaction Summary

```
┌─────────────────────────────────────────────────────────────────┐
│                      COMPONENT LAYERS                           │
└─────────────────────────────────────────────────────────────────┘

Layer 1: REGISTRIES (Single Source of Truth)
════════════════════════════════════════════
┌──────────────────────┐  ┌──────────────────────┐
│ PrimitiveRegistry    │  │ SpecialFormRegistry  │
│                      │  │                      │
│ Maps:                │  │ Maps:                │
│ "s.base/+" → fn      │  │ "if" → IfForm        │
│ "s.char/..." → fn    │  │ "delay" → DelayForm  │
│ "s.lazy/..." → fn    │  │ "lambda" → ...       │
│                      │  │                      │
│ ✅ Knows namespaces   │  │ ✅ Pluggable forms   │
└──────────────────────┘  └──────────────────────┘
         ▲                          ▲
         │                          │
         │                          │
Layer 2: LIBRARY BUILDERS (Query Registries)
═════════════════════════════════════════════
         │                          │
         │                          │
┌────────┴──────────────────────────┴──────────┐
│  RustLibraryLoader                           │
│                                              │
│  Builders receive &PrimitiveRegistry:        │
│                                              │
│  build_scheme_base(env, registry) {          │
│    ┌─────────────────────┐                  │
│    │ Step 1: Query       │                  │
│    │ registry for prims  │──────────────────┼──┐
│    └─────────────────────┘                  │  │
│    ┌─────────────────────┐                  │  │
│    │ Step 2: Include     │                  │  │
│    │ Scheme code         │                  │  │
│    └─────────────────────┘                  │  │
│    ┌─────────────────────┐                  │  │
│    │ Step 3: Return      │                  │  │
│    │ combined exports    │                  │  │
│    └─────────────────────┘                  │  │
│  }                                          │  │
└─────────────────────────────────────────────┘  │
         │                                       │
         │                                       │
Layer 3: LIBRARIES (Mixed Rust + Scheme)         │
═════════════════════════════════════════════    │
         │                                       │
         ▼                                       │
┌───────────────────────────────────────┐        │
│  Library Object                       │        │
│                                       │        │
│  env: Environment {                   │        │
│    Rust primitives: ◄─────────────────┼────────┘
│      +, -, *, cons, car, ...          │
│                                       │
│    Scheme definitions:                │
│      caar, not, let, cond, ...        │
│  }                                    │
│                                       │
│  exports: {                           │
│    "+": Primitive{lib:"s.base",...}   │
│    "caar": Lambda{...}                │
│    "let": Syntax{...}                 │
│  }                                    │
└───────────────────────────────────────┘
         │
         │
Layer 4: USER CODE
══════════════════
         │
         ▼
┌───────────────────────────────────────┐
│  (import (scheme base))               │
│                                       │
│  (+ 1 2)        ; Rust primitive      │
│  (caar '((1)))  ; Scheme derived      │
│  (let ((x 5))   ; Scheme macro        │
│    (+ x 1))                           │
└───────────────────────────────────────┘

DATA FLOW:
═════════
1. Primitives registered in PrimitiveRegistry (Rust code)
2. Library builder queries registry for primitives
3. Builder includes Scheme code for derived functions
4. Library object combines both seamlessly
5. User imports library, gets all features
6. No distinction between Rust and Scheme from user perspective!
```

These diagrams illustrate the key architectural improvements and how components interact in the new design.

---

## Appendix E: Name Resolution Walkthrough

### How the System Resolves Function Calls

**Question:** When the user calls function `foo`, how do we know if it's a user-defined function or a function from a library?

**Answer:** Through environment chain lookup. The system doesn't distinguish between "user-defined" and "library" functions—everything is just a binding in the environment chain.

### E.1: The Environment Chain

```
┌─────────────────────────────────────────────────────────────┐
│                    ENVIRONMENT CHAIN                        │
│                                                             │
│  User Code Env ──► Imported Library Env ──► Global Env     │
│   (local vars)      (library bindings)      (primitives)   │
└─────────────────────────────────────────────────────────────┘

Example after: (import (scheme base))
               (define user-func (lambda (x) (* x 2)))

┌──────────────────────┐
│   User Code Env      │
│  ┌────────────────┐  │
│  │ user-func: λ   │  │  ← User-defined functions
│  │ x: 42          │  │  ← Local variables
│  └────────────────┘  │
│    parent ↓          │
└──────────────────────┘
         ↓
┌──────────────────────┐
│  Global Env          │
│  ┌────────────────┐  │
│  │ +: Primitive   │  │  ← From (scheme base) import
│  │ *: Primitive   │  │
│  │ map: Primitive │  │
│  │ let: Syntax    │  │  ← Macro from bootstrap.scm
│  │ caar: Lambda   │  │  ← Scheme-defined from bootstrap
│  └────────────────┘  │
│    parent: None      │
└──────────────────────┘
```

### E.2: Detailed Resolution Example

Let's trace what happens when evaluating: `(user-func (+ 3 4))`

#### Step 1: Evaluate `(user-func (+ 3 4))`

```
Expression: (user-func (+ 3 4))
Context: User code environment (has parent → global env)

Evaluator calls: eval_step_impl(expr, user_env, tail=false)
```

**File:** `crates/patina-tree-walker/src/eval/mod.rs:252`

```rust
fn eval_step_impl(&self, expr: &Value, env: &Rc<Environment>, ...) {
    match expr {
        // For a list like (user-func (+ 3 4)):
        Value::Pair(_, _) => {
            let (car, cdr) = extract_list_parts(expr);

            // car = 'user-func (symbol)
            // cdr = ((+ 3 4))

            // Need to evaluate 'user-func to get the procedure
            let proc = self.eval_in_env(&car, env)?;  // → Triggers symbol lookup
            ...
```

#### Step 2: Resolve Symbol `user-func`

```
Expression: user-func (symbol)
Context: User code environment

Evaluator matches: Value::Symbol(name)
```

**File:** `crates/patina-tree-walker/src/eval/mod.rs:283`

```rust
Value::Symbol(name) => {
    // name = "user-func"

    // Try lookup in current environment (user_env)
    if let Some(value) = env.get(name) {
        return Ok(EvalResult::Value(value));  // ✅ FOUND!
    }

    // If not found, try global environment (for gensyms)
    if let Some(value) = self.global_env.get(name) {
        return Ok(EvalResult::Value(value));
    }

    // Not found anywhere - error
    return Err(EvalError::UndefinedVariable(name.clone()));
}
```

**File:** `crates/patina-runtime/src/environment.rs:49`

```rust
// Environment::get() walks the parent chain
pub fn get(&self, name: &str) -> Option<Value> {
    // name = "user-func"

    // Step A: Check local bindings first
    if let Some(value) = self.bindings.borrow().get(name) {
        return Some(value.clone());  // ✅ Found in user_env!
    }

    // Step B: Not found locally, try parent environment
    else if let Some(parent) = &self.parent {
        parent.get(name)  // Would search global_env if not found
    }

    // Step C: No parent, not found
    else {
        None
    }
}
```

**Result:** `user-func` resolves to `Lambda { params: [x], body: (* x 2), ... }`

#### Step 3: Resolve Symbol `+` in Argument `(+ 3 4)`

```
Expression: (+ 3 4)
Context: User code environment (evaluating argument to user-func)

Evaluator needs to evaluate this before calling user-func
```

Same process:
1. Extract car = `+` (symbol), cdr = `(3 4)`
2. Evaluate symbol `+`:
   - Check `user_env.bindings` → **NOT FOUND**
   - Check `user_env.parent` (which is `global_env`) → **FOUND!**
   - `+` → `Primitive { name: "+", arity: Some(0..), namespace: "scheme.base", ... }`

**Result:** `+` resolves to the primitive addition function

#### Step 4: Call the Resolved Functions

```
1. Evaluate (+ 3 4):
   - Procedure: Primitive(+)
   - Arguments: [Integer(3), Integer(4)]
   - Result: Integer(7)

2. Evaluate (user-func 7):
   - Procedure: Lambda { params: [x], body: (* x 2), env: user_env }
   - Arguments: [Integer(7)]
   - Create new environment with parent = lambda's captured env
   - Bind: x = 7
   - Evaluate body: (* x 2)
     - Resolve *: found in global_env → Primitive(*)
     - Resolve x: found in lambda env → Integer(7)
     - Result: Integer(14)

Final Result: 14
```

### E.3: What About Library Functions?

Let's trace: `(map (lambda (x) (+ x 1)) '(1 2 3))`

#### Import Creates Bindings in Global Environment

When you evaluate `(import (scheme base))`:

**File:** `crates/patina-tree-walker/src/eval/special_forms.rs` (import handler)

```rust
fn eval_import(&self, import_set: &ImportSet, env: &Rc<Environment>) {
    // import_set = (scheme base)

    // 1. Load the library
    let lib = self.load_library(&["scheme", "base"])?;

    // lib.exports = {
    //   "+": Primitive { ... },
    //   "map": Primitive { ... },
    //   "let": Syntax { ... },
    //   "caar": Lambda { ... },
    //   ...
    // }

    // 2. Copy exports into current environment (usually global_env)
    for (name, value) in &lib.exports {
        env.define(name.clone(), value.clone());
    }

    // Now global_env has: {"+": Primitive, "map": Primitive, ...}
}
```

**Result:** `map` is now bound in `global_env`

#### Resolving `map`

```
Expression: map (symbol)
Context: User code environment

Resolution:
1. Check user_env.bindings → NOT FOUND
2. Check user_env.parent (global_env) → FOUND!
3. map → Primitive { name: "map", namespace: "scheme.base", ... }
```

**The Key Insight:** From the evaluator's perspective, there's **no difference** between:
- User-defined: `user-func` bound in user environment
- Library function: `map` bound in global environment via import
- Primitive: `+` bound in global environment via import

All are just **bindings in the environment chain**!

### E.4: Shadowing Example

What if the user redefines a library function?

```scheme
(import (scheme base))

(define + (lambda (a b) (* a b)))  ; Redefine + as multiplication!

(+ 2 3)  ; What happens?
```

#### Resolution Process

```
┌──────────────────────┐
│   User Code Env      │
│  ┌────────────────┐  │
│  │ +: Lambda{*}   │  │  ← User's redefinition (SHADOWS!)
│  └────────────────┘  │
│    parent ↓          │
└──────────────────────┘
         ↓
┌──────────────────────┐
│  Global Env          │
│  ┌────────────────┐  │
│  │ +: Primitive   │  │  ← Original from (scheme base)
│  │ *: Primitive   │  │  ← This is what user's + calls
│  └────────────────┘  │
└──────────────────────┘

When evaluating (+2 3):
1. Resolve + in user_env → FOUND locally → Lambda{*}
2. Never checks parent environment!
3. Calls user's lambda: (* 2 3)
   4. Resolve * in lambda's env → checks parent → finds Primitive(*)
   5. Result: 6

Answer: 6 (not 5!)
```

**This is correct Scheme behavior!** Local definitions shadow imported ones.

### E.5: Library-Private Definitions

Libraries can have internal definitions that aren't exported:

```scheme
; In file: lib/mylib.sld
(define-library (mylib)
  (import (scheme base))
  (export public-func)
  (begin
    (define private-helper (lambda (x) (* x 2)))  ; NOT exported
    (define public-func (lambda (x)               ; Exported
      (+ (private-helper x) 1)))))
```

When loaded:

```
┌────────────────────────────────┐
│  Library (mylib) Environment   │
│  ┌──────────────────────────┐  │
│  │ private-helper: Lambda   │  │  ← Internal only
│  │ public-func: Lambda      │  │  ← Will be exported
│  └──────────────────────────┘  │
│  parent → global_env            │
└────────────────────────────────┘

After import:
┌──────────────────────────────┐
│  User's Global Env           │
│  ┌────────────────────────┐  │
│  │ public-func: Lambda    │  │  ← Only exported binding!
│  └────────────────────────┘  │  (private-helper NOT copied)
└──────────────────────────────┘

public-func's closure still has access to private-helper
through its captured environment!
```

### E.6: Summary - How Name Resolution Works

```
┌─────────────────────────────────────────────────────────────┐
│              WHEN EVALUATING SYMBOL 'foo'                   │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  1. Check Current Environment's Local Bindings              │
│     - User-defined variables: (define foo ...)             │
│     - Lambda parameters: (lambda (foo) ...)                │
│     - Let bindings: (let ((foo ...)) ...)                  │
│                                                             │
│  2. If Not Found, Walk Parent Chain                        │
│     - Check parent environment's bindings                   │
│     - Repeat until found or reach environment with no parent│
│                                                             │
│  3. Special Case: Gensyms                                  │
│     - If symbol is a gensym (##name#42) and not in chain   │
│     - Try global environment (for macro hygiene)           │
│                                                             │
│  4. Not Found Anywhere                                     │
│     - Error: Undefined variable 'foo'                      │
│                                                             │
├─────────────────────────────────────────────────────────────┤
│  THERE IS NO "IS THIS FROM A LIBRARY?" CHECK               │
│                                                             │
│  Everything is just a binding in an environment!            │
│  - Primitives: bound by library builder                    │
│  - Library functions: bound by library builder             │
│  - Imported bindings: copied into current env by import    │
│  - User definitions: bound by define                       │
│  - Local variables: bound by lambda/let                    │
│                                                             │
│  The environment chain naturally handles precedence:        │
│  - Innermost (local) bindings shadow outer ones            │
│  - User definitions can shadow imports                     │
│  - Lexical scoping "just works"                            │
└─────────────────────────────────────────────────────────────┘
```

### E.7: Code Locations for Name Resolution

All the key code is already in place:

1. **Symbol Resolution**
   `crates/patina-tree-walker/src/eval/mod.rs:283-298`
   ```rust
   Value::Symbol(name) => {
       if let Some(value) = env.get(name) { ... }
   }
   ```

2. **Environment Lookup with Parent Chain**
   `crates/patina-runtime/src/environment.rs:49-58`
   ```rust
   pub fn get(&self, name: &str) -> Option<Value> {
       // Check local, then parent recursively
   }
   ```

3. **Import Processing**
   `crates/patina-tree-walker/src/eval/special_forms.rs` (import handler)
   ```rust
   fn eval_import(...) {
       // Loads library and copies exports to current env
   }
   ```

4. **Library Export Collection**
   `crates/patina-runtime/src/library.rs:59-61`
   ```rust
   pub fn export(&mut self, name: String, value: Value) {
       self.exports.insert(name, value);
   }
   ```

The beauty of this design: **name resolution is completely orthogonal to where the binding came from**. The environment chain handles everything uniformly!
