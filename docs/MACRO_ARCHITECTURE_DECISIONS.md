# Macro Architecture Decisions

**Last Updated:** 2025-11-23
**Status:** Active Documentation

This document tracks important architectural decisions and known limitations in the macro system.

---

## Decision: MacroEnv vs Environment in Compiler

**Date:** 2025-11-23
**Context:** During the extraction of `patina-macros` crate and creation of `MacroEnv`

### Current State

The `Compiler` struct (`crates/patina-macros/src/macro_expander/compiler.rs:68-89`) has this field:

```rust
pub struct Compiler {
    // ... other fields ...

    /// Environment where the macro is being defined (for hygiene)
    /// Free variables in templates will capture this environment
    env: Option<Rc<patina_runtime::Environment>>,

    // ... other fields ...
}
```

**This still uses `patina_runtime::Environment` instead of `MacroEnv`.**

### Why This Decision Was Made

During the MacroEnv migration, we decided to **temporarily defer** updating `Compiler::env` because:

1. **Scope of current work:** The main goal was to extract the macro crate and separate macro-time from runtime concerns in the expansion/hygiene code
2. **Complexity:** The `Compiler::env` is used for **environment capture at macro definition time** - a more complex hygiene feature
3. **Current usage:** The `env` field is `Option<Rc<Environment>>` and is mostly unused in the current implementation
4. **Priority:** Getting the core architecture working (pipeline, CoreExpr integration) is more urgent

### Known Limitation: Lexical Scoping Hygiene Bug

**This causes a known bug with lexical macro scoping:**

```scheme
;; From chibi-scheme r7rs-tests.scm
(test 'outer
  (let ((x 'outer))
    (let-syntax ((m (syntax-rules () ((m) x))))
      (let ((x 'inner))
        (m)))))

;; Expected: 'outer (macro should capture x from outer let)
;; Actual:   'inner (macro incorrectly uses inner x)
```

**Why this fails:**
- When `let-syntax` defines macro `m`, it should capture the **lexical environment** at that point
- Free variable `x` in the template should refer to the `x` from the outer `let`
- Currently, there's no proper environment capture, so `x` gets resolved during expansion instead of at definition time

### What Needs to Be Done (Future Work)

To fix this properly:

1. **Change `Compiler::env` to use `MacroEnv`**
   ```rust
   pub struct Compiler {
       /// Macro environment where the macro is being defined (for hygiene)
       /// Free variables in templates will capture this environment
       macro_env: Option<Rc<MacroEnv>>,
       // ...
   }
   ```

2. **Store captured environment with compiled macros**
   ```rust
   pub struct CompiledMacro {
       pub name: Rc<str>,
       pub literals: Vec<Rc<str>>,
       pub rules: Vec<CompiledRule>,
       pub max_pvars: usize,

       /// Captured macro environment at definition time
       /// Used for resolving free identifiers in templates
       pub captured_env: Option<Rc<MacroEnv>>,  // ADD THIS
   }
   ```

3. **Use captured environment during template expansion**
   - In `Expander::new()`, accept both the expansion-time `MacroEnv` AND the captured `MacroEnv`
   - When renaming identifiers, check the captured environment first
   - This allows proper lexical scoping for macros

4. **Update `Identifier` in `template.rs`**
   - The `Identifier` type already has infrastructure for environment capture
   - Connect it to the `CompiledMacro::captured_env`

### Testing Strategy

When implementing this fix:

1. **Add test case** from chibi-scheme:
   ```rust
   #[test]
   fn test_lexical_macro_scoping() {
       let code = r#"
       (let ((x 'outer))
         (let-syntax ((m (syntax-rules () ((m) x))))
           (let ((x 'inner))
             (m))))
       "#;
       assert_eval_to(code, "outer");  // Should be 'outer, not 'inner
   }
   ```

2. **Verify chibi test passes:**
   ```bash
   ./scripts/run_chibi_tests.sh
   # Check that the "lexical macro scoping" test passes
   ```

3. **Additional edge cases:**
   - Nested `let-syntax` forms
   - Macros defined inside macros
   - Multiple free variables with shadowing

### References

- **Original discussion:** This decision was made during the MacroEnv migration session on 2025-11-23
- **Test case location:** `scheme_tests/chibi-scheme/tests/r7rs-tests.scm` (search for "let-syntax")
- **Related files:**
  - `crates/patina-macros/src/macro_expander/compiler.rs:68-89` - Where `env` is defined
  - `crates/patina-macros/src/macro_expander/template.rs` - `Identifier` type with capture infrastructure
  - `crates/patina-macros/src/macro_expander/expander.rs` - Template expansion logic

### Decision Log

| Date | Status | Notes |
|------|--------|-------|
| 2025-11-23 | Deferred | Decided to keep `Compiler::env` as `Option<Rc<Environment>>` temporarily |
| TBD | To Implement | Switch to `MacroEnv` and implement proper lexical scoping |

---

## Related Decisions

See `docs/MACRO_ARCHITECTURE_PROPOSAL.md` for the overall macro architecture design.

---

**Action Items:**
- [ ] Implement lexical scoping hygiene fix (tracked in this document)
- [ ] Add test case for lexical macro scoping
- [ ] Verify chibi r7rs-tests pass after implementation
