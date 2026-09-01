# Patina Macro System Architecture

**Last Updated:** 2025-12-13

This document provides a comprehensive overview of Patina's hygienic macro system, designed to help developers understand the implementation and prepare for future `syntax-case` support.

---

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Key Data Structures](#key-data-structures)
4. [Hygiene Mechanism](#hygiene-mechanism)
5. [Pattern Matching](#pattern-matching)
6. [Template Expansion](#template-expansion)
7. [Ellipsis Handling](#ellipsis-handling)
8. [Integration with Evaluator](#integration-with-evaluator)
9. [Debug Infrastructure](#debug-infrastructure)
10. [Future: syntax-case](#future-syntax-case)
11. [References](#references)

---

## Overview

Patina implements R7RS `syntax-rules` with full hygienic macro expansion using **Racket-style scope sets** (based on Matthew Flatt's "Binding as Sets of Scopes" paper, 2016).

### Feature Status

| Feature | Status |
|---------|--------|
| Basic pattern matching | ✅ Working |
| Pattern variables | ✅ Working |
| Literals (including `_`) | ✅ Working |
| Ellipsis (`...`) | ✅ Working |
| Nested ellipsis (`... ...`) | ✅ Working |
| Ellipsis escaping `(... template)` | ✅ Working |
| Macro-generating macros | ✅ Working |
| Hygienic renaming | ✅ Working |
| `let-syntax` / `letrec-syntax` | ✅ Working |
| Custom ellipsis identifier | ✅ Working |

### Design Principles

1. **Scope-set hygiene** - No alpha-renaming; discrimination via scope sets
2. **Compiled macros** - Patterns and templates are compiled once, executed many times
3. **PVREF encoding** - O(1) pattern variable binding and lookup
4. **Gauche-inspired matching** - `num_following` optimization eliminates backtracking

---

## Architecture

### Crate Structure

```
crates/patina-macros/
├── src/
│   ├── lib.rs                    # Public API, MacroExpander trait
│   └── macro_expander/
│       ├── mod.rs                # Module exports
│       ├── interface.rs          # expand_macro() entry point
│       ├── compiler.rs           # Pattern/template compilation
│       ├── matcher.rs            # Pattern matching engine
│       ├── expander.rs           # Template expansion engine
│       ├── pattern.rs            # Pattern type (re-export from runtime)
│       ├── template.rs           # Template type (re-export from runtime)
│       └── utils.rs              # Shared utilities
```

### Shared Types (in patina-runtime)

Core types are defined in `patina-runtime` for cross-crate sharing:

```
crates/patina-runtime/src/
├── pattern.rs      # Pattern enum
├── template.rs     # Template enum, Identifier
├── match_env.rs    # MatchEnv, MatchValue
├── pvref.rs        # PVRef (pattern variable reference)
├── scope.rs        # ScopeId, ScopeSet
└── value/hygiene.rs # IdentifierData on Value
```

### Processing Pipeline

```
Source: (define-syntax my-macro (syntax-rules (lit) ...))
                            ↓
                    [Compiler] compile_syntax_rules()
                            ↓
            CompiledMacro { patterns, templates, literals }
                            ↓
                    Stored in Environment as Value::Macro
                            ↓
Use: (my-macro arg1 arg2)
                            ↓
                    [Desugarer] detects macro, calls expand_macro()
                            ↓
                    [Interface] expand_macro_with_hygiene()
                            ↓
        ┌───────────────────┴───────────────────┐
        ↓                                       ↓
[Flip scope on INPUT]                   [Try each clause]
                                               ↓
                                        [Matcher] match_pattern()
                                               ↓
                                        MatchEnv with bindings
                                               ↓
                                        [Expander] expand()
                                               ↓
                                        Raw expansion result
        ↓                                       ↓
        └───────────────────┬───────────────────┘
                            ↓
                    [Flip scope on OUTPUT]
                            ↓
                    Final hygienic expansion
```

---

## Key Data Structures

### Pattern (compiled pattern representation)

```rust
pub enum Pattern {
    Wildcard,                    // _ (matches anything)
    Literal(Value),              // Exact match required
    Var(PVRef),                  // Pattern variable
    List(Vec<Pattern>),          // (p1 p2 p3)
    Vector(Vec<Pattern>),        // #(p1 p2 p3)
    DottedList {                 // (p1 p2 . rest)
        patterns: Vec<Pattern>,
        tail: Box<Pattern>,
    },
    Ellipsis {                   // p ... (zero or more)
        subpattern: Box<Pattern>,
        level: usize,            // Nesting depth
        num_following: usize,    // Elements after ellipsis (Gauche optimization)
        vars: Vec<PVRef>,        // Variables in subpattern
    },
}
```

### Template (compiled template representation)

```rust
pub enum Template {
    Literal(Value),              // Self-evaluating constant
    Symbol(Identifier),          // Identifier (may need renaming)
    Var(PVRef),                  // Pattern variable substitution
    List(Vec<Template>),         // (t1 t2 t3)
    Vector(Vec<Template>),       // #(t1 t2 t3)
    DottedList {                 // (t1 t2 . rest)
        templates: Vec<Template>,
        tail: Box<Template>,
    },
    Ellipsis {                   // t ... or t ... ...
        subtemplate: Box<Template>,
        level: u8,               // Ellipsis level
        nesting: u8,             // 1 = single, 2 = double
        vars: Vec<PVRef>,        // Variables driving iteration
    },
}
```

### PVRef (Pattern Variable Reference)

```rust
pub struct PVRef {
    level: u8,   // Ellipsis nesting depth (0 = not in ellipsis)
    index: u8,   // Variable index within pattern
}
```

**Level semantics:**
- Level 0: Simple binding (e.g., `x` in `(foo x)`)
- Level 1: Single ellipsis (e.g., `x` in `(foo x ...)`)
- Level 2: Double ellipsis (e.g., `x` in `(foo (x ...) ...)`)

### MatchEnv (pattern matching result)

```rust
pub struct MatchEnv {
    bindings: Vec<Option<MatchValue>>,
}

pub enum MatchValue {
    Leaf(Value),              // Single binding
    Branch(Vec<MatchValue>),  // Ellipsis repetitions
}
```

**Tree structure for nested ellipsis:**
```
Pattern: ((a b ...) ...)
Input:   ((1 2 3) (4 5))

MatchEnv:
  a → Branch([Leaf(1), Leaf(4)])
  b → Branch([
        Branch([Leaf(2), Leaf(3)]),
        Branch([Leaf(5)])
      ])
```

### ScopeSet (hygiene tracking)

```rust
pub struct ScopeSet {
    scopes: SmallVec<[ScopeId; 4]>,  // Usually small
}

pub struct ScopeId(u32);  // Fresh ID per macro expansion
```

### IdentifierData (hygienic identifier)

```rust
pub struct IdentifierData {
    pub name: Rc<str>,
    pub scopes: ScopeSet,
}
```

---

## Hygiene Mechanism

Patina uses **Racket-style scope sets** for hygiene, avoiding the complexity of alpha-renaming.

### Core Concept

Each identifier carries a set of scopes. Two identifiers refer to the same binding if:
1. They have the same name, AND
2. Their scope sets have a subset relationship

### Flip-Scope Algorithm

The key insight is the **flip-scope** operation:

```
flip(id, scope) = if scope ∈ id.scopes
                  then id.scopes - {scope}
                  else id.scopes ∪ {scope}
```

**Expansion process:**

1. **Before matching:** Flip `macro_scope` on INPUT
   - Adds scope to use-site identifiers

2. **During expansion:** Template identifiers get their definition-time scopes

3. **After expansion:** Flip `macro_scope` on OUTPUT
   - Use-site identifiers: scope added then removed → unchanged
   - Introduced identifiers: scope not present then added → has macro_scope

### Example

```scheme
(define-syntax swap!
  (syntax-rules ()
    ((swap! a b)
     (let ((tmp a))
       (set! a b)
       (set! b tmp)))))

(let ((tmp 1) (x 2))
  (swap! x tmp))
```

**Hygiene in action:**
- `tmp` in template (introduced): gets `macro_scope`
- `tmp` in input (use-site): no `macro_scope`
- They are distinguished by their scope sets!

### Binding Forms and Introduced Identifiers

Racket's expander puts a binding form's scope on the form's body when it
enters the form. Patina's desugarer gets the same effect two ways, and which
one applies depends on what the reference is:

- A **symbol** written in source carries no scopes and is resolved with the
  scopes the desugarer has accumulated on the way to it (`current_scopes`),
  which include every enclosing binding form's. Nothing is added to the tree.
- An **identifier with scopes** — one a macro introduced — carries its own
  and nothing of where it now sits. On entering a `let-syntax` the desugarer
  therefore walks the body as written and adds the form's scope to every such
  identifier (`add_scope_to_scoped_identifiers`), and under `letrec-syntax`
  to the transformer forms as well. That is what tells a reference *in* the
  body from one a transformer will introduce later: chibi's `(m k)` binds `n`
  and references it from the same template, and the reference gets the
  scope; a sibling transformer's reference to `n` under `let-syntax` does not.
- An **identifier with empty scopes** is a user's symbol that passed through
  a pattern variable (the input flip gave it the expansion scope, the output
  flip took it away). It resolves like a symbol. When the template compiler
  meets one inside a transformer it compiles, it stamps the transformer's
  definition scopes on it — what a symbol written there would get — while a
  substituted identifier that already has scopes is emitted verbatim, since
  its identity is the outer expansion's and adding the inner macro's context
  would let `(let ((if …)) …)` capture a generator's `if`.

A `let-syntax` binder is bound at its own scopes plus the form's, where a
binder written in source stands in `definition_scopes`. Binding it unscoped
as well, so that any reference of that spelling could reach it, is what let a
`let-syntax ((quote …))` around a *call* capture the callee template's
`quote` (Larceny triage family 33).

### Literal Matching

Literals use `bound-identifier=?` semantics:

```rust
fn values_match_as_literal(pattern_lit: &Value, input: &Value) -> bool {
    match (pattern_lit, input) {
        (Value::Identifier(pat_id), Value::Identifier(inp_id)) => {
            // Empty pattern scopes = substituted from outer expansion
            if pat_id.scopes.is_empty() {
                return true;
            }
            // bound-identifier=?: same name AND subset relationship
            pat_id.name == inp_id.name
                && pat_id.scopes.is_subset_of(&inp_id.scopes)
        }
        // ... other cases
    }
}
```

---

## Pattern Matching

### Entry Point

```rust
// In matcher.rs
impl Matcher {
    pub fn match_pattern(&self, pattern: &Pattern, input: &Value)
        -> Result<MatchEnv, MatchError>
}
```

### Key Algorithms

**1. List Matching with Ellipsis**

```rust
fn match_list(&self, patterns: &[Pattern], input: &Value, ...) {
    // Gauche-style: num_following tells us how many elements
    // to reserve for patterns after the ellipsis
    let to_consume = remaining_input - num_following;

    for each consumed element:
        match subpattern, collect bindings into Branch
}
```

**2. Literal Shadowing Check**

```rust
fn is_literal_shadowed(&self, lit: &Value, input: &Value) -> bool {
    // If literal is in shadowed_names (from let bindings at use-site),
    // it should NOT match
    self.shadowed_names.contains(&input_name)
}
```

---

## Template Expansion

### Entry Point

```rust
// In expander.rs
impl Expander {
    pub fn expand(&self, template: &Template, env: &MatchEnv)
        -> Result<Value, ExpandError>
}
```

### Key Algorithms

**1. Variable Substitution with Scope Marking**

```rust
fn mark_substituted_value(&self, value: Value) -> Value {
    // Symbols from pattern variables get macro_scope added
    // This marks them as "came from input via substitution"
    match value {
        Value::Symbol(name) => {
            let scopes = ScopeSet::new().with_scope(self.macro_scope);
            Value::Identifier(Box::new(IdentifierData { name, scopes }))
        }
        // Recurse for pairs, vectors (but NOT into syntax-rules forms)
    }
}
```

**2. Ellipsis Iteration**

```rust
fn expand_single_ellipsis(&self, subtemplate: &Template, vars: &[PVRef], ...) {
    let count = get_iteration_count(first_var);

    for i in 0..count:
        new_indices[level] = i;
        expand_impl(subtemplate, env, &new_indices)
}
```

**3. Double Ellipsis (SRFI-149)**

```rust
fn expand_double_ellipsis(&self, ...) {
    // Pattern: ((a b ...) ...)
    // Template: (b ... ...)
    // Result: flattened list of all b values

    for outer_idx in 0..outer_count:
        for inner_idx in 0..inner_count:
            expand and collect
}
```

---

## Ellipsis Handling

### Pattern Compilation

```rust
// In compiler.rs, detecting ellipsis
fn next_is_ellipsis(&self, items: &[Value], idx: usize) -> bool {
    items.get(idx + 1).map_or(false, |v| {
        matches!(v, Value::Symbol(s) if self.ellipsis.as_ref() == Some(s))
    })
}

// Compiling ellipsis pattern
fn compile_ellipsis_pattern(&mut self, subpat: &Value, level: usize,
                            num_following: usize) -> Result<Pattern, MacroError>
```

### num_following Optimization

Gauche's insight: pre-calculate how many elements follow the ellipsis to avoid backtracking.

```
Pattern: (a ... b c)
num_following = 2  // b and c

Input: (1 2 3 4 5)
a matches: 1, 2, 3  (leave 2 for b, c)
b matches: 4
c matches: 5
```

### Ellipsis Escaping

```scheme
(define-syntax define-begin-like
  (syntax-rules ()
    ((_ name)
     (define-syntax name
       (... (syntax-rules ()      ; <-- Escape: treat ... as literal
              ((name x ...)
               (begin x ...))))))))
```

**Implementation:**

```rust
fn compile_with_escaped_ellipsis(&mut self, form: &Value, level: usize) {
    let saved = self.ellipsis.take();  // Disable ellipsis recognition
    let result = self.compile_template(form, level);
    self.ellipsis = saved;             // Restore
    result
}
```

---

## Integration with Evaluator

### Macro-Aware Desugaring

The desugarer (in `patina-frontend`) checks for macros during AST → CoreExpr conversion:

```rust
// In desugarer/mod.rs
fn desugar_list(&mut self, list: &Value, env: &Rc<Environment>) -> Result<CoreExpr> {
    // Check if operator is a macro
    if let Some(Value::Macro(compiled)) = env.get(&name) {
        // Expand the macro
        let expanded = expand_macro(compiled, form, env)?;
        // Recursively desugar the result
        return self.desugar(&expanded, env);
    }
    // ... normal processing
}
```

### Flip-Scope Integration

```rust
// In interface.rs
pub fn expand_macro_with_hygiene(
    compiled: &CompiledMacro,
    form: &Value,
    env: &Rc<Environment>,
) -> Result<Value, MacroError> {
    let macro_scope = ScopeId::fresh();

    // Step 1: Flip scope on input (mark use-site identifiers)
    let flipped_input = flip_scope(form, macro_scope);

    // Step 2: Try each clause
    for (pattern, template) in &compiled.clauses {
        if let Ok(match_env) = matcher.match_pattern(pattern, &flipped_input) {
            let expanded = expander.expand(template, &match_env)?;

            // Step 3: Flip scope on output
            return Ok(flip_scope(&expanded, macro_scope));
        }
    }

    Err(MacroError::NoMatchingPattern)
}
```

---

## Debug Infrastructure

Enable macro debugging in the REPL with:

```scheme
(macro-debug-mode 'on)   ; Enable debug output
(macro-debug-mode 'off)  ; Disable debug output
```

Or programmatically in Rust:

```rust
patina_runtime::macro_debug::enable();
patina_runtime::macro_debug::disable();
```

### Debug Output

```
[MACRO] Expanding macro: my-or
[MACRO]   Clause 1:
[MACRO]     Pattern: (my-or)
[MACRO]     Input: (my-or #f 1)
[MACRO]     Match: FAILED (TooFewElements)
[MACRO]   Clause 2:
[MACRO]     Pattern: (my-or ?test)
[MACRO]     Input: (my-or #f 1)
[MACRO]     Match: FAILED (TooManyElements)
[MACRO]   Clause 3:
[MACRO]     Pattern: (my-or ?test ?rest ...)
[MACRO]     Input: (my-or #f 1)
[MACRO]     Match: SUCCESS
[MACRO]     Bindings:
[MACRO]       test = #f
[MACRO]       rest = [1]
[SCOPE-SETS] Introduced 'let' (will get macro scope S42 on output flip)
[SCOPE-SETS] Free variable 'my-or' with scopes {}
```

### Scope trace — what bindings actually get bound at

`macro-debug-mode` narrates *expansion*. When the question is instead "what
scope set does this binding actually carry, and how was that reference
answered", use `PATINA_SCOPE_TRACE`:

```bash
PATINA_SCOPE_TRACE=/tmp/trace.txt ./target/release/patina --tree-walker prog.scm
```

**Use an absolute path** — the compat harness and the Larceny runner both `cd`
into a scratch directory they then delete.

**Trace one program, not a suite.** There is no sampling and no deduplication,
so cost scales with work done, not code size: a 300 000-iteration loop measured
**14x slower and 352 MB of trace**. Shrink the program first.

```text
RUN pid=4711
BIND    phase=desugar name="c" scopes={S136} byname=false
BIND    phase=run name="c" scopes={} byname=true
RESOLVE phase=run name="c" ref={S137} cands=0 picked=- via=byname op=set
WROTE   phase=run name="c" ref={S137} landed=byname
```

Those four lines were triage family 36 (fixed 2026-08-31; the `BIND
phase=run` line now reads `scopes={S136} byname=true`). The desugarer stamped
the internal define with `{S136}`; the binding that reached the runtime
carried nothing, so a reference at `{S137}` had no candidate, fell back to
spelling, and the write landed somewhere the rule never chose. A `let` binder
traced the same way kept its scopes at `phase=run` — that contrast was the
axis the family turned on, and is still the shape of trace this instrument
exists to catch.

| field | meaning |
|---|---|
| `phase` | `desugar` / `compile` (the VM's renamer) / `run`. Without it the desugarer's lookups and the evaluator's interleave indistinguishably. |
| `scopes` | what the binding is *filed under*. `{}` means the plain by-name table, where set-of-scopes resolution cannot see it. |
| `byname` | `visible_by_name` — whether a name-only lookup may also reach it. The difference between a parameter and a definition. |
| `cands` | how many bindings **passed** `is_candidate` — the same meaning at every site, so the backends can be compared. |
| `picked` / `via` | which binding won, and how it ended: `scoped`, `byname` (the rule declined and the by-name fallback answered), `unbound` (the rule declined and the fallback found nothing — on a name whose `BIND` shows a scoped binding, that is the fallback *refusing* a rejected binding, the family-36 rule at work), `ambiguous` (two candidates, neither more specific). At `phase=compile` the VM defers its fallback to runtime, so `byname` there means only "left to a by-name lookup". |
| `op` | `get`, `set`, or `bind` for a binding occurrence being renamed. The VM's renamer serves all three through one function. |
| `landed` | on `WROTE`: where a scoped write finally went. A write walks environment by environment, so its trace is a sequence; this is its conclusion. |

`via=byname` is common — every reference to a global from a scoped context is
one — so treat it as a filter, not a verdict. What is worth grepping is
`via=unbound` on a name that *has* a name-visible scoped binding two lines
above: resolution rejected that binding and the fallback declined to
resurrect it by spelling (triage family 36's rule). If such a read instead
comes back `via=byname` with the rejected binding's own value, the rule has
been broken.

Records are **not** deduplicated (unlike `PATINA_AMBIGUITY_LOG`) because order
and repetition are the signal. Off, the trace costs a `OnceLock` read and a
branch — measured on the tree-walker's hot path, interleaved, at no detectable
cost.

The paired instrument is `crates/patina-tests/tests/hygiene_matrix.rs`, which
enumerates hygiene *shapes* and scores both backends against chibi and Racket.
The trace explains a row; the matrix says which rows exist.

---

## Future: syntax-case

`syntax-case` is the procedural macro system that underlies `syntax-rules`. While `syntax-rules` is pattern-based and declarative, `syntax-case` provides full programmatic control.

**For detailed design and implementation plan, see [`PRD/phase2/SYNTAX_CASE_DESIGN.md`](../PRD/phase2/SYNTAX_CASE_DESIGN.md).**

### Why syntax-case?

`syntax-rules` cannot:
- Inspect syntax objects programmatically
- Generate identifiers dynamically
- Use guards/fenders on patterns
- Implement complex transformations

### Compatibility with Current Implementation

The current scope-set hygiene system is **fully compatible** with syntax-case:
- `ScopeSet` already tracks binding context
- `IdentifierData` carries hygiene information
- Flip-scope algorithm works for any macro type

**Key insight:** syntax-case adds procedural control over pattern matching and template construction—the underlying hygiene mechanism remains unchanged.

### Implementation Phases (Summary)

1. **Syntax Objects** - Add `Value::Syntax` with lexical context
2. **Context Utilities** - `datum->syntax`, `bound-identifier=?`, etc.
3. **syntax-case Form** - Pattern matching with fenders
4. **Quasisyntax** - Template construction (`#``, `#,`, `#,@`)
5. **syntax-rules via syntax-case** - Reimplement as macro
6. **Polish** - Error messages, documentation

---

## References

### Papers

1. **"Binding as Sets of Scopes"** - Matthew Flatt, 2016
   - Foundation for Patina's hygiene implementation
   - https://www.cs.utah.edu/plt/scope-sets/

2. **"Hygienic Macro Expansion"** - Kohlbecker et al., 1986
   - Original hygiene algorithm (historical reference)

3. **"Syntactic Abstraction in Scheme"** - Dybvig et al., 1993
   - `syntax-case` design paper

### Reference Implementations

1. **Gauche** - Pattern matching with num_following optimization
   - `src/macro.c`
   - https://github.com/shirok/Gauche

2. **Chibi-scheme** - Compact R7RS implementation
   - `lib/init-7.scm`, `eval.c`
   - https://github.com/ashinn/chibi-scheme

3. **Racket** - Scope sets implementation
   - Expander in `racket/src/expander/`

### Patina Source Files

| File | Purpose |
|------|---------|
| `crates/patina-macros/src/macro_expander/compiler.rs` | Pattern/template compilation |
| `crates/patina-macros/src/macro_expander/matcher.rs` | Pattern matching |
| `crates/patina-macros/src/macro_expander/expander.rs` | Template expansion |
| `crates/patina-macros/src/macro_expander/interface.rs` | Entry point, flip-scope |
| `crates/patina-runtime/src/scope.rs` | ScopeId, ScopeSet |
| `crates/patina-runtime/src/value/hygiene.rs` | IdentifierData |
| `lib/scheme/base-extras.scm` | Core macros (let, cond, case, do, etc.) |

### Test Files

| File | Coverage |
|------|----------|
| `crates/patina-tests/tests/hygiene.rs` | Hygiene edge cases |
| `crates/patina-tests/tests/compliance/macros_advanced.rs` | Advanced macro features |
| `crates/patina-tests/tests/macro_expander_interface.rs` | Interface tests |

---

## Known Limitations

See `internal/MACRO_SYSTEM_KNOWN_LIMITATIONS.md` for:
- Edge case with binding before macro definition
- Workarounds and explanations
