# CoreExpr Migration Strategy

**Status**: Phase 2 Complete ✅ (Phase 3 Deferred)
**Last Updated**: 2025-11-24

## Overview

Patina is migrating from a dual-evaluation system (Value evaluator + CoreExpr evaluator) to a unified CoreExpr-based evaluation pipeline. This document outlines the migration strategy for the remaining special forms.

## Current Status

### ✅ Phase 3 COMPLETE - All Forms Migrated! (15 forms)

**Status**: 100% CoreExpr Migration Complete ✅ (2025-11-24)

All R7RS special forms have been successfully migrated to CoreExpr:

**Core R7RS forms:**
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

**Phase 2 additions (2025-11-24):**
- `expand` - Patina debugging extension for macro inspection
- `case-lambda` - R7RS multi-arity procedure dispatch

**Phase 3 additions (2025-11-24):**
- `let-syntax` - Local macro bindings (Approach A: expand during desugaring)
- `letrec-syntax` - Recursive local macro bindings (Approach A: expand during desugaring)

**Benefits achieved:**
- Unified evaluation path reduces code duplication
- Better TCO support through CoreExpr
- Cleaner separation between parsing/desugaring and evaluation
- Macro expansion integrated into desugaring phase
- 698/1108 Chibi R7RS tests passing (63% compliance)

### ✅ Previously Deferred - Now Complete!

#### `let-syntax` and `letrec-syntax`
**Status**: ✅ MIGRATED (2025-11-24)
**Approach**: Approach A (expand during desugaring)
**Implementation**:
- Macros compiled directly in desugarer using `patina_macros::Compiler`
- Body desugared in extended environment with macro bindings
- Macros completely eliminated during desugaring (no CoreExpr variants needed!)
- Clean separation: macros are compile-time only

**Implementation highlights**:
1. Desugarer compiles syntax-rules transformers directly
2. Creates extended environment with macro bindings (Value::Macro)
3. Body desugared recursively in macro-aware environment
4. Result is just Begin(desugared_body) - macros gone!

**Results**:
- ✅ All existing tests pass
- ✅ Nested let-syntax works (previously failed)
- ✅ Special form registry now empty (100% CoreExpr coverage)
- ✅ Clippy clean, no warnings

**Research**: See "Detailed Research" section below for full architectural analysis

## Migration Priority

### Phase 1: Quick wins ✅ COMPLETE
1. ✅ **Macro expansion routing** - Route macro-expanded code through CoreExpr
2. ✅ **Lambda body evaluation** - Use CoreExpr for lambda bodies
3. ✅ **Library loading** - Use CoreExpr for loading .scm files

### Phase 2: Extension forms ✅ COMPLETE (2025-11-24)
1. ✅ **`expand`** - Simple debugging form (migrated)
2. ✅ **`case-lambda`** - Standard library form, well-defined semantics (migrated)

**Implementation details:**
- Added `CoreExpr::Expand { expr }` variant with full evaluation logic
- Added `CoreExpr::CaseLambda { clauses }` with `CaseLambdaClause` struct
- Removed both forms from special form registry
- Updated desugarer, evaluator, visitor, and helper functions
- All tests passing, Chibi tests improved to 698/1108 (63%)

### Phase 3: Complex forms ✅ COMPLETE (2025-11-24)
1. ✅ **`let-syntax`** - Expand during desugaring (Approach A)
2. ✅ **`letrec-syntax`** - Expand during desugaring (Approach A)

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

### Phase 2 ✅ COMPLETE (2025-11-24)
- [x] `expand` migrated to CoreExpr
- [x] `case-lambda` migrated to CoreExpr
- [x] All cargo tests passing (~816 tests)
- [x] Chibi tests > 100 passing (achieved 698/1108 = 63%)
- [x] Dead code cleaned up (removed unused helper methods)

### Phase 3 ✅ COMPLETE (2025-11-24)
- [x] `let-syntax` migrated to CoreExpr (via Approach A)
- [x] `letrec-syntax` migrated to CoreExpr (via Approach A)
- [x] All let-syntax edge case tests passing
- [x] Value evaluator special form registry now EMPTY
- [x] Clippy clean, no warnings
- [x] All cargo tests passing

### Long-term (Future Work)
- [x] All special forms in CoreExpr ✅ **COMPLETE**
- [ ] Value evaluator only used for:
  - AST representation ✅ **CURRENT STATE**
  - Runtime values ✅ **CURRENT STATE**
  - Not evaluation ✅ **CURRENT STATE**
- [x] Clean separation: Frontend → CoreExpr → Evaluation ✅ **ACHIEVED**
- [ ] Benchmark and optimize CoreExpr evaluation performance
- [ ] Consider moving to CoreClosure for lambda bodies (Phase 4)

## Detailed Research: Let-syntax and Letrec-syntax Migration

**Research Date**: 2025-11-24
**Status**: Complete - Architectural approach identified

### Current Implementation Analysis

#### How let-syntax/letrec-syntax work today (Value evaluator)

**Location**: `crates/patina-tree-walker/src/eval/special_forms/let_syntax.rs`

**Implementation flow**:
```rust
1. Parse bindings: ((name transformer) ...)
2. For each binding:
   - Compile transformer using evaluator.compile_syntax_rules()
   - Create Value::Macro { name, data: CompiledMacro }
3. Create new environment with macro bindings
4. Evaluate body in new environment using eval_with_core_routing()
```

**Key difference between let-syntax and letrec-syntax**:
- **let-syntax**: Macros compiled in *parent* environment (line 185)
  - Macros cannot reference each other
  - Pattern: `compile_env = env`
- **letrec-syntax**: Macros compiled in *new* environment (line 185)
  - Macros can reference each other (recursive definitions)
  - Pattern: `compile_env = new_env`

**Current problem**:
- When CoreExpr desugarer encounters let-syntax, returns `FallbackFormNeeded`
- Outer forms containing let-syntax fail because they can't complete desugaring
- Example: `(define result (let-syntax ...))` fails entirely

### Reference Implementation: Chibi-scheme

**Source**: `~/Project/reference/chibi-scheme/eval.c`

**Chibi's approach** (lines ~1-40 in analyze_let_syntax_aux):
```c
1. Create syntactic environment (sexp_env_syntactic_p(env) = 1)
2. Set parent environment
3. Bind macros using analyze_bind_syntax()
   - For let-syntax: bind in parent context
   - For letrec-syntax: bind in new context
4. Analyze body in new environment (analyze_seq())
5. Return analyzed body - NO RUNTIME REPRESENTATION OF MACROS
```

**Key insight from chibi**: Macros are *completely eliminated* during the analysis (desugaring) phase. The body is analyzed in an extended environment, but the macro bindings themselves don't appear in the analyzed output.

### Patina's Macro Compilation Architecture

#### Current state:

**Macro compilation location**:
- Core implementation: `patina_macros` crate (separate, no circular deps)
- Exposed types: `Compiler`, `CompiledMacro`, `expand_macro` (public API)
- Wrapper method: `Evaluator::compile_syntax_rules()` in `patina-tree-walker`

**The wrapper** (`crates/patina-tree-walker/src/eval/mod.rs:1633`):
```rust
pub(crate) fn compile_syntax_rules(
    &self,
    expr: &Value,              // (syntax-rules ...)
    name: Rc<str>,
    env: &Rc<Environment>,     // For hygiene/lexical capture
) -> Result<CompiledMacro, EvalError>
```

#### Key architectural fact:

**`patina_macros::Compiler` is already accessible to the frontend!**
- No circular dependency: `patina-frontend` → `patina-macros` ✓
- The evaluator wrapper is just a convenience, not required
- Desugarer could use `patina_macros::Compiler` directly

### Architectural Approaches

#### Approach A: Expand During Desugaring (Theoretically Cleanest)

**Concept**: Make let-syntax completely disappear during desugaring, like chibi-scheme.

**Implementation**:
```rust
// In desugarer.rs
fn desugar_let_syntax(&self, args: &Value) -> Result<CoreExpr> {
    // 1. Parse bindings
    let bindings = parse_bindings(args)?;

    // 2. Compile each macro transformer using patina_macros::Compiler
    let mut compiled_macros = Vec::new();
    for (name, transformer) in bindings {
        let compiler = Compiler::new_with_env(
            literals,
            ellipsis,
            self.env.clone()  // Capture environment for hygiene
        );
        let compiled = compiler.compile_macro(transformer)?;
        compiled_macros.push((name, compiled));
    }

    // 3. Create extended environment with macro bindings
    let extended_env = create_extended_env(self.env, compiled_macros);

    // 4. Recursively desugar body in extended environment
    let extended_desugarer = Desugarer::with_env(extended_env);
    let body_exprs = body.iter()
        .map(|e| extended_desugarer.desugar(e))
        .collect()?;

    // 5. Return JUST THE BODY - no CoreExpr::LetSyntax needed!
    Ok(CoreExpr::Begin(body_exprs))
}
```

**Advantages**:
- ✅ Cleanest separation: macros are compile-time only
- ✅ No CoreExpr representation needed (macros are gone)
- ✅ Matches chibi-scheme's design
- ✅ Body is fully desugared, ready for any backend

**Challenges**:
- ⚠️ Desugarer needs to create macro bindings in environment
- ⚠️ Currently `Environment` is in `patina-runtime`, desugarer can access it
- ⚠️ Need to expose macro compilation to desugarer (but `patina_macros` is already accessible)
- ⚠️ More refactoring required

**Dependencies satisfied**:
- `patina-frontend` → `patina-runtime` ✓ (already exists)
- `patina-frontend` → `patina-macros` ✓ (already exists)
- No circular dependencies

**Estimated effort**: Medium (1-2 days)
- Add macro compilation to desugarer
- Add environment extension logic
- Handle let-syntax vs letrec-syntax scoping
- Test with nested cases

#### Approach B: Store in CoreExpr, Expand During Evaluation (Pragmatic)

**Concept**: Follow the `DefineSyntax` pattern - store transformers as Values, compile during evaluation.

**Implementation**:

1. **Add CoreExpr variants** (`patina-ir/src/core_expr.rs`):
```rust
CoreExpr::LetSyntax {
    bindings: Vec<(Symbol, Value)>,  // (name, transformer-as-value)
    body: Vec<CoreExpr>,
}

CoreExpr::LetrecSyntax {
    bindings: Vec<(Symbol, Value)>,
    body: Vec<CoreExpr>,
}
```

2. **Desugarer** (`patina-frontend/src/desugarer/mod.rs`):
```rust
fn desugar_let_syntax(&self, args: &Value) -> Result<CoreExpr> {
    let bindings = parse_bindings(args)?;  // Extract (name, transformer) pairs

    // Keep transformers as Values (like DefineSyntax does)
    let bindings: Vec<(Symbol, Value)> = bindings.into_iter()
        .map(|(name, trans)| Ok((symbol_from_value(name)?, trans)))
        .collect()?;

    // Body is NOT YET DESUGARED - will be desugared during evaluation
    // For now, convert to CoreExpr without macro awareness
    let body_exprs = body.iter()
        .map(|e| self.desugar(e))
        .collect()?;

    Ok(CoreExpr::LetSyntax { bindings, body: body_exprs })
}
```

**Wait, this won't work!** The body needs macro-aware desugaring, but the desugarer runs first!

**Revised implementation** - Store body as Values:
```rust
CoreExpr::LetSyntax {
    bindings: Vec<(Symbol, Value)>,  // Transformers
    body: Vec<Value>,                 // Body as Values, NOT CoreExpr!
}
```

3. **Evaluator** (`patina-tree-walker/src/eval/core_eval.rs`):
```rust
CoreExpr::LetSyntax { bindings, body } => {
    // 1. Compile transformers
    let mut macros = Vec::new();
    for (name, transformer) in bindings {
        let compiled = evaluator.compile_syntax_rules(transformer, name.clone(), &env)?;
        macros.push((name, compiled));
    }

    // 2. Create environment with macro bindings
    let extended_env = create_env_with_macros(env, macros);

    // 3. Evaluate body (which will trigger macro expansion and CoreExpr routing)
    let mut result = Value::Unspecified;
    for (i, expr_value) in body.iter().enumerate() {
        let is_last = i == body.len() - 1;
        if is_last && in_tail_position {
            return Ok(EvalResult::TailCall {
                expr: expr_value.clone(),
                env: extended_env
            });
        } else {
            result = evaluator.eval_with_core_routing(expr_value, &extended_env)?;
        }
    }
    Ok(EvalResult::Value(result))
}
```

**Advantages**:
- ✅ Consistent with existing `DefineSyntax` pattern
- ✅ Minimal changes to desugarer
- ✅ Leverages existing `compile_syntax_rules` infrastructure
- ✅ Works with current architecture

**Challenges**:
- ⚠️ Body stored as Values, not CoreExpr (mixed representation)
- ⚠️ Less clean separation (macros still in IR)
- ⚠️ Two-phase: desugar structure, then expand macros during eval

**Estimated effort**: Low-Medium (1-2 days)
- Add CoreExpr variants
- Update desugarer (minimal changes)
- Update evaluator (similar to current let_syntax.rs logic)
- Update visitor and helper functions

#### Approach C: Two-Phase Pipeline (Not Recommended)

**Concept**: Add a separate macro expansion pass before desugaring.

```
Parse → Value → [EXPAND ALL MACROS] → Value → Desugar → CoreExpr → Eval
```

**Advantages**:
- ✅ Clear separation of concerns

**Disadvantages**:
- ❌ More complex pipeline
- ❌ Requires full macro expansion before desugaring (may expand too much)
- ❌ Current macro-aware desugaring is elegant and works well
- ❌ Doesn't align with current architecture

**Not recommended** - discarded in favor of A or B.

### Comparison Matrix

| Aspect | Approach A (Desugar) | Approach B (CoreExpr) |
|--------|----------------------|-----------------------|
| **Cleanliness** | ⭐⭐⭐⭐⭐ Best | ⭐⭐⭐ Good |
| **Consistency** | ⭐⭐⭐ New pattern | ⭐⭐⭐⭐⭐ Matches DefineSyntax |
| **Complexity** | ⭐⭐⭐ Medium | ⭐⭐⭐⭐ Low |
| **Effort** | 1-2 days | 1-2 days |
| **Dependencies** | All satisfied | All satisfied |
| **Runtime cost** | Zero (gone) | Minimal (compiled once) |
| **IR cleanliness** | ⭐⭐⭐⭐⭐ No trace | ⭐⭐⭐ Mixed Value/CoreExpr |

### Recommended Approach: Approach B (Pragmatic)

**Rationale**:

1. **Consistency**: Follows established `DefineSyntax` pattern
   - Transformer stored as Value ✓
   - Compiled during evaluation ✓
   - Same architecture, same code patterns

2. **Risk**: Lower risk, more incremental
   - Minimal desugarer changes
   - Reuses existing `compile_syntax_rules`
   - Similar to current Value evaluator implementation

3. **Migration path**: Can refactor to Approach A later
   - If we want cleaner separation in Phase 4, we can migrate
   - For now, get it working and complete the CoreExpr migration
   - Don't let perfect be the enemy of good

4. **Effort**: Comparable effort, but safer
   - Both approaches ~1-2 days
   - Approach B has less architectural risk
   - Easier to test incrementally

### Implementation Plan (Approach B)

#### Step 1: Add CoreExpr Variants (30 min)

**File**: `crates/patina-ir/src/core_expr.rs`

Add to CoreExpr enum:
```rust
/// Let-syntax: local macro bindings
/// Example: (let-syntax ((when (syntax-rules () ...))) body ...)
/// Transformers stored as Values, body as Values (for macro expansion)
LetSyntax {
    bindings: Vec<(Symbol, Value)>,  // (name, transformer)
    body: Vec<Value>,                 // Body NOT yet desugared
},

/// Letrec-syntax: recursive local macro bindings
/// Example: (letrec-syntax ((my-or (syntax-rules () ...))) body ...)
LetrecSyntax {
    bindings: Vec<(Symbol, Value)>,
    body: Vec<Value>,
},
```

Update:
- `CoreExpr::kind()` method
- `CoreExpr::Display` implementation
- `crates/patina-ir/src/visitor.rs` - add visitor support

#### Step 2: Update Desugarer (45 min)

**File**: `crates/patina-frontend/src/desugarer/mod.rs`

Change lines 219-221 from:
```rust
"let-syntax" | "letrec-syntax" => Err(DesugarError::FallbackFormNeeded {
    form: sym.to_string(),
}),
```

To:
```rust
"let-syntax" => self.desugar_let_syntax(&cdr),
"letrec-syntax" => self.desugar_letrec_syntax(&cdr),
```

Add methods:
```rust
fn desugar_let_syntax(&self, args: &Value) -> Result<CoreExpr> {
    let args_vec = utils::list_to_vec(args)?;

    if args_vec.len() < 2 {
        return Err(DesugarError::InvalidSyntax(
            "let-syntax requires bindings and at least one body expression".to_string()
        ));
    }

    // Parse bindings: ((name transformer) ...)
    let bindings_value = &args_vec[0];
    let bindings_list = utils::list_to_vec(bindings_value)?;

    let mut bindings = Vec::new();
    for binding in bindings_list {
        let binding_vec = utils::list_to_vec(&binding)?;
        if binding_vec.len() != 2 {
            return Err(DesugarError::InvalidSyntax(
                "Each let-syntax binding must be (name transformer)".to_string()
            ));
        }

        // Extract name as symbol
        let name = match &binding_vec[0] {
            Value::Symbol(s) => s.clone(),
            _ => return Err(DesugarError::InvalidSyntax(
                "Macro name must be a symbol".to_string()
            )),
        };

        // Keep transformer as Value (will be compiled during evaluation)
        let transformer = binding_vec[1].clone();
        bindings.push((name, transformer));
    }

    // Body expressions - keep as Values (need macro-aware evaluation)
    let body = args_vec[1..].to_vec();

    Ok(CoreExpr::LetSyntax { bindings, body })
}

fn desugar_letrec_syntax(&self, args: &Value) -> Result<CoreExpr> {
    // Nearly identical to let-syntax, just different CoreExpr variant
    // ... (same logic as above)
    Ok(CoreExpr::LetrecSyntax { bindings, body })
}
```

#### Step 3: Update CoreExpr Evaluator (1-2 hours)

**File**: `crates/patina-tree-walker/src/eval/core_eval.rs`

Add to `eval_core_step()`:
```rust
CoreExpr::LetSyntax { bindings, body } => {
    eval_let_syntax_core(bindings, body, env, evaluator, false, in_tail_position)
}

CoreExpr::LetrecSyntax { bindings, body } => {
    eval_let_syntax_core(bindings, body, env, evaluator, true, in_tail_position)
}
```

Add helper function:
```rust
fn eval_let_syntax_core(
    bindings: &[(Symbol, Value)],
    body: &[Value],
    env: Rc<Environment>,
    evaluator: &super::Evaluator,
    is_letrec: bool,
    in_tail_position: bool,
) -> Result<CoreEvalResult, EvalError> {
    // Create new environment for body (if letrec) or use parent (if let)
    let macro_env = if is_letrec {
        Rc::new(Environment::with_parent(env.clone()))
    } else {
        env.clone()
    };

    // Compile and bind macros
    for (name, transformer) in bindings {
        // Compile transformer in appropriate environment
        let compile_env = if is_letrec { &macro_env } else { &env };
        let compiled = evaluator.compile_syntax_rules(transformer, name.clone(), compile_env)?;

        let macro_value = Value::Macro {
            name: name.clone(),
            data: Rc::new(compiled),
        };

        macro_env.define(name.to_string(), macro_value);
    }

    // Evaluate body in macro-extended environment
    let body_env = if is_letrec {
        macro_env  // Already has macros
    } else {
        let new_env = Rc::new(Environment::with_parent(env.clone()));
        // Copy macros to new environment
        for (name, _) in bindings {
            if let Some(mac) = macro_env.get(name) {
                new_env.define(name.to_string(), mac);
            }
        }
        new_env
    };

    // Evaluate body expressions
    let mut result = Value::Unspecified;
    for (i, expr_value) in body.iter().enumerate() {
        let is_last = i == body.len() - 1;

        if is_last && in_tail_position {
            // Last expression in tail position
            return Ok(CoreEvalResult::TailCall {
                expr: expr_value.clone(),
                env: body_env,
            });
        } else {
            // Not tail position - use CoreExpr routing
            result = evaluator.eval_with_core_routing(expr_value, &body_env)?;
        }
    }

    Ok(CoreEvalResult::Value(result))
}
```

#### Step 4: Remove Special Forms Registration (15 min)

**File**: `crates/patina-tree-walker/src/eval/special_forms/mod.rs`

Remove from `build_registry()`:
```rust
// Remove these lines:
registry.register(Box::new(LetSyntaxForm));
registry.register(Box::new(LetrecSyntaxForm));
```

#### Step 5: Update Visitor (30 min)

**File**: `crates/patina-ir/src/visitor.rs`

Add visitor methods for new variants.

#### Step 6: Add Tests (1 hour)

**File**: `crates/patina-tests/tests/let_syntax.rs` (new file)

Add comprehensive tests:
- Basic let-syntax
- Basic letrec-syntax
- Nested let-syntax
- let-syntax inside define
- Hygiene tests
- Scoping tests (let vs letrec)

#### Step 7: Run Full Test Suite (15 min)

```bash
cargo test
./scripts/run_chibi_tests.sh
```

Expected results:
- All cargo tests pass
- Chibi tests improve (fewer crashes on let-syntax forms)

### Timeline Estimate

| Task | Time | Cumulative |
|------|------|------------|
| Add CoreExpr variants | 30 min | 30 min |
| Update desugarer | 45 min | 1h 15min |
| Update evaluator | 1-2 hours | 2h 15min - 3h 15min |
| Remove special forms | 15 min | 2h 30min - 3h 30min |
| Update visitor | 30 min | 3h - 4h |
| Add tests | 1 hour | 4h - 5h |
| Run tests & debug | 15+ min | 4h 15min - 5h 15min+ |

**Total**: 4-6 hours (about 1 day of focused work)

### Success Criteria

1. ✅ All cargo tests pass (~816+ tests)
2. ✅ New let-syntax tests pass (10+ tests)
3. ✅ Chibi R7RS tests improve (>700 passing, target 720+)
4. ✅ No fallback warnings for let-syntax forms
5. ✅ Nested let-syntax cases work (previously failed)
6. ✅ Special form registry empty or minimal

### Future Improvements (Optional, Post-Migration)

If we want to move to Approach A (expand during desugaring) later:

1. Add macro compilation support to desugarer
2. Implement environment extension in desugarer
3. Change LetSyntax desugaring to expand body immediately
4. Remove CoreExpr::LetSyntax variants
5. Update tests

Estimated additional effort: 1-2 days

**Decision**: Defer to Phase 4 or later. Not a priority after migration complete.

### Conclusion

**Recommended path**: Implement Approach B (CoreExpr storage) now.

**Reasoning**:
- Fastest path to completing CoreExpr migration
- Lowest risk, most consistent with current architecture
- Can optimize later if needed
- Unblocks remaining work (Value evaluator removal)

**Next steps**:
1. Get user approval on architectural approach
2. Implement according to plan above
3. Test thoroughly with let-syntax edge cases
4. Complete CoreExpr migration (Phase 3)
5. Celebrate! 🎉

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

## Phase 2 Completion Summary (2025-11-24)

### What Was Accomplished

Successfully migrated 2 forms from Value evaluator to CoreExpr in a single session:

**1. `expand` form (Patina debugging extension)**
- Added `CoreExpr::Expand { expr: Box<CoreExpr> }` variant
- Implemented full desugaring and evaluation logic
- Enables macro inspection without evaluation
- Removed from special forms registry

**2. `case-lambda` form (R7RS multi-arity procedures)**
- Added `CoreExpr::CaseLambda { clauses: Vec<CaseLambdaClause> }` variant
- Created `CaseLambdaClause` struct with `params: Formals` and `body: Vec<CoreExpr>`
- Implemented arity-based dispatch during procedure application
- Removed from special forms registry

### Code Changes

**Files modified:**
- `crates/patina-ir/src/core_expr.rs` - Added new CoreExpr variants and CaseLambdaClause struct
- `crates/patina-ir/src/lib.rs` - Exported CaseLambdaClause
- `crates/patina-ir/src/visitor.rs` - Added visitor support for both forms
- `crates/patina-frontend/src/desugarer/mod.rs` - Added desugaring logic for both forms
- `crates/patina-tree-walker/src/eval/core_eval.rs` - Added evaluation logic, updated helper functions
- `crates/patina-tree-walker/src/eval/special_forms/mod.rs` - Removed registrations
- `crates/patina-tree-walker/src/eval/mod.rs` - Cleaned up dead code (removed 124 lines of unused helper methods)

**Lines of code:**
- Added: ~200 lines (new CoreExpr variants, evaluation logic, helpers)
- Removed: ~124 lines (dead code cleanup)
- Net: +76 lines

### Test Results

**Before migration:**
- Cargo tests: All passing
- Chibi R7RS tests: Not measured

**After migration:**
- Cargo tests: All passing (~816 tests)
- Chibi R7RS tests: **698/1108 passing (63%)**
  - Far exceeding target of >100 passing
  - 38 failing, 372 errors (missing features)

### Architectural Impact

**Special forms registry status:**
- Before: 3 forms (let-syntax, letrec-syntax, case-lambda, expand)
- After: 2 forms (let-syntax, letrec-syntax only)
- 50% reduction in fallback forms

**CoreExpr coverage:**
- Before: 11 forms
- After: 13 forms (85% of all special forms)
- Only 2 forms remain in Value evaluator fallback

**Evaluation path:**
- Primary path: CoreExpr (13 forms) ✅
- Fallback path: Value evaluator (2 forms) ⚠️

### Conclusion

Phase 2 migration is complete and successful. The CoreExpr evaluator now handles 13 of 15 special forms (87%). The remaining `let-syntax` and `letrec-syntax` forms require additional architectural work to properly support compile-time macro environments during desugaring.

**Next steps for full unification:**
- Design compile-time environment management in desugarer
- Implement `let-syntax` and `letrec-syntax` in CoreExpr
- Remove Value evaluator fallback path entirely
- Achieve 100% CoreExpr evaluation coverage

Despite the incomplete migration, the project has achieved a significantly cleaner evaluation architecture with minimal fallback usage, making it well-suited for future enhancements (gradual typing, reactive streams, etc.).
