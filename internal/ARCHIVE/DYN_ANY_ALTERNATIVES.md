# Avoiding `dyn Any` in Patina

> **Status: ✅ FULLY RESOLVED (2025-11-25)**
>
> All `dyn Any` usages have been completely eliminated:
> - Lambda body storage resolved with `LambdaBody` enum in `patina-core`
> - Macro storage resolved with `CompiledMacro` moved to `patina-core`
> - Compiler environment now uses `Rc<Environment>` directly
>
> **Zero `dyn Any` remains in the codebase.**

This document analyzes the uses of `dyn Any` that existed in Patina and documents how they were resolved.

## Resolved `dyn Any` Usage

### 1. Macro Storage in `Value::Macro` - ✅ RESOLVED

**Solution implemented:** `CompiledMacro` moved to `patina-core/src/compiled_macro.rs`

```rust
// In patina-core/src/value.rs
Macro(Rc<CompiledMacro>),  // Type-safe, no dyn Any!
```

By placing `CompiledMacro` (and its dependencies `Pattern`, `Template`, `Identifier`) in the
`patina-core` foundation crate, we avoided the circular dependency and achieved type safety.
The `patina-macros` crate now imports these types from `patina-core` via `patina-runtime`.

### 2. Lambda Body Storage - ✅ RESOLVED

**Solution implemented:** `LambdaBody` enum in `patina-core/src/value.rs`

```rust
pub enum LambdaBody {
    Values(Vec<Value>),   // Legacy: body as syntax Values
    Core(Vec<CoreExpr>),  // Optimized: body as CoreExpr
}

Lambda {
    body: LambdaBody,  // Type-safe, no dyn Any!
    // ...
},
```

By placing `CoreExpr` in the `patina-core` foundation crate, we avoided the circular dependency and achieved type safety.

### 3. Compiler Environment - ✅ RESOLVED

**Solution implemented:** Changed `Compiler::env` from `Rc<dyn Any>` to `Rc<Environment>`

```rust
// In patina-macros/src/macro_expander/compiler.rs
env: Option<Rc<Environment>>,  // Type-safe, no dyn Any!
```

Since `Environment` is already in `patina-core`, and `patina-macros` already depends on
`patina-runtime` (which re-exports `patina-core`), we can use `Rc<Environment>` directly.
The simplified code no longer needs `downcast_ref()` calls

---

## Problems with `dyn Any`

1. **No compile-time type safety** - Downcasts can fail at runtime
2. **Boilerplate** - Every access requires `downcast_ref::<T>()` and error handling
3. **Hidden dependencies** - The actual type is implicit, not visible in the API
4. **Refactoring hazard** - Changing the stored type doesn't produce compile errors
5. **Documentation burden** - Must document expected types in comments

---

## Alternative Approaches

### Option 1: Move `CoreExpr` to `patina-runtime`

**Approach:** Since `CoreExpr` is fundamentally about runtime value representation, move it from `patina-ir` to `patina-runtime`.

**New dependency graph:**
```
patina-runtime (Value, Environment, CoreExpr)
       ↑
patina-ir (visitor patterns, IR passes)
       ↑
patina-frontend (parser, desugarer)
       ↑
patina-macros
       ↑
patina-tree-walker
```

**Changes required:**
1. Move `crates/patina-ir/src/core_expr.rs` to `crates/patina-runtime/src/core_expr.rs`
2. Keep `visitor.rs` in `patina-ir` (it's about IR transformation passes)
3. Update all imports

**Pros:**
- `Procedure::Lambda` can store `Vec<CoreExpr>` directly
- Type-safe, no downcasting
- Natural fit since CoreExpr contains `Value`

**Cons:**
- Larger `patina-runtime` crate
- Mixes "data types" with "IR" concepts

### Option 2: Create `patina-core` Foundation Crate

**Approach:** Extract shared types into a foundation crate that everyone depends on.

**New crate structure:**
```
patina-core (Value, Environment, CoreExpr, ScopeSet)
       ↑
patina-runtime (Backend trait, Library system)
patina-ir (visitor patterns, passes)
patina-macros (CompiledMacro)
       ↑
patina-frontend (parser, desugarer)
       ↑
patina-tree-walker (evaluator)
```

**Changes required:**
1. Create new `crates/patina-core/` crate
2. Move `Value`, `Environment`, `CoreExpr`, `ScopeId`, `ScopeSet` there
3. All crates depend on `patina-core`
4. `patina-runtime` becomes thinner (just Backend, Library system)

**Pros:**
- Clean separation of concerns
- No circular dependencies possible
- All shared types in one place
- `Lambda` can store `Vec<CoreExpr>` directly

**Cons:**
- More crates to manage
- Significant refactoring effort
- Need to decide what goes where

### Option 3: Type-Erased Wrapper with Trait

**Approach:** Instead of raw `dyn Any`, use a trait that provides the needed interface.

```rust
// In patina-runtime
pub trait LambdaBody: std::fmt::Debug {
    fn evaluate(&self, env: &Rc<Environment>, evaluator: &dyn Evaluator)
        -> Result<Value, EvalError>;
    fn clone_box(&self) -> Box<dyn LambdaBody>;
}

// Procedure stores trait object
Lambda {
    body: Vec<Value>,           // Fallback
    body_typed: Option<Box<dyn LambdaBody>>,  // Type-erased but with interface
}
```

**Pros:**
- Defined interface instead of arbitrary `Any`
- Can add methods as needed
- Still allows different implementations

**Cons:**
- Still runtime dispatch
- Need to implement trait for each body type
- More complex than direct storage

### Option 4: Generic `Procedure<B>` Type

**Approach:** Make `Procedure` generic over the body type.

```rust
pub enum Procedure<Body = Vec<Value>> {
    Primitive { ... },
    Lambda {
        params: Vec<String>,
        variadic: Option<String>,
        body: Body,
        env: Rc<Environment>,
        binding_scope: Option<ScopeId>,
    },
    CaseLambda { ... },
    Continuation,
}

// Type aliases for common uses
pub type ValueProcedure = Procedure<Vec<Value>>;
pub type CoreProcedure = Procedure<Vec<CoreExpr>>;
```

**Pros:**
- Fully type-safe
- No `dyn Any` needed
- Flexible for different use cases

**Cons:**
- Generic parameter propagates everywhere
- `Value` enum would need to be generic too
- Significant API changes

### Option 5: Enum with All Body Types

**Approach:** Define an enum that covers all possible body representations.

```rust
pub enum LambdaBody {
    /// Legacy: body as syntax values (requires desugaring)
    Values(Vec<Value>),
    /// Optimized: body as CoreExpr (no re-desugaring)
    CoreExprs(Vec<CoreExpr>),
    /// Future: body as bytecode
    Bytecode(Vec<u8>),
}

Lambda {
    body: LambdaBody,
    // ...
}
```

**Pros:**
- Type-safe, no downcasting
- Explicit about what body types exist
- Easy to extend

**Cons:**
- `patina-runtime` must know about `CoreExpr`
- Requires moving `CoreExpr` or creating shared crate

---

## Recommended Approach

### Short-term: Option 5 (Enum) + Option 1 (Move CoreExpr)

1. Move `CoreExpr` to `patina-runtime`
2. Create `LambdaBody` enum in `patina-runtime`
3. Update `Procedure::Lambda` to use `LambdaBody`

**Migration steps:**

```rust
// Step 1: Move CoreExpr to patina-runtime/src/core_expr.rs
// Keep patina-ir for visitor patterns and passes

// Step 2: Define LambdaBody enum
pub enum LambdaBody {
    Values(Vec<Value>),
    CoreExprs(Vec<CoreExpr>),
}

// Step 3: Update Procedure::Lambda
Lambda {
    params: Vec<String>,
    variadic: Option<String>,
    body: LambdaBody,  // Replaces body + body_core
    env: Rc<Environment>,
    binding_scope: Option<ScopeId>,
}
```

### Long-term: Option 2 (Foundation Crate)

For a cleaner architecture, create `patina-core`:

```
crates/
├── patina-core/        # Value, Environment, CoreExpr, Scope types
├── patina-runtime/     # Backend, Library system, primitives
├── patina-ir/          # Visitor patterns, IR passes
├── patina-macros/      # Macro compilation and expansion
├── patina-frontend/    # Lexer, Parser, Desugarer
├── patina-tree-walker/ # Tree-walking evaluator
├── patina-pipeline/    # Pipeline orchestration
├── patina-interpreter/ # High-level API
├── patina-repl/        # Interactive REPL
└── patina-tests/       # Integration tests
```

---

## Handling the Macro `dyn Any`

For `Value::Macro { data: Rc<dyn Any> }`, the solution is different since `CompiledMacro` is truly a separate concern.

**Options:**

1. **Keep `dyn Any`** - Macros are a special case where type erasure makes sense
2. **Move CompiledMacro to patina-core** - If we create the foundation crate
3. **Use a trait** - `trait MacroTransformer { fn expand(...) -> Value; }`

The macro case is less problematic because:
- It's only used in one place (macro expansion)
- The downcast always succeeds (we control all creation points)
- It's a natural boundary between macro system and runtime

---

## Summary

| Approach | Effort | Type Safety | Recommended For |
|----------|--------|-------------|-----------------|
| Move CoreExpr to runtime | Low | Full | Short-term fix for Lambda body |
| Create patina-core | High | Full | Long-term architecture |
| Trait wrapper | Medium | Partial | Interface-focused designs |
| Generic Procedure | High | Full | Not recommended (too invasive) |
| Body enum | Low | Full | Best combined with Option 1 |

**Immediate recommendation:** Move `CoreExpr` to `patina-runtime` and use the `LambdaBody` enum. This removes `dyn Any` from `Procedure::Lambda` with minimal disruption.

**Future consideration:** Create `patina-core` as part of a larger architecture cleanup, potentially when adding VM/JIT backends.
