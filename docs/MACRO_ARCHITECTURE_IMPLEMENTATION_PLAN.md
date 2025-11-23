# When to Implement MacroEnv Integration in Compiler

## Current Status (2025-11-23)

**Architecture Completed:**
- ✅ patina-macros crate created and extracted
- ✅ MacroEnv type implemented with parent chains for lexical scoping
- ✅ patina-pipeline crate created with StandardPipeline
- ✅ SimpleInterpreter added for easier API
- ✅ All tests passing (0 failures, 7 ignored tests in patina-macros)

**Known Limitation:**
- `Compiler::env` still uses `Environment` instead of `MacroEnv`
- This causes a lexical macro scoping bug (documented in MACRO_ARCHITECTURE_DECISIONS.md)
- Test case: `(let ((x 'outer)) (let-syntax ((m (syntax-rules () ((m) x)))) (let ((x 'inner)) (m))))`
  - Expected: `'outer`
  - Current: `'inner` (captures wrong binding)

## Recommendation: Defer Until Phase 1 Completion

**Why defer:**

1. **Current workaround is acceptable**: The bug only affects `let-syntax` with lexical capture, which is an edge case
2. **No immediate user impact**: Patina is pre-alpha; most macro usage doesn't hit this edge case
3. **Focus on R7RS completeness**: Higher priority work remains:
   - I/O and ports (display, write, file operations)
   - Exception handling (guard, raise)
   - Module system
   - Full string/vector suites
   - Continuations (call/cc)

4. **Clean implementation requires more work**:
   - Need to update all macro compilation call sites
   - May require changes to how macros are loaded from bootstrap.scm
   - Testing would need comprehensive `let-syntax` test suite

5. **Current architecture is sound**: The separation is clean, just needs wiring

## When to Implement

**Trigger conditions** (implement when ANY is true):

1. **User reports lexical macro scoping bug**: Real-world impact means we should fix it
2. **Working on Module System**: R7RS libraries need proper lexical macro scoping
3. **After Phase 1 completion**: Once R7RS-small compliance is done, clean up technical debt
4. **Before Phase 2 (Gradual Typing)**: Type system will likely interact with macro expansion

## Implementation Strategy (When Ready)

**Step 1: Update Compiler**
```rust
// In crates/patina-macros/src/macro_expander/compiler.rs
pub struct Compiler {
    literals: HashSet<Rc<str>>,
    rules: Vec<CompiledRule>,
    env: Option<Rc<MacroEnv>>,  // ← Change from Environment to MacroEnv
}
```

**Step 2: Update all call sites**
- Search for `Compiler::new()` and `Compiler::with_env()`
- Pass MacroEnv instead of Environment
- May need to build MacroEnv from Environment for backward compatibility

**Step 3: Test thoroughly**
- Add comprehensive `let-syntax` tests
- Test nested `let-syntax` with shadowing
- Test macro-defining macros
- Run full R7RS compliance suite

**Step 4: Update documentation**
- Mark MACRO_ARCHITECTURE_DECISIONS.md as "IMPLEMENTED"
- Update architecture docs
- Add release notes about the fix

## Current State: DEFERRED

**Decision:** Keep current implementation with known limitation documented.

**Rationale:** Focus on R7RS compliance features that affect more users. This is technical debt we can pay down later when it becomes blocking or when we have bandwidth.

**Tracking:** This file serves as the implementation plan. When ready to implement, use this as the guide.

---

*Last updated: 2025-11-23*
*Status: Architecture ready, implementation deferred*
