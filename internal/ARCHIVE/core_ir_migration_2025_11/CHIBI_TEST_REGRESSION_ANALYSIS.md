# Chibi Test Regression Analysis - UPDATED

**Date:** 2025-11-22 (Updated after investigation)
**Issue:** Chibi tests dropped from 704 passing (63.5%) to 18 passing (1.6%)

## ROOT CAUSE: CoreExpr Migration

**CONFIRMED:** The CoreExpr migration in `backend.rs` is causing the regression.

### Evidence

1. **Without CoreExpr changes** (vanilla main): 704 passing tests (63.5%)
2. **With CoreExpr changes**: 18 passing tests (1.6%)
3. Stashing CoreExpr changes and rebuilding immediately restored 704 passing tests

## The Problem

The CoreExpr pipeline in `crates/patina-tree-walker/src/backend.rs` calls `expand_all_macros()` on EVERY expression before evaluation. This breaks the existing macro expansion flow.

### Before CoreExpr (Working - 704 tests)

```rust
fn eval(&self, expr: &Value, env: &Rc<Environment>) -> Result<Value, Self::Error> {
    self.evaluator.eval_in_env(expr, env)  // Macros expanded during evaluation
}
```

### After CoreExpr (Broken - 18 tests)

```rust
fn eval(&self, expr: &Value, env: &Rc<Environment>) -> Result<Value, Self::Error> {
    let expanded = self.evaluator.expand_all_macros(expr, env)?;  // Pre-expand ALL macros

    match desugarer.desugar(&expanded) {
        Ok(core_expr) => eval_core(&core_expr, env.clone(), &self.evaluator),
        Err(_) => self.evaluator.eval_in_env(expr, env),  // Fallback
    }
}
```

## What's Broken

Calling `expand_all_macros` at the Backend level causes issues because:

1. **Double expansion**: The fallback path calls `eval_in_env` which also expands macros, but we're passing the original `expr`, so macros get expanded twice
2. **Wrong expansion context**: Macros are being expanded before the evaluator can set up proper environments
3. **Library loading issues**: Library code goes through this path and gets incorrectly macro-expanded

## Next Steps

Need to fix the CoreExpr pipeline to properly integrate with the existing macro expansion system without breaking it.

Possible solutions:
1. Don't call `expand_all_macros` in `backend.rs` - let the evaluators handle it
2. Only use CoreExpr path for simple expressions, keep complex ones on Value path
3. Fix the macro expansion to work correctly in the CoreExpr context

## Previous Investigation (Outdated)

Earlier investigation incorrectly attributed the regression to commit 31f2d0d (hygiene changes). This was based on incomplete testing. The hygiene changes are NOT the cause - the CoreExpr migration is.
