# Macro-Generating Macro Hygiene Fix

**Status:** Research Complete, Implementation Blocked
**Priority:** Medium (specific edge case)
**Relationship:** Should be fixed AFTER `DEFINE_SYNTAX_ELIMINATION.md`

---

## Problem Statement

When a macro generates another macro (via `define-syntax` in the template), and the outer macro's pattern variable is substituted with a symbol that happens to match an inner macro's pattern variable name, the inner macro incorrectly captures the substituted symbol.

### Failing Test Case (from chibi r7rs-tests.scm)

```scheme
(let ()
  (define-syntax foo
    (syntax-rules ()
      ((foo bar y)
       (define-syntax bar
         (syntax-rules ()
           ((bar x) 'y))))))
  (foo bar x)    ; y binds to symbol 'x'
  (bar 1))       ; Should return 'x, actually returns 1
```

### Expected Behavior

1. `(foo bar x)` expands to `(define-syntax bar (syntax-rules () ((bar x) 'x)))`
2. The `'x` in the generated template should be the literal symbol `x` (from outer `y`)
3. `(bar 1)` should return `'x` (the symbol), not `1` (the argument)

### Actual Behavior

1. `(foo bar x)` expands correctly to `(define-syntax bar (syntax-rules () ((bar x) 'x)))`
2. When inner macro `bar` is compiled, the `x` in `'x` is seen as a pattern variable reference
3. `(bar 1)` returns `1` because `x` is bound to the argument `1`

---

## Root Cause Analysis

### The Hygiene Flow

1. **Input flip**: `(foo bar x)` has scope S77 flipped on all identifiers
   - But `bar` and `x` are plain Symbols (no scopes), so flip doesn't affect them

2. **Pattern matching**: `y` binds to Symbol `x` (no scopes)

3. **Template expansion**: Template `'y` is expanded, `y` is substituted with Symbol `x`
   - Result: `(define-syntax bar (syntax-rules () ((bar x) (quote x))))`
   - The substituted `x` is still a plain Symbol

4. **Output flip**: Scope S77 is toggled on identifiers
   - Symbol `x` (from substitution) doesn't get scopes (Symbols don't flip)
   - Introduced identifiers like `define-syntax`, `syntax-rules`, `quote` get `{S77}`

5. **Inner macro compilation**: When `bar` is compiled:
   - Pattern `(bar x)` introduces pattern variable `x`
   - Template `(quote x)` - compiler sees `x` and finds it in pvars table
   - Incorrectly treats the substituted `x` as a pattern variable reference

### The Bug Location

**File:** `patina-macros/src/macro_expander/mod.rs`, function `flip_scope_on_value`

```rust
// Symbols stay as Symbols - they don't participate in hygiene flip
// This preserves special forms like `if`, `define`, `lambda`, etc.
Value::Symbol(_) => value.clone(),
```

The comment explains the rationale: Symbols are for special forms which shouldn't be hygiene-renamed. But the issue is that **pattern variable substitution produces Symbols** that SHOULD participate in hygiene.

---

## Solution Design

### High-Level Approach

The fix requires distinguishing between:
1. **Symbols from source code** - special forms, keywords (shouldn't get scopes)
2. **Symbols from pattern variable substitution** - should get scopes

### Option A: Mark at Substitution Time (Recommended)

Convert Symbols to Identifiers when substituting pattern variables in template expansion.

**Location:** `patina-macros/src/macro_expander/expander.rs`, `Template::Var` handling

```rust
Template::Var(pvref) => {
    match env.get(*pvref, indices) {
        Some(value) => {
            // Convert Symbol to Identifier so it participates in output flip
            let result = match &value {
                Value::Symbol(s) => Value::Identifier(Box::new(
                    patina_runtime::IdentifierData {
                        name: s.clone(),
                        scopes: patina_runtime::ScopeSet::new(),
                    },
                )),
                _ => value,
            };
            Ok(result)
        }
        None => Err(...)
    }
}
```

**Why this works:**
1. Substituted Symbol `x` becomes `Identifier{x, {}}`
2. Output flip adds macro scope: `Identifier{x, {S77}}`
3. When inner macro compiles, `x{S77}` has scopes, so won't match pvar `x`

**Challenge:**
This breaks normal macros! When `let` expands `((lambda (x) body) val)`:
- `x` comes from pattern variable substitution
- If `x` becomes `Identifier{x, {scope}}`, it won't match references in body

### Option B: Check Scopes in Compiler (Partial Fix)

Make the compiler check if an identifier has scopes before treating it as a pattern variable.

**Location:** `patina-macros/src/macro_expander/compiler.rs`, `compile_template`

```rust
if let Some(s) = Self::extract_symbol_name(form) {
    // Only treat as pvar if it's a plain Symbol or Identifier without scopes
    let has_scopes = matches!(form, Value::Identifier(id) if !id.scopes.is_empty());

    if !has_scopes {
        if let Some(pvref) = self.pvars.get(s) {
            return Ok(Template::Var(*pvref));
        }
    }
    // ... rest of hygiene handling
}
```

**Why this alone doesn't work:**
- The substituted `x` is still a Symbol (no scopes)
- Need Option A to make it an Identifier with scopes first

### Option C: Selective Conversion Based on Context

Only convert Symbols to Identifiers when the template contains `syntax-rules`.

**Complexity:** High - requires detecting if we're inside a syntax-rules template

### Recommended Solution: Option A + B Combined

1. **Option A**: Convert Symbols to Identifiers during pattern variable substitution
2. **Option B**: Compiler checks scopes to avoid capturing scoped identifiers as pvars
3. **Additional**: Need to ensure this doesn't break normal variable binding

---

## Why This Should Be Fixed AFTER DefineSyntax Elimination

### Arguments for Fixing After

1. **Cleaner codebase**: `DEFINE_SYNTAX_ELIMINATION.md` simplifies the macro compilation flow
   - After: macro compilation happens entirely in desugarer
   - Easier to reason about scope handling in one place

2. **Single location**: Currently macro compilation happens in TWO places:
   - Desugarer (for `let-syntax`, and partially for `define-syntax`)
   - Evaluator (for `DefineSyntax` CoreExpr)

   After elimination, it's all in the desugarer, so the fix is localized.

3. **Testing**: The DefineSyntax elimination includes its own test verification
   - Fix this bug after that baseline is established
   - Easier to detect regressions

4. **Scope handling clarity**: DefineSyntax elimination clarifies scope propagation
   - `definition_scopes` flow becomes clearer
   - Understanding of scope handling improves

### Arguments for Fixing Before

1. **Bug is independent**: The bug is in the expander and compiler, not in where compilation happens

2. **Baseline**: Fixing this bug first establishes better baseline for chibi tests

### Recommendation: Fix AFTER

The DefineSyntax elimination is a cleaner refactoring that doesn't change semantics.
This bug fix changes the semantics of pattern variable substitution.

**Order:**
1. Complete `DEFINE_SYNTAX_ELIMINATION.md` (structural change, no semantic change)
2. Fix macro-generating macro hygiene (semantic change, localized to macro system)

---

## Implementation Plan

### Phase 1: Deep Understanding (Research)

- [x] Identify the exact failure point (Symbols don't flip)
- [x] Trace through chibi test case with debug output
- [x] Document why simple fixes break other tests
- [x] Create this roadmap document

### Phase 2: Prerequisites

- [ ] Complete `DEFINE_SYNTAX_ELIMINATION.md`
- [ ] All macro compilation in desugarer
- [ ] Test baseline established

### Phase 3: Implement Symbol-to-Identifier Conversion

**File:** `patina-macros/src/macro_expander/expander.rs`

```rust
Template::Var(pvref) => {
    match env.get(*pvref, indices) {
        Some(value) => {
            // For macro-generating macros: convert Symbol to Identifier
            // so it participates in the output flip and gets macro scope.
            //
            // This is necessary because:
            // 1. Symbols don't participate in flip_scope_on_value (to preserve
            //    special forms like `if`, `define`, etc.)
            // 2. But symbols from pattern variable substitution SHOULD participate
            // 3. By converting to Identifier, the output flip will add macro scope
            //
            // IMPORTANT: This only converts to Identifier with EMPTY scopes.
            // The output flip will then add the macro scope.
            // We do NOT add the macro scope here directly.
            let result = match &value {
                Value::Symbol(s) => Value::Identifier(Box::new(
                    patina_runtime::IdentifierData {
                        name: s.clone(),
                        scopes: patina_runtime::ScopeSet::new(),
                    },
                )),
                _ => value,
            };
            Ok(result)
        }
        None => Err(ExpandError::UndefinedVariable { pvref: format!("{:?}", pvref) }),
    }
}
```

### Phase 4: Implement Scope Check in Compiler

**File:** `patina-macros/src/macro_expander/compiler.rs`

```rust
pub fn compile_template(&mut self, form: &Value, level: usize) -> Result<Template, MacroError> {
    if let Some(s) = Self::extract_symbol_name(form) {
        // Check if it's a pattern variable
        // IMPORTANT: Only treat as pattern variable if it's a plain Symbol OR
        // an Identifier without scopes. Identifiers WITH scopes come from outer
        // macro expansions and should NOT be captured as pattern variables.
        let has_scopes = matches!(form, Value::Identifier(id) if !id.scopes.is_empty());

        if !has_scopes {
            if let Some(pvref) = self.pvars.get(s) {
                if pvref.level() > level {
                    return Err(MacroError::InvalidSyntax(...));
                }
                return Ok(Template::Var(*pvref));
            }
        }

        // Not a pattern variable - apply hygiene handling
        // ...
    }
    // ...
}
```

### Phase 5: Handle Variable Binding (THE HARD PART)

The conversion breaks normal variable binding. When `let` expands:

```scheme
(let ((x 10)) body)
; Expands to:
((lambda (x) body) 10)
```

The `x` in `(lambda (x) ...)` comes from pattern variable substitution.
If we convert it to `Identifier{x, {}}`, the output flip makes it `Identifier{x, {scope}}`.

**Problem:** The `x` reference in `body` won't have the same scopes, causing "undefined variable".

**Solutions:**

#### 5a: Don't convert in parameter position

Detect when we're substituting into a lambda parameter list and skip conversion.

**Challenge:** Template structure doesn't tell us we're in parameter position.

#### 5b: Propagate scopes to body

When creating a lambda, propagate the parameter scopes to references in body.

**Challenge:** Requires significant changes to evaluation model.

#### 5c: Use a different marker

Instead of using scopes, use a different field to mark "came from substitution".

**Implementation:** Add `from_substitution: bool` to `IdentifierData`.

```rust
pub struct IdentifierData {
    pub name: Rc<str>,
    pub scopes: ScopeSet,
    pub from_substitution: bool,  // NEW
}
```

**Compiler check:**
```rust
let is_substituted = matches!(form, Value::Identifier(id) if id.from_substitution);
if !is_substituted {
    // Check pattern variables
}
```

**Advantage:** Doesn't affect scope-based binding resolution.
**Disadvantage:** Adds complexity to Value type.

#### 5d: Only convert within syntax-rules context

Track when we're expanding a template that contains `syntax-rules`, only convert then.

**Implementation:** Add a flag to Expander:
```rust
pub struct Expander {
    expansion_env: Rc<Environment>,
    macro_scope: ScopeId,
    in_syntax_rules_template: bool,  // NEW
}
```

When expanding, detect `syntax-rules` forms and set the flag.

**Challenge:** Need to track context through recursive expansion.

### Phase 6: Testing

1. **Macro-generating macro tests:**
   - `test_macro_generating_macro_conflicting_names` should pass
   - Chibi `(foo bar x)` test should pass

2. **Regression tests:**
   - All existing hygiene tests pass
   - All `let`, `let*`, `letrec`, etc. tests pass
   - All `do` loop tests pass
   - All `cond`/`case` tests pass

3. **Edge cases:**
   - Nested macro-generating macros
   - Mutual recursion in generated macros
   - Pattern variable shadowing

---

## Complexity Assessment

| Aspect | Difficulty | Notes |
|--------|------------|-------|
| Symbol→Identifier conversion | Easy | Simple code change |
| Scope check in compiler | Easy | Simple code change |
| Not breaking let/lambda binding | Hard | Requires careful design |
| Testing all edge cases | Medium | Many macro interactions |
| Total | **High** | The "not breaking" part is hard |

---

## Alternative: Accept as Known Limitation

Given the complexity, an alternative is to document this as a known limitation:

1. Mark the test as `#[ignore]` with detailed comment (DONE)
2. Document in user-facing docs that conflicting names in macro-generating macros may not work
3. Recommend users choose non-conflicting names

**Trade-off:** Lower implementation effort, but incomplete R7RS compliance.

---

## Recommendation

1. **Complete `DEFINE_SYNTAX_ELIMINATION.md` first** - simpler, cleaner
2. **Then implement Solution 5d** (context-aware conversion)
   - Only convert when inside syntax-rules template
   - Minimizes impact on normal macros
3. **If 5d is too complex, fallback to documenting limitation**

---

## Files to Modify

| File | Change |
|------|--------|
| `patina-macros/src/macro_expander/expander.rs` | Symbol→Identifier in Template::Var |
| `patina-macros/src/macro_expander/compiler.rs` | Scope check for pattern variables |
| `patina-runtime/src/value/mod.rs` | Possibly add field to IdentifierData |
| `patina-tests/tests/hygiene.rs` | Update test when fixed |

---

## Related Documents

- `PRD/phase1/DEFINE_SYNTAX_ELIMINATION.md` - Should be completed first
- `PRD/phase1/BINDING_OBJECTS_DESIGN.md` - Longer-term hygiene improvements
- `internal/ARCHIVE/macro_research/` - Historical macro research
