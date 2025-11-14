# Architecture Review & Refactoring Roadmap

**Date:** 2025-11-13
**Reviewer:** Architectural Analysis
**Codebase Size:** ~16,700 LOC across 89 Rust files
**Overall Grade:** B+ (Good foundation, needs architectural cleanup for multi-backend future)

## Executive Summary

Patina has a **solid foundation** with clean separation of concerns at the crate level. The tree-walker interpreter is production-ready for R7RS compliance. However, to achieve the stated goals of supporting multiple backends (VM, JIT) and future extensions (gradual typing, reactive concurrency), the codebase needs architectural refactoring to move from a **modular monolith** to a **pluggable framework**.

**Core Issue:** The architecture has good instincts (traits, registries, separation) but hasn't fully committed to abstraction. The codebase is ~70% pluggable - the remaining 30% involves adding trait boundaries at key interfaces.

---

## Current Architecture

### Dependency Graph

```
patina-repl → patina-interpreter → patina-tree-walker → patina-runtime
                                 ↗  patina-frontend    ↗
patina-tests → patina-interpreter
patina-ir (isolated, not yet integrated)
```

### Crate Responsibilities

| Crate | Responsibility | LOC | Quality |
|-------|---------------|-----|---------|
| `patina-runtime` | Core types (Value, Environment, Library) | ~3k | ✅ Excellent |
| `patina-frontend` | Lexer, Parser, Macro Expander | ~4k | ✅ Excellent |
| `patina-tree-walker` | Tree-walking interpreter backend | ~5k | ✅ Good |
| `patina-interpreter` | High-level API facade | ~150 | ⚠️ Needs abstraction |
| `patina-repl` | Interactive REPL | ~1k | ✅ Good |
| `patina-tests` | Integration & compliance tests | ~3k | ⚠️ Coupling issues |
| `patina-ir` | Future IR definitions | ~500 | ⏸️ Not integrated |

---

## Strengths - What's Working Well

### 1. Clean Separation of Frontend & Runtime ✅

**Evidence:**
- `patina-runtime` has zero dependencies on evaluation strategy
- `patina-frontend` is completely isolated from backend
- No circular dependencies in the workspace

**Impact:** This is **textbook-correct** architecture for multi-backend compilers/interpreters.

### 2. Trait-Based Extensibility (Partial) ✅

**Implemented:**
- `LibraryLoader` trait (`patina-runtime/src/library_loader.rs:25-41`)
  - Enables pluggable library loading (Rust, Scheme, mixed)
  - Registry pattern for loader composition
- `MacroExpander` trait (`patina-frontend/src/macro_expander/interface.rs:20-33`)
  - Clean abstraction for macro implementations
  - Supports testing and future compiler optimization

**Assessment:** Strong design patterns where implemented.

### 3. Sound Evaluation Architecture ✅

**Trampoline-based TCO:**
```rust
pub enum EvalResult {
    Value(Value),
    TailCall { expr: Value, env: Rc<Environment> },
    TailCallPrimitive { proc: Value, args: Vec<Value> },
}
```

- Implements proper tail call optimization (R7RS requirement)
- Clean separation of values vs continuations
- Tail position tracking throughout evaluation

**Assessment:** Production-quality implementation.

---

## Critical Issues - Architectural Debt

### Issue #1: Backend Coupling ✅ COMPLETED

**Status:** RESOLVED (implemented before review)
**Location:** `patina-runtime/src/backend.rs`, `patina-tree-walker/src/backend.rs`

**Previous Problem:**
```rust
pub use patina_tree_walker::{EvalError, Evaluator};  // ❌ Direct coupling

pub struct Interpreter {
    evaluator: Evaluator,  // ❌ Concrete type, not a trait
}
```

**Impact:**
- Cannot swap VM/JIT backends without modifying `patina-interpreter`
- Violates stated goal: "supports multiple backend implementations"
- Every backend-specific type leaks through the public API
- Forces all tests to use tree-walker (no backend isolation)

**Root Cause:** No `Backend` trait abstraction.

**Solution:**

```rust
// In patina-runtime/src/backend.rs (NEW)
pub trait Backend {
    type Error: std::error::Error + From<RuntimeError>;

    /// Evaluate an expression in the given environment
    fn eval(&self, expr: &Value, env: &Rc<Environment>)
        -> Result<Value, Self::Error>;

    /// Get the global environment
    fn global_env(&self) -> &Rc<Environment>;

    /// Evaluate in a specific environment (for library loading, REPL)
    fn eval_in_env(&self, expr: &Value, env: &Rc<Environment>)
        -> Result<Value, Self::Error>;
}

// In patina-interpreter/src/lib.rs
pub struct Interpreter<B: Backend> {
    backend: B,
}

impl<B: Backend> Interpreter<B> {
    pub fn new(backend: B) -> Self {
        Interpreter { backend }
    }

    pub fn eval_str(&self, input: &str) -> Result<Value, InterpreterError<B::Error>> {
        let mut parser = Parser::new(input)?;
        let expr = parser.parse()?;
        Ok(self.backend.eval(&expr, self.backend.global_env())?)
    }
}

// Convenience type alias for default backend
pub type TreeWalkInterpreter = Interpreter<TreeWalker>;
```

**Migration Path:**
1. Define `Backend` trait in `patina-runtime`
2. Create `TreeWalker` newtype wrapper around `Evaluator`
3. Implement `Backend` for `TreeWalker`
4. Make `Interpreter` generic (with default type parameter for compatibility)
5. Update `patina-repl` to use `TreeWalkInterpreter`

**Current Implementation:**
- ✅ `Backend` trait defined in `patina-runtime/src/backend.rs`
- ✅ `TreeWalker` wrapper in `patina-tree-walker/src/backend.rs`
- ✅ Generic `Interpreter<B: Backend>` in `patina-interpreter`
- ✅ Backend-agnostic error type: `InterpreterError<E>`
- ✅ Type alias `TreeWalkInterpreter` for convenience
- ✅ Full test coverage for trait implementation

**Achievement:** Architecture supports pluggable backends, ready for VM/JIT
**Next Steps:** Can now implement VM or JIT backend without modifying interpreter

---

### Issue #2: Primitive Dispatch - 530-Line Match Statement ✅ COMPLETED

**Status:** RESOLVED as of 2025-11-13
**Location:** `patina-tree-walker/src/eval/primitives/mod.rs` (now using registry)

**Previous Problem:**
```rust
pub fn apply_primitive(&self, name: &str, args: Vec<Value>,
                       in_tail_position: bool) -> Result<EvalResult, EvalError> {
    match name {
        "+" => arithmetic::add(self, args).map(EvalResult::Value),
        "-" => arithmetic::subtract(self, args).map(EvalResult::Value),
        "*" => arithmetic::multiply(self, args).map(EvalResult::Value),
        // ... 100+ more cases ...
        "vector-fill!" => vectors::vector_fill(self, args).map(EvalResult::Value),
        _ => Err(EvalError::UndefinedVariable(name.to_string())),
    }
}
```

**Impact:**
- **Not pluggable:** Cannot add primitives without modifying this file
- **Compile-time only:** No runtime registration
- **Poor scalability:** Every new primitive touches central dispatch
- **No introspection:** Primitives can't self-describe (arity, documentation)
- **Duplicated code:** `.map(EvalResult::Value)` repeated 100+ times

**Solution:**

```rust
// In patina-tree-walker/src/eval/primitives/registry.rs (NEW)

/// A primitive procedure with metadata
pub struct PrimitiveFn {
    pub name: &'static str,
    pub arity: Arity,
    pub help: &'static str,
    pub handler: fn(&Evaluator, Vec<Value>, bool) -> Result<EvalResult, EvalError>,
}

pub struct PrimitiveRegistry {
    primitives: HashMap<String, PrimitiveFn>,
}

impl PrimitiveRegistry {
    pub fn new() -> Self {
        Self { primitives: HashMap::new() }
    }

    pub fn register(&mut self, prim: PrimitiveFn) {
        self.primitives.insert(prim.name.to_string(), prim);
    }

    pub fn get(&self, name: &str) -> Option<&PrimitiveFn> {
        self.primitives.get(name)
    }

    pub fn apply(&self, name: &str, args: Vec<Value>,
                 evaluator: &Evaluator, in_tail: bool)
                 -> Result<EvalResult, EvalError> {
        let prim = self.get(name)
            .ok_or_else(|| EvalError::UndefinedVariable(name.to_string()))?;

        // Arity checking in one place
        prim.arity.check(args.len())?;

        (prim.handler)(evaluator, args, in_tail)
    }

    /// List all primitives (for help system)
    pub fn list_primitives(&self) -> Vec<&PrimitiveFn> {
        self.primitives.values().collect()
    }
}

// In primitives/arithmetic.rs
pub fn register_arithmetic(registry: &mut PrimitiveRegistry) {
    registry.register(PrimitiveFn {
        name: "+",
        arity: Arity::AtLeast(0),
        help: "Returns the sum of its arguments",
        handler: |eval, args, _tail| add(eval, args).map(EvalResult::Value),
    });

    registry.register(PrimitiveFn {
        name: "-",
        arity: Arity::AtLeast(1),
        help: "Subtracts remaining arguments from the first",
        handler: |eval, args, _tail| subtract(eval, args).map(EvalResult::Value),
    });
    // etc.
}
```

**Benefits:**
- ✅ Runtime extensibility (plugins, user-defined primitives)
- ✅ Auto-generated help system (`(help '+)`)
- ✅ Centralized arity checking
- ✅ Better error messages (include expected arity)
- ✅ Primitives grouped by category (still in separate modules)
- ✅ Enables profiling (count primitive calls)

**Current Implementation:**
- ✅ `PrimitiveRegistry` implemented with full metadata support
- ✅ All 125 R7RS primitives migrated to registry
- ✅ Organized by category with register() functions
- ✅ Arity checking centralized in registry
- ✅ Help text for all primitives
- ✅ Tail call optimization support (in_tail parameter)
- ✅ Library namespacing (scheme.base/+, etc.)
- ✅ Match statement reduced to 9 non-R7RS utilities

**Achievement:** Completed in 2 sessions (2025-11-12 to 2025-11-13)
**Next Steps:** Add help system to utilize metadata, consider plugin API

---

### Issue #3: Special Forms Hardcoded in Evaluator 🔴 HIGH

**Location:** `patina-tree-walker/src/eval/mod.rs:300-450` (approx)

**Problem:**
```rust
// Inside eval_step_impl
match first_value {
    Value::Symbol(s) if s.as_ref() == "quote" => { /* 10 lines */ }
    Value::Symbol(s) if s.as_ref() == "if" => { /* 30 lines */ }
    Value::Symbol(s) if s.as_ref() == "lambda" => { /* 50 lines */ }
    Value::Symbol(s) if s.as_ref() == "define" => { /* 40 lines */ }
    Value::Symbol(s) if s.as_ref() == "set!" => { /* 20 lines */ }
    // ... 20+ more special forms ...
}
```

**Supporting Evidence:**
- `special_forms.rs`: 1078 lines (single file)
- 42KB of special form logic in one module

**Impact:**
- Not extensible - cannot add special forms without modifying evaluator core
- Will be duplicated when adding VM/JIT backends
- Difficult to test individual special forms in isolation
- File size indicates module is "screaming" for decomposition

**Solution:**

```rust
// In patina-tree-walker/src/eval/special_forms/trait.rs (NEW)

pub trait SpecialForm {
    fn name(&self) -> &str;

    fn eval(&self,
            evaluator: &Evaluator,
            args: &Value,           // Arguments after the form name
            env: &Rc<Environment>,
            in_tail: bool) -> Result<EvalResult, EvalError>;
}

pub struct SpecialFormRegistry {
    forms: HashMap<String, Box<dyn SpecialForm>>,
}

// In special_forms/quote.rs
pub struct QuoteForm;

impl SpecialForm for QuoteForm {
    fn name(&self) -> &str { "quote" }

    fn eval(&self, _eval: &Evaluator, args: &Value,
            _env: &Rc<Environment>, _in_tail: bool)
            -> Result<EvalResult, EvalError> {
        // Extracted from current implementation
        let Value::Pair(p) = args else {
            return Err(EvalError::InvalidSyntax("quote requires 1 argument".into()));
        };
        if !matches!(p.1, Value::Null) {
            return Err(EvalError::InvalidSyntax("quote takes exactly 1 argument".into()));
        }
        Ok(EvalResult::Value(p.0.clone()))
    }
}

// In special_forms/mod.rs
pub fn build_registry() -> SpecialFormRegistry {
    let mut registry = SpecialFormRegistry::new();
    registry.register(Box::new(QuoteForm));
    registry.register(Box::new(IfForm));
    registry.register(Box::new(LambdaForm));
    // etc.
    registry
}
```

**File Structure:**
```
special_forms/
├── mod.rs           # Registry + exports
├── trait.rs         # SpecialForm trait definition
├── quote.rs         # QuoteForm
├── if.rs            # IfForm
├── lambda.rs        # LambdaForm
├── define.rs        # DefineForm
├── control.rs       # BeginForm, CondForm, CaseForm
├── binding.rs       # LetForm, LetStarForm, LetrecForm
└── ...
```

**Benefits:**
- Each special form is independently testable
- Clear module boundaries (~50-150 lines per form)
- Easy to add new forms (for DSLs, extensions)
- Better code organization

**Effort:** 4-5 days
**Priority:** HIGH (code quality, maintainability)

---

### Issue #4: Library System - Partial Abstraction ⚠️ MEDIUM

**Location:** `patina-tree-walker/src/eval/mod.rs:77-120`

**Problem:**
```rust
fn init_loaders(&self) {
    let mut rust_loader = RustLibraryLoader::with_standard_libraries();

    // Backend manually registering all libraries!
    rust_loader.register(vec!["scheme", "base"], stdlib::build_scheme_base);
    rust_loader.register(vec!["scheme", "char"], stdlib::build_scheme_char);
    rust_loader.register(vec!["scheme", "complex"], stdlib::build_scheme_complex);
    // ... 10+ more registrations
}
```

**Impact:**
- Backend-specific code mixed with library configuration
- Cannot register libraries without modifying backend
- Will require copy-paste when adding VM/JIT backends
- Violates single responsibility principle

**Solution:**

```rust
// In patina-runtime/src/stdlib/mod.rs

/// Register all R7RS standard libraries with a loader
pub fn register_standard_libraries(loader: &mut RustLibraryLoader) {
    loader.register(vec!["scheme".into(), "base".into()], build_scheme_base);
    loader.register(vec!["scheme".into(), "char".into()], build_scheme_char);
    loader.register(vec!["scheme".into(), "complex".into()], build_scheme_complex);
    loader.register(vec!["scheme".into(), "inexact".into()], build_scheme_inexact);
    // etc. - all in one place
}

// In patina-tree-walker/src/eval/mod.rs
fn init_loaders(&self) {
    use patina_runtime::{RustLibraryLoader, stdlib};

    let mut loaders = self.loader_registry.borrow_mut();

    // Rust libraries
    let mut rust_loader = RustLibraryLoader::new();
    stdlib::register_standard_libraries(&mut rust_loader);
    loaders.add(Box::new(rust_loader));

    // Scheme .sld libraries
    loaders.add(Box::new(SchemeLibraryLoader::new()));
}
```

**Benefits:**
- Library registration lives with library definitions
- Backends just wire up loaders (no knowledge of specific libraries)
- Easy to add/remove libraries in one place
- VM/JIT backends use identical initialization

**Effort:** 2 hours
**Priority:** MEDIUM (quick win, improves organization)

---

### Issue #5: Value/Environment Cyclic References ⚠️ MEDIUM

**Location:** `patina-runtime/src/value/mod.rs:1-50`

**Observation:**
```rust
pub enum Value {
    Integer(i64),
    String(Rc<RefCell<String>>),

    // Executable entities mixed with data
    Lambda {
        params: Vec<String>,
        body: Vec<Value>,
        env: Rc<Environment>,  // ← Environment contains Values!
        variadic: bool,
    },
    Primitive { name: String, arity: Arity },
    Macro { name: String, data: Box<Value> },
}
```

**Circular Dependency:**
- `Value::Lambda` contains `Rc<Environment>`
- `Environment` contains `HashMap<String, Value>`
- Creates potential reference cycles (mitigated by `Rc`, not `Rc<RefCell>`)

**Assessment:**
- **Acceptable for tree-walker** (current implementation is correct)
- **Will cause pain for:**
  - Serializing closures (pickle/marshal)
  - VM bytecode (environments become register frames)
  - Debugging/introspection (pretty-printing closures)
  - Garbage collection (if you add one later)

**Future Consideration:**
When implementing VM backend, consider separating:

```rust
// Pure data (serializable, inspectable)
pub enum RuntimeValue {
    Integer(i64),
    String(Rc<str>),
    Pair(Rc<(RuntimeValue, RuntimeValue)>),
    Closure(ClosureId),  // Opaque reference
    // etc.
}

// Execution context (VM-internal)
pub struct Closure {
    code: CodeObject,      // Bytecode or AST
    env: EnvironmentId,    // Reference to environment table
}
```

**Effort:** Large (VM-level refactor)
**Priority:** LOW (defer until VM implementation)
**Action:** Document for future VM work

---

### Issue #6: Error Types - Backend Leakage ⚠️ MEDIUM

**Location:** Multiple crates

**Problem:**
```rust
// patina-frontend/src/error.rs
pub enum ParseError { ... }
pub enum LexError { ... }

// patina-tree-walker/src/eval/error.rs
pub enum EvalError { ... }  // ← Backend-specific!

// patina-interpreter/src/lib.rs
pub enum InterpreterError {
    Parse(ParseError),
    Eval(EvalError),  // ← Tightly coupled to tree-walker
}
```

**Impact:**
When adding `patina-vm`:
- Need to add `VMError` variant to `InterpreterError`
- Cannot make interpreter generic over backend errors cleanly
- Every backend error type must be explicitly handled

**Solution:**

```rust
// In patina-runtime/src/error.rs (NEW)

/// Backend-agnostic runtime errors
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    #[error("Type error: expected {expected}, got {got}")]
    TypeError {
        expected: &'static str,
        got: String,  // Value::type_name()
    },

    #[error("Arity mismatch: expected {expected}, got {got} arguments")]
    ArityMismatch { expected: String, got: usize },

    #[error("Unbound variable: {0}")]
    UnboundVariable(String),

    #[error("Division by zero")]
    DivisionByZero,

    #[error("Invalid syntax: {0}")]
    InvalidSyntax(String),

    // etc. - all semantic errors
}

// Backend errors wrap runtime errors
#[derive(Debug, thiserror::Error)]
pub enum EvalError {
    #[error(transparent)]
    Runtime(#[from] RuntimeError),

    #[error(transparent)]
    MacroExpansion(#[from] FrontendError),

    #[error("Internal evaluator error: {0}")]
    Internal(String),
}

// Generic interpreter error
#[derive(Debug, thiserror::Error)]
pub enum InterpreterError<E: std::error::Error> {
    #[error(transparent)]
    Parse(#[from] ParseError),

    #[error(transparent)]
    Backend(E),  // Generic over backend error
}

impl<E: std::error::Error + From<RuntimeError>> From<RuntimeError>
    for InterpreterError<E> {
    fn from(e: RuntimeError) -> Self {
        InterpreterError::Backend(E::from(e))
    }
}
```

**Benefits:**
- Shared error taxonomy across backends
- Better error messages (consistent formatting)
- Easier to test error conditions
- Interpreter truly backend-agnostic

**Effort:** 1 day
**Priority:** MEDIUM (improves maintainability)

---

### Issue #7: Testing Architecture - Inverted Dependency ⚠️ MEDIUM

**Location:** `patina-tests/` (separate crate)

**Current State:**
- `patina-tests` imports `patina-interpreter`
- All tests go through high-level API:
  ```rust
  let interp = Interpreter::new();  // ← Hardcoded to tree-walker
  interp.eval_str("(+ 1 2)");
  ```

**Problem:**
When you have 3 backends (tree-walker, VM, JIT), how do you:
- Test each backend in isolation?
- Run compliance suite against all backends?
- Benchmark backend performance differences?

**Current Limitation:**
Cannot run backend-specific tests without coupling to interpreter.

**Solution:**

1. **Move test utilities to runtime:**
   ```rust
   // patina-runtime/src/testing.rs (NEW)
   pub trait TestableBackend {
       fn eval_str(&self, input: &str) -> Result<Value, Box<dyn Error>>;
       fn eval_program(&self, input: &str) -> Result<Value, Box<dyn Error>>;
   }

   // Shared test helpers
   pub fn assert_eval_to<B: TestableBackend>(
       backend: &B,
       expr: &str,
       expected: &str
   ) { ... }
   ```

2. **Each backend has own tests:**
   ```rust
   // crates/patina-tree-walker/src/eval/mod.rs
   #[cfg(test)]
   mod tests {
       use super::*;

       #[test]
       fn test_tree_walker_specific_behavior() {
           let eval = Evaluator::new();
           // Direct backend testing
       }
   }
   ```

3. **Integration tests test through interpreter:**
   ```rust
   // crates/patina-tests/tests/compliance/numbers.rs
   #[test]
   fn test_addition_all_backends() {
       test_all_backends(|interp| {
           assert_eval_to(interp, "(+ 1 2)", "3");
       });
   }

   fn test_all_backends<F>(test: F)
   where F: Fn(&dyn TestableBackend) {
       // Test tree-walker
       test(&Interpreter::new(TreeWalker::new()));

       // Test VM (when implemented)
       test(&Interpreter::new(VM::new()));

       // Test JIT (when implemented)
       test(&Interpreter::new(JIT::new()));
   }
   ```

**Benefits:**
- Backend isolation for unit tests
- Multi-backend compliance testing
- Easy to compare backend behavior
- Clear test organization

**Effort:** 2-3 days
**Priority:** MEDIUM (quality of life improvement)

---

## Architectural Metrics

| Metric | Current | Industry Standard | Assessment |
|--------|---------|-------------------|------------|
| **Crate coupling** | Moderate (4/7 depend on runtime) | Low | ✅ Good |
| **Backend abstraction** | None (0 traits) | 1-2 core traits | 🔴 **Critical** |
| **Primitive extensibility** | Compile-time only | Runtime registry | ⚠️ Needs work |
| **Code duplication** | Low | Minimal | ✅ Good |
| **Largest single module** | 1078 lines | <500 lines | ⚠️ Refactor needed |
| **Test isolation** | Coupled to interpreter | Backend-independent | ⚠️ Medium issue |
| **Trait coverage** | 2 traits (Library, Macro) | 3-4 traits | ⚠️ Needs Backend trait |
| **Public API stability** | Concrete types | Generic/trait-based | ⚠️ Will break on VM |

---

## Recommended Refactoring Roadmap

### Phase 1: Backend Abstraction (CRITICAL) 🎯

**Goal:** Enable multiple backends without modifying interpreter

**Tasks:**
1. Define `Backend` trait in `patina-runtime/src/backend.rs`
2. Create `TreeWalker` wrapper around `Evaluator`
3. Implement `Backend` for `TreeWalker`
4. Make `Interpreter` generic: `Interpreter<B: Backend>`
5. Add type alias: `type TreeWalkInterpreter = Interpreter<TreeWalker>`
6. Update `patina-repl` to use `TreeWalkInterpreter::new()`

**Validation:**
- All existing tests pass
- Can create `Interpreter<MockBackend>` in tests
- No breaking changes to REPL binary

**Effort:** 1-2 days
**Priority:** CRITICAL
**Blocks:** VM/JIT implementation

---

### Phase 2: Primitive Registry (HIGH VALUE) 🎯

**Goal:** Runtime-extensible primitives with introspection

**Tasks:**
1. Create `PrimitiveRegistry` in `patina-tree-walker/src/eval/primitives/registry.rs`
2. Define `PrimitiveFn` struct with metadata
3. Convert each category module to register functions:
   - `arithmetic::register(registry)`
   - `lists::register(registry)`
   - `strings::register(registry)`, etc.
4. Replace match statement with registry lookup
5. Add `Evaluator::list_primitives()` for help system
6. Centralize arity checking in registry

**Validation:**
- All primitive tests pass
- Can add primitive at runtime
- `(help '+)` works (stretch goal)

**Effort:** 3-4 days
**Priority:** HIGH
**Enables:** Plugin system, better UX, auto-documentation

---

### Phase 3: Special Form Decomposition (CODE QUALITY) 🎯

**Goal:** Modular, testable special forms

**Tasks:**
1. Define `SpecialForm` trait
2. Extract each form to dedicated file:
   - `quote.rs`, `if.rs`, `lambda.rs`, `define.rs`, etc.
3. Create `SpecialFormRegistry`
4. Update evaluator to dispatch through registry
5. Split `special_forms.rs` (1078 lines) into directory structure

**Validation:**
- All special form tests pass
- Each form has isolated unit tests
- No behavioral changes

**Effort:** 4-5 days
**Priority:** MEDIUM
**Improves:** Maintainability, testability

---

### Phase 4: Error Taxonomy (MAINTAINABILITY) 🔧

**Goal:** Backend-agnostic error types

**Tasks:**
1. Define `RuntimeError` in `patina-runtime/src/error.rs`
2. Make backend errors wrap `RuntimeError`
3. Generic `InterpreterError<E>` over backend error type
4. Improve error messages with context
5. Update error tests

**Validation:**
- Error messages at least as good as before
- No error information lost
- Backends can add custom error variants

**Effort:** 1-2 days
**Priority:** MEDIUM

---

### Quick Wins (Low-Hanging Fruit) ⚡

These can be done independently in <1 day each:

1. **Move stdlib registration** (Issue #4)
   - Extract library registration from evaluator
   - **Effort:** 2 hours
   - **File:** `patina-runtime/src/stdlib/mod.rs`

2. **Split special_forms.rs**
   - Create `special_forms/` directory
   - Move each form to own file (no trait yet)
   - **Effort:** 4 hours
   - **Benefit:** Immediate readability improvement

3. **Add primitive arity metadata**
   - Define `PRIMITIVE_ARITIES` map
   - Check before dispatch
   - **Effort:** 2 hours
   - **Benefit:** Better error messages

4. **Create patina-backend crate stub**
   - Just the `Backend` trait (no implementations)
   - **Effort:** 1 hour
   - **Benefit:** Documents architecture direction

---

## Long-Term Considerations

### When Implementing VM Backend

**Separation of Data and Code:**
Consider splitting `Value` into:
- `RuntimeValue` - pure data (serializable)
- `Executable` - code + context (VM-internal)

This will help with:
- Bytecode serialization
- Debugging/introspection
- Garbage collection (if added)

**See Issue #5 for details.**

---

### When Adding Gradual Typing (Phase 2)

The current `Value` enum will need type annotations. Consider:

```rust
pub enum Value {
    // Annotated values
    Typed(Box<Value>, TypeAnnotation),

    // Existing variants
    Integer(i64),
    // ...
}
```

The `Backend` trait will need extension:

```rust
pub trait Backend {
    type Error;

    // New method for type-checking
    fn typecheck(&self, expr: &Value) -> Result<TypeAnnotation, Self::Error>;

    // Existing methods
    fn eval(&self, expr: &Value, env: &Rc<Environment>)
        -> Result<Value, Self::Error>;
}
```

---

### Module System Considerations

R7RS libraries are partially implemented. Future work needed:
- Import/export scoping (currently basic)
- Conditional exports
- Library versioning
- Separate compilation

The `LibraryLoader` trait is designed for this. No architectural changes needed.

---

## Migration Strategy

### Backward Compatibility

All refactorings should maintain:
- ✅ Existing REPL binary works
- ✅ All tests pass
- ✅ Public API unchanged (or extended with deprecation)

### Phased Rollout

**Phase 1 can be done without breaking changes:**
- Add `Backend` trait alongside existing concrete API
- Provide `TreeWalkInterpreter` type alias
- Deprecate direct `Evaluator` access

**Phases 2-4 are internal refactors:**
- No public API changes
- Purely improve maintainability

---

## Conclusion

**Current State:** Patina has a **strong foundation** with clean crate boundaries and sound implementation patterns. The tree-walker is production-ready for R7RS compliance.

**Key Insight:** The codebase is structured like a **modular monolith** when it needs to be a **pluggable framework**. The pieces are well-organized; they just need one more layer of indirection (trait boundaries).

**Path Forward:** The recommended refactoring roadmap is **incremental and safe**. Each phase delivers value independently:
- Phase 1 unblocks VM/JIT work
- Phase 2 enables plugins and better UX
- Phase 3 improves code quality
- Phase 4 future-proofs error handling

**Estimated Total Effort:** 10-15 days of focused work across all phases.

**Risk Assessment:** LOW - refactorings are architectural, not algorithmic. The evaluation logic is sound and doesn't need changes, just better organization.

**Recommendation:** Prioritize Phase 1 (Backend trait) before implementing VM. The other phases can be done opportunistically during feature work.

---

## References

**Related Documents:**
- `PRD/MULTI_BACKEND_STRATEGY.md` - Original multi-backend vision
- `docs/DEVELOPMENT.md` - Developer guide
- `docs/TEST_ORGANIZATION.md` - Test structure

**Key Source Files:**
- `crates/patina-interpreter/src/lib.rs` - High-level API
- `crates/patina-tree-walker/src/eval/mod.rs` - Evaluator core
- `crates/patina-runtime/src/value/mod.rs` - Value representation
- `crates/patina-runtime/src/library_loader.rs` - Library loading traits

**Codebase Stats:**
- Total LOC: ~16,700
- Rust files: 89
- Crates: 7
- Tests: 435 (392 passing, 43 ignored)
- R7RS Compliance: ~53% (68/129 chibi tests passing)
