# Macro Architecture Proposal

**Date:** 2025-11-22
**Status:** Proposal for Future Work
**Context:** After CoreExpr migration investigation

## Background

The CoreExpr migration revealed architectural issues with how macro expansion is integrated into the evaluation pipeline. Currently, macro expansion is tightly coupled with the tree-walker evaluator, making it difficult to:

1. Use alternative evaluation strategies (CoreExpr, VM, JIT)
2. Test macro expansion independently
3. Support multiple macro systems (syntax-rules, syntax-case, etc.)

## Current Architecture

```
┌─────────────────────────────────────────────────────────────┐
│ patina-frontend (Lexer, Parser, MacroExpander)              │
│  - MacroExpander uses Environment from patina-runtime       │
│  - Tightly coupled to Value representation                  │
└─────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────┐
│ patina-tree-walker (Evaluator)                              │
│  - Calls macro expansion during evaluation                  │
│  - expand_all_macros() method on Evaluator                  │
│  - Macro expansion in eval_list_impl()                      │
└─────────────────────────────────────────────────────────────┘
```

**Problems:**
- Macro expansion happens at two points: before evaluation AND during evaluation
- Can't use CoreExpr without breaking macro expansion
- MacroExpander depends on Environment, creating circular concerns
- No clear separation between parsing, expansion, and evaluation

## Proposed Architecture: Separate Macro Crate

```
┌─────────────────────────────────────────────────────────────┐
│ patina-frontend (Lexer, Parser only)                        │
│  - Pure syntax transformation                               │
│  - No macro knowledge                                       │
└─────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────┐
│ patina-macros (NEW CRATE)                                   │
│  - MacroExpander (syntax-rules)                             │
│  - Macro environment (separate from runtime env)            │
│  - Hygiene system                                           │
│  - Future: syntax-case, procedural macros                   │
└─────────────────────────────────────────────────────────────┘
                            ↓
┌─────────────────────────────────────────────────────────────┐
│ patina-pipeline (NEW CRATE - optional)                      │
│  - Orchestrates: parse → expand → desugar → eval            │
│  - Pluggable macro expanders                                │
│  - Pluggable evaluators (Value, CoreExpr, VM, JIT)          │
└─────────────────────────────────────────────────────────────┘
         ↓                                    ↓
┌─────────────────────────┐    ┌──────────────────────────────┐
│ patina-tree-walker      │    │ patina-vm (future)           │
│  - Value evaluator      │    │  - Bytecode evaluator        │
└─────────────────────────┘    └──────────────────────────────┘
```

## Benefits

### 1. Clear Separation of Concerns

**Parsing** (patina-frontend):
- Lexical analysis
- Syntactic analysis
- Build AST (Value)
- No semantic knowledge

**Macro Expansion** (patina-macros):
- Pattern matching
- Template expansion
- Hygiene
- Macro environment management

**Evaluation** (patina-tree-walker, patina-vm, etc.):
- Execute code
- No macro knowledge
- Works on expanded code only

### 2. Flexible Pipeline Assembly

Users can compose different pipelines:

```rust
// Standard pipeline (what we have now)
let pipeline = Pipeline::new()
    .with_parser(Parser::new())
    .with_macro_expander(SyntaxRulesExpander::new())
    .with_evaluator(TreeWalker::new());

// CoreExpr optimization pipeline
let pipeline = Pipeline::new()
    .with_parser(Parser::new())
    .with_macro_expander(SyntaxRulesExpander::new())
    .with_desugarer(Desugarer::new())
    .with_evaluator(CoreExprEvaluator::new());

// VM pipeline (future)
let pipeline = Pipeline::new()
    .with_parser(Parser::new())
    .with_macro_expander(SyntaxRulesExpander::new())
    .with_compiler(BytecodeCompiler::new())
    .with_vm(VM::new());

// Testing: skip macro expansion
let pipeline = Pipeline::new()
    .with_parser(Parser::new())
    .with_evaluator(TreeWalker::new());
```

### 3. Testability

Each component can be tested independently:

```rust
// Test macro expansion without evaluation
let expander = SyntaxRulesExpander::new();
let expanded = expander.expand("(or #f #t)", &macro_env)?;
assert_eq!(expanded.to_string(), "((lambda (tmp) (if tmp tmp #t)) #f)");

// Test evaluation without macro expansion
let evaluator = TreeWalker::new();
let result = evaluator.eval("((lambda (x) x) 42)", &env)?;
assert_eq!(result.to_string(), "42");

// Test full pipeline
let pipeline = Pipeline::standard();
let result = pipeline.eval("(or #f #t)")?;
assert_eq!(result.to_string(), "#t");
```

### 4. Multiple Macro Systems

Easy to add alternative macro systems:

```rust
// patina-macros/src/lib.rs
pub trait MacroExpander {
    fn expand(&self, expr: &Value, env: &MacroEnv) -> Result<Value, MacroError>;
}

pub struct SyntaxRulesExpander { ... }  // Current implementation
pub struct SyntaxCaseExpander { ... }   // Future: full syntax-case
pub struct ProceduralExpander { ... }   // Future: procedural macros
```

### 5. Cleaner CoreExpr Integration

The CoreExpr path becomes just another evaluator choice:

```rust
impl Pipeline {
    pub fn eval(&self, code: &str) -> Result<Value, Error> {
        // 1. Parse
        let ast = self.parser.parse(code)?;

        // 2. Expand macros (if configured)
        let expanded = if let Some(expander) = &self.macro_expander {
            expander.expand(&ast, &self.macro_env)?
        } else {
            ast
        };

        // 3. Desugar (if using CoreExpr)
        let evaluable = if let Some(desugarer) = &self.desugarer {
            EvaluableForm::CoreExpr(desugarer.desugar(&expanded)?)
        } else {
            EvaluableForm::Value(expanded)
        };

        // 4. Evaluate
        self.evaluator.eval(&evaluable, &self.runtime_env)
    }
}
```

## Migration Path

### Phase 1: Extract Macro Crate (Low Risk)
1. Create `patina-macros` crate
2. Move `macro_expander/` from `patina-frontend` to `patina-macros`
3. Update imports
4. No functional changes

**Effort:** 2-4 hours
**Risk:** Low (pure code movement)

### Phase 2: Separate Macro Environment (Medium Risk)
1. Create `MacroEnv` type in `patina-macros`
2. Separate from runtime `Environment`
3. Update macro expander to use `MacroEnv`

**Effort:** 4-8 hours
**Risk:** Medium (requires careful migration)

### Phase 3: Create Pipeline Crate (Low Risk)
1. Create `patina-pipeline` crate
2. Implement `Pipeline` builder
3. Move orchestration logic from `patina-interpreter`

**Effort:** 4-6 hours
**Risk:** Low (additive change)

### Phase 4: Enable CoreExpr Path (Now Easy!)
1. Add CoreExpr option to pipeline
2. Test thoroughly with new test suite
3. Make it opt-in via feature flag

**Effort:** 2-4 hours (most work already done!)
**Risk:** Low (isolated change)

## Alternative: Minimal Change

If you don't want to create new crates, a minimal fix:

1. **Remove macro expansion from `eval_list_impl`**
   - Macro expansion happens ONLY before evaluation
   - Never during evaluation

2. **Add explicit expansion step in backend**
   ```rust
   fn eval(&self, expr: &Value, env: &Rc<Environment>) -> Result<Value, Self::Error> {
       // Expand macros ONCE at the top level
       let expanded = self.evaluator.expand_all_macros(expr, env)?;

       // Then choose evaluation path
       if should_use_core_expr(&expanded) {
           let core_expr = Desugarer::new().desugar(&expanded)?;
           eval_core(&core_expr, env.clone(), &self.evaluator)
       } else {
           // Use Value evaluator, but DON'T expand macros again
           self.evaluator.eval_expanded(&expanded, env)
       }
   }
   ```

3. **Add `eval_expanded` method**
   - Like `eval_in_env` but skips macro expansion
   - Assumes macros already expanded

**Effort:** 4-6 hours
**Risk:** Medium (subtle behavior changes)
**Benefit:** Enables CoreExpr without major refactoring

## Recommendation

**Short term (next session):**
- Implement the "Minimal Change" approach
- This unblocks CoreExpr integration
- Low effort, clear benefit

**Long term (when time permits):**
- Extract `patina-macros` crate (Phase 1)
- Creates cleaner architecture
- Enables future experimentation

**Future (when needed):**
- Full pipeline architecture (Phases 2-4)
- Only if you need multiple macro systems or evaluators

## Questions to Consider

1. **Do you want multiple macro systems?** (syntax-rules, syntax-case, procedural)
   - If yes → separate macro crate is valuable
   - If no → minimal change is sufficient

2. **Do you want multiple evaluators?** (tree-walker, VM, JIT)
   - If yes → pipeline architecture is valuable
   - If no → current architecture is fine

3. **Is testability a priority?**
   - If yes → separate components help
   - If no → integrated approach is simpler

4. **How important is CoreExpr optimization?**
   - If critical → worth the refactoring
   - If nice-to-have → defer to later

## Next Steps

1. **Decide on approach:**
   - Minimal change (quick, enables CoreExpr)
   - Macro crate (clean, flexible)
   - Full pipeline (comprehensive, future-proof)

2. **Implement chosen approach**

3. **Test thoroughly** with:
   - All compliance tests
   - Chibi test suite
   - New CoreExpr integration tests

4. **Document the decision** in architecture docs

---

**My Recommendation:** Start with the minimal change to enable CoreExpr, then extract the macro crate when you have time. This gives you immediate benefits while setting up for long-term flexibility.
