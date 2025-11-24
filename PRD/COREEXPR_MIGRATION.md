# CoreExpr Migration Strategy

**Status**: In Progress
**Last Updated**: 2025-11-23

## Overview

Patina is migrating from a dual-evaluation system (Value evaluator + CoreExpr evaluator) to a unified CoreExpr-based evaluation pipeline. This document outlines the migration strategy for the remaining special forms.

## Current Status

### ✅ Migrated to CoreExpr (8 forms)
All core R7RS special forms have been successfully migrated:
- `quote` - Literal values and symbolic data
- `if` - Conditional branching
- `begin` - Sequential evaluation
- `define` - Top-level bindings
- `set!` - Assignment
- `lambda` - Procedure creation
- `apply` - Function application
- `quasiquote` - Template construction with unquoting
- `define-syntax` - Macro definitions
- `import` - Library imports
- `parameterize` - Dynamic parameters

**Benefits achieved:**
- Unified evaluation path reduces code duplication
- Better TCO support through CoreExpr
- Cleaner separation between parsing/desugaring and evaluation
- Macro expansion integrated into desugaring phase

### ⚠️ Pending Migration (4 forms)

#### 1. `let-syntax` and `letrec-syntax`
**Status**: Deferred ([GitHub issue needed](https://github.com/avalonalex/patina/issues/15))
**Complexity**: High
**Reason for deferral**: Complex scoping semantics

**Current implementation**: Value evaluator special forms
- `let-syntax`: Local macro bindings (macros can't reference each other)
- `letrec-syntax`: Recursive local macro bindings

**Migration challenges**:
1. **Scoping complexity**: Macros introduce new bindings that are visible during expansion but not at runtime
2. **Expansion environment**: Need to track separate environments for macro expansion vs runtime
3. **Interaction with other forms**: Nested `let-syntax` inside `define`, `lambda`, etc.

**Migration path**:
1. **Add CoreExpr variants**:
   ```rust
   CoreExpr::LetSyntax {
       bindings: Vec<(Symbol, Value)>, // (name, transformer) - keep as Value
       body: Vec<CoreExpr>,
   }
   CoreExpr::LetrecSyntax {
       bindings: Vec<(Symbol, Value)>,
       body: Vec<CoreExpr>,
   }
   ```

2. **Desugarer changes**:
   - Create new environment with macro bindings
   - Desugar body expressions in macro-aware environment
   - Store transformers as `Value` (template data, not code)

3. **Evaluator changes**:
   - Evaluate body in sequence (like `begin`)
   - Macros are compile-time only, no runtime representation

**Estimated effort**: Medium-High (2-3 days)

**Known issues to fix**:
- Currently, forms containing nested `let-syntax` fail because:
  - CoreExpr desugarer encounters `let-syntax` and returns `FallbackFormNeeded`
  - Outer form (e.g., `define`) can't be desugared
  - Value evaluator can't handle it because `define` is no longer in registry
- Debug output shows: `⚠️ FALLBACK: CoreExpr desugaring failed due to nested fallback form`

#### 2. `case-lambda`
**Status**: Not yet in CoreExpr
**Complexity**: Medium
**Library**: `(scheme case-lambda)`

**Current implementation**: Value evaluator special form
- Allows multiple clauses with different arities
- Dispatches based on number of arguments

**Migration path**:
1. **Add CoreExpr variant**:
   ```rust
   CoreExpr::CaseLambda {
       clauses: Vec<CaseLambdaClause>,
   }

   struct CaseLambdaClause {
       params: Formals,
       body: Vec<CoreExpr>,
   }
   ```

2. **Desugarer changes**:
   - Parse each clause: `((formals) body ...)`
   - Validate no duplicate arities
   - Desugar each body to `Vec<CoreExpr>`

3. **Evaluator changes**:
   - Create `Value::Procedure::CaseLambda` with clauses
   - Application logic tries each clause in order
   - First matching arity is used

**Estimated effort**: Low-Medium (1-2 days)

#### 3. `expand` (Patina debugging extension)
**Status**: Patina-specific, not R7RS
**Complexity**: Low
**Library**: `(patina debug)`

**Current implementation**: Value evaluator special form
- Expands macros without evaluating
- Debugging tool for macro development

**Strategic question**: Should Patina-specific forms be in CoreExpr?

**Options**:

**Option A: Migrate to CoreExpr**
- **Pros**: Consistent with unified evaluation model
- **Cons**: CoreExpr is meant to be language-neutral IR; Patina extensions pollute it

**Option B: Keep in Value evaluator**
- **Pros**: Separates language core from extensions
- **Cons**: Requires maintaining Value evaluator indefinitely

**Option C: Special "extension" mechanism**
- **Pros**: Clean separation; extensible for future features
- **Cons**: More complex architecture

**Recommended**: **Option A** (Migrate to CoreExpr)
- CoreExpr is Patina's IR, not a universal IR
- Better to have one evaluation path than two
- Future Patina extensions (gradual typing, reactive streams) will need CoreExpr anyway

**Migration path**:
1. **Add CoreExpr variant**:
   ```rust
   CoreExpr::Expand {
       expr: Box<CoreExpr>,
   }
   ```

2. **Evaluator changes**:
   - Expand macros in the expression
   - Return the expanded form as a `Value` (for display)
   - Don't evaluate the result

**Estimated effort**: Low (few hours)

## Migration Priority

### Phase 1: Quick wins (1 week)
1. ✅ **Macro expansion routing** - Route macro-expanded code through CoreExpr
2. ✅ **Lambda body evaluation** - Use CoreExpr for lambda bodies
3. ✅ **Library loading** - Use CoreExpr for loading .scm files

### Phase 2: Extension forms (1 week)
1. **`expand`** - Simple debugging form
2. **`case-lambda`** - Standard library form, well-defined semantics

### Phase 3: Complex forms (2-3 weeks)
1. **`let-syntax`** - Complex scoping, high priority due to test failures
2. **`letrec-syntax`** - Similar to `let-syntax`, can be done together

## Handling Future Patina Extensions

### Strategy: CoreExpr as Patina's IR

**Decision**: CoreExpr is Patina's intermediate representation, not a universal IR.
- Patina-specific features belong in CoreExpr
- This includes future extensions:
  - Gradual typing annotations
  - Reactive stream primitives
  - Logic programming constructs (miniKanren)
  - Debugging/profiling forms

### Extension Categories

#### 1. Debugging Forms (Library: `(patina debug)`)
- `expand` - Macro expansion
- Future: `trace`, `break`, `profile`
- Strategy: Add to CoreExpr, marked as debug-only

#### 2. Typing Forms (Library: `(patina types)`)
- Future: `define-type`, `cast`, `assert-type`
- Strategy: Add to CoreExpr with type annotations

#### 3. Reactive Forms (Library: `(patina reactive)`)
- Future: `stream`, `observable`, `subscribe`
- Strategy: Add to CoreExpr with special evaluation semantics

#### 4. Logic Forms (Library: `(patina logic)`)
- Future: `run`, `fresh`, `conde`
- Strategy: Add to CoreExpr, may need separate evaluation mode

## Testing Strategy

### Current Test Results
- **Cargo tests**: 802 passing, 4 failing (let-syntax with nested forms)
- **Chibi tests**: 88 passing (up from 0 after macro routing fix)

### Test Requirements for Migration
For each form migrated to CoreExpr:
1. All existing Value evaluator tests must pass
2. Add CoreExpr-specific tests (TCO, nested contexts)
3. Parity tests comparing Value vs CoreExpr results
4. Integration tests with other CoreExpr forms

### Known Issues
- Forms with nested `let-syntax` fail because:
  - Desugarer can't handle nested fallback forms
  - Value evaluator missing `define` and other core forms
- **Fix**: Migrate `let-syntax` to CoreExpr (eliminates nesting issue)

## Architecture Notes

### Current Routing Logic

**Evaluation flow** (as of 2025-11-23):
```
Expression
  ↓
Backend::eval (backend.rs)
  ↓
Desugarer::with_env(env) - macro-aware
  ↓
Desugarer::desugar(expr)
  ├─ Success → eval_core(CoreExpr) ✅ PRIMARY PATH
  └─ FallbackFormNeeded → eval_in_env(Value) ⚠️ FALLBACK PATH
```

**Special cases**:
1. **Macro expansion** (eval/mod.rs:643-667):
   - Expanded code routes through CoreExpr
   - Ensures `(test 1 1)` → `(define epsilon ...)` works

2. **Lambda bodies** (eval/mod.rs:1149-1180):
   - Use `eval_with_core_routing` method
   - Handles internal defines correctly

3. **Library loading** (eval/mod.rs:358-392):
   - Each expression in .scm file goes through CoreExpr
   - Enables bootstrapping with CoreExpr forms

### Debug Output

When fallback occurs:
```
⚠️  FALLBACK: CoreExpr desugaring failed due to nested fallback form
   Expression: (define result (let-syntax ...))
   Reason: let-syntax requires Value evaluator (not yet in CoreExpr)
   → Falling through to Value evaluator (may fail if form not in registry)
```

This helps identify:
- Which expressions trigger fallback
- Why desugaring failed
- Where to focus migration efforts

## Success Criteria

### Short-term (Complete Phase 2)
- [ ] `expand` migrated to CoreExpr
- [ ] `case-lambda` migrated to CoreExpr
- [ ] All cargo tests passing (including let-syntax tests with workaround)
- [ ] Chibi tests > 100 passing

### Medium-term (Complete Phase 3)
- [ ] `let-syntax` migrated to CoreExpr
- [ ] `letrec-syntax` migrated to CoreExpr
- [ ] All 4 failing let-syntax tests passing
- [ ] Value evaluator special form registry empty
- [ ] Chibi tests > 200 passing

### Long-term (Value evaluator removal)
- [ ] All special forms in CoreExpr
- [ ] Value evaluator only used for:
  - AST representation
  - Runtime values
  - Not evaluation
- [ ] Clean separation: Frontend → CoreExpr → Evaluation

## Related Documents

- **Implementation status**: `PRD/phase1/IMPLEMENTATION_STATUS.md`
- **Test organization**: `docs/TEST_ORGANIZATION.md`
- **Feature matrix**: `docs/FEATURE_STATUS.md`

## Open Questions

1. **Should we keep Value evaluator for future use cases?**
   - Possible use: REPL evaluation of runtime-generated code
   - Possible use: `eval` procedure implementation
   - Decision: Revisit after all forms migrated

2. **How to handle library-specific forms?**
   - Example: `case-lambda` is in `(scheme case-lambda)`, not `(scheme base)`
   - Should CoreExpr know about library boundaries?
   - Decision: CoreExpr is library-agnostic; all forms available

3. **Performance implications of routing everything through CoreExpr?**
   - Desugaring overhead for every expression
   - Need benchmarks comparing Value vs CoreExpr paths
   - Decision: Defer optimization until migration complete
