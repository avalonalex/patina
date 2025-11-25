# Patina Core Crate Design

> **Status: ✅ COMPLETE (2025-11-25)**
>
> This design has been fully implemented. See `internal/MILESTONES.md` for details.

## Overview

This document proposes creating a `patina-core` foundation crate to eliminate `dyn Any` usage and create a cleaner architecture.

## Current Dependency Graph

```
patina-runtime (no patina deps) ─────────────────────────┐
       ↑                                                  │
patina-ir (runtime)                                       │
patina-macros (runtime)                                   │
       ↑                                                  │
patina-frontend (runtime, ir, macros)                     │ All depend on runtime
       ↑                                                  │
patina-tree-walker (runtime, frontend, macros, ir)        │
       ↑                                                  │
patina-pipeline (tree-walker, runtime, frontend)          │
       ↑                                                  │
patina-interpreter (pipeline)                             │
       ↑                                                  │
patina-repl (interpreter)                                 │
patina-tests (interpreter) ──────────────────────────────┘
```

## The Problem

**Circular dependency issue:**
- `patina-runtime` defines `Value` and `Procedure::Lambda`
- `patina-ir` defines `CoreExpr` which contains `Value`
- `Procedure::Lambda` needs to store `Vec<CoreExpr>` for hygiene
- But `patina-runtime` can't depend on `patina-ir` (cycle!)

**Current workaround:** `body_core: Option<Rc<dyn Any>>` in `Procedure::Lambda`

## Proposed Solution

Create `patina-core` as the foundation crate containing all shared types:

```
patina-core (no deps) ────────────────────────────────────┐
       ↑                                                   │
patina-runtime (core) - Backend, Library system            │
patina-ir (core) - Visitor patterns, IR passes             │
patina-macros (core) - Macro compilation/expansion         │ All depend on core
       ↑                                                   │
patina-frontend (core, ir, macros)                         │
       ↑                                                   │
patina-tree-walker (core, runtime, frontend, macros, ir)   │
       ↑                                                   │
... rest unchanged ...                                    ─┘
```

## What Goes Into `patina-core`

### Definitely Move (Core Data Types)

| Type | Current Location | Reason |
|------|-----------------|--------|
| `Value` | patina-runtime/value.rs | Used everywhere |
| `Procedure` | patina-runtime/value.rs | Part of Value |
| `CaseLambdaClause` | patina-runtime/value.rs | Part of Procedure |
| `Arity` | patina-runtime/value.rs | Part of Procedure |
| `PromiseState` | patina-runtime/value.rs | Part of Value |
| `Environment` | patina-runtime/environment.rs | Used by Value (closures) |
| `ScopedBinding` | patina-runtime/environment.rs | Used by Environment |
| `ScopeId` | patina-runtime/scope.rs | Used by Value, Environment |
| `ScopeSet` | patina-runtime/scope.rs | Used by Value, Environment |
| `CoreExpr` | patina-ir/core_expr.rs | **Key move** - enables typed Lambda body |
| `Formals` | patina-ir/core_expr.rs | Part of CoreExpr |
| `Symbol` | patina-ir/core_expr.rs | Type alias, used widely |
| `PVRef` | patina-runtime/pvref.rs | Used by macros and core |
| `MatchValue` | patina-runtime/pvref.rs | Used by macros |
| `MatchEnv` | patina-runtime/pvref.rs | Used by macros |

### Stay in `patina-runtime` (Runtime Services)

| Type | Reason |
|------|--------|
| `Backend` trait | Evaluation abstraction |
| `Library` | Library representation |
| `LibraryRegistry` | Library management |
| `LibraryLoader` | Library loading |
| `RustLibraryLoader` | Rust library support |
| `RuntimeError` | Runtime-specific errors |
| `stdlib/*` | Standard library implementations |
| `macro_debug` | Debug utilities |

### Stay in `patina-ir` (IR Utilities)

| Type | Reason |
|------|--------|
| `ExprVisitor` | IR transformation trait |
| `SurfaceSyntax` | Surface syntax helpers |
| `map_children()` | IR traversal helpers |

## New `Procedure::Lambda` Definition

With `CoreExpr` in `patina-core`, we can properly type the body:

```rust
// In patina-core/src/value.rs

/// Body representation for Lambda procedures
#[derive(Debug, Clone)]
pub enum LambdaBody {
    /// Legacy: body as syntax Values (requires desugaring on each call)
    Values(Vec<Value>),
    /// Optimized: body as CoreExpr (preserves scope IDs for hygiene)
    Core(Vec<CoreExpr>),
}

#[derive(Debug, Clone)]
pub enum Procedure {
    Primitive { ... },

    Lambda {
        params: Vec<String>,
        variadic: Option<String>,
        body: LambdaBody,           // <-- Type-safe, no dyn Any!
        env: Rc<Environment>,
        binding_scope: Option<ScopeId>,
    },

    CaseLambda {
        clauses: Vec<CaseLambdaClause>,
        env: Rc<Environment>,
    },

    Continuation,
}
```

## File Structure

```
crates/patina-core/
├── Cargo.toml
└── src/
    ├── lib.rs              # Re-exports
    ├── value.rs            # Value, Procedure, LambdaBody, Arity
    ├── environment.rs      # Environment, ScopedBinding
    ├── scope.rs            # ScopeId, ScopeSet
    ├── core_expr.rs        # CoreExpr, Formals, Symbol, CaseLambdaClause
    └── pvref.rs            # PVRef, MatchValue, MatchEnv
```

## Cargo.toml for `patina-core`

```toml
[package]
name = "patina-core"
version = "0.1.0"
edition = "2021"
description = "Core types for Patina Scheme interpreter"

[dependencies]
num-bigint = "0.4"
num-rational = "0.4"
num-traits = "0.2"
```

Note: Minimal dependencies - only what's needed for numeric types in `Value`.

## Migration Plan

### Phase 1: Create patina-core (Non-Breaking)

1. Create `crates/patina-core/` with empty lib.rs
2. Add to workspace in root `Cargo.toml`
3. Copy types from `patina-runtime` and `patina-ir` (don't move yet)
4. Verify it compiles standalone

### Phase 2: Migrate patina-runtime

1. Add `patina-core` dependency to `patina-runtime`
2. Re-export core types: `pub use patina_core::{Value, Environment, ...}`
3. Remove duplicated type definitions
4. Update internal uses to use re-exports
5. Run tests - should pass (API unchanged)

### Phase 3: Migrate patina-ir

1. Add `patina-core` dependency to `patina-ir`
2. Remove `CoreExpr` definition (now in core)
3. Re-export: `pub use patina_core::{CoreExpr, Formals, Symbol}`
4. Keep visitor patterns in `patina-ir`
5. Run tests

### Phase 4: Update Other Crates

1. `patina-macros`: Add core dependency, use core types
2. `patina-frontend`: Update imports
3. `patina-tree-walker`: Update imports
4. Run full test suite

### Phase 5: Remove dyn Any

1. Change `Procedure::Lambda::body_core` to `body: LambdaBody`
2. Update `core_eval.rs` to use `LambdaBody::Core`
3. Update `apply_procedure` to match on `LambdaBody`
4. Remove `dyn Any` imports
5. Run tests

### Phase 6: Clean Up

1. Remove deprecated re-exports after deprecation period
2. Update documentation
3. Update CLAUDE.md with new architecture

## API Compatibility

To maintain backwards compatibility during migration:

```rust
// In patina-runtime/src/lib.rs

// Re-export everything from core (backwards compatible)
pub use patina_core::*;

// Keep runtime-specific types
pub use backend::Backend;
pub use library::Library;
// ...
```

Crates depending on `patina-runtime` continue to work unchanged.

## Benefits

1. **Type Safety**: No more `dyn Any` for Lambda bodies
2. **Clear Architecture**: Foundation types separated from runtime services
3. **Faster Compilation**: `patina-core` rarely changes, good caching
4. **Future-Proof**: Easy to add VM/JIT backends that share core types
5. **Documentation**: Clear boundary between "data" and "services"

## Risks and Mitigations

| Risk | Mitigation |
|------|------------|
| Breaking downstream code | Re-exports maintain API compatibility |
| Increased complexity | Actually reduces complexity (clearer deps) |
| Migration effort | Phased approach, tests at each step |
| Merge conflicts | Do in single PR, coordinate with any parallel work |

## Estimated Effort

| Phase | Effort | Risk |
|-------|--------|------|
| Phase 1: Create core | 1-2 hours | Low |
| Phase 2: Migrate runtime | 2-3 hours | Low |
| Phase 3: Migrate IR | 1 hour | Low |
| Phase 4: Update crates | 2-3 hours | Medium |
| Phase 5: Remove dyn Any | 1-2 hours | Medium |
| Phase 6: Clean up | 1 hour | Low |
| **Total** | **8-12 hours** | |

## Open Questions

1. **Should `CompiledMacro` move to core?**
   - Pro: Removes another `dyn Any` (in `Value::Macro`)
   - Con: Macro compilation is complex, might bloat core
   - Recommendation: Keep in `patina-macros`, `dyn Any` is acceptable here

2. **Should `RuntimeError` move to core?**
   - Pro: Errors are foundational
   - Con: Runtime errors are runtime-specific
   - Recommendation: Keep separate error types per crate

3. **Naming: `patina-core` vs `patina-types` vs `patina-foundation`?**
   - `patina-core` - Clear, matches common Rust conventions
   - `patina-types` - More specific but less idiomatic
   - Recommendation: `patina-core`

## Conclusion

Creating `patina-core` is a clean architectural improvement that:
- Eliminates `dyn Any` from `Procedure::Lambda`
- Creates clear separation between data types and services
- Sets up the codebase for future VM/JIT backends
- Has low risk with phased migration approach

The effort is moderate (8-12 hours) with high payoff in code quality and maintainability.
