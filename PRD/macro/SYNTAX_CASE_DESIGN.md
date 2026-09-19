# Syntax-Case Implementation Design

**Status:** Future Enhancement
**Priority:** Medium (after Phase 1 R7RS compliance)
**Estimated Complexity:** High
**Prerequisites:** Current `syntax-rules` implementation complete

---

## Executive Summary

This document outlines the design for implementing `syntax-case`, the procedural macro system that underlies `syntax-rules` in R6RS and many Scheme implementations. While `syntax-rules` is pattern-based and declarative, `syntax-case` provides full programmatic control over macro expansion.

**Key Insight:** Patina's existing scope-set hygiene infrastructure is fully compatible with `syntax-case`. The main work is adding syntax objects and procedural interfaces—the hygiene mechanism remains unchanged.

**Counterpoint recorded 2026-08-31:** "remains unchanged" is compatible-with, not settled-for. This rewrite is the one natural moment to *consolidate* hygiene instead of carrying its current distribution forward — see [Resolve Once, Before the Backends](#resolve-once-before-the-backends) below, added after the 2026-08 hygiene defect queue (triage families 33–39) closed.

---

## Table of Contents

1. [Motivation](#motivation)
2. [Resolve Once, Before the Backends](#resolve-once-before-the-backends)
3. [Scoped Relinking, Sized](#scoped-relinking-sized)
4. [Deferred Mechanization (H5)](#deferred-mechanization-at-the-syntax-case-boundary-h5)
5. [Current State](#current-state)
6. [Syntax Objects](#syntax-objects)
7. [Core Forms](#core-forms)
8. [Hygiene Utilities](#hygiene-utilities)
9. [Quasisyntax](#quasisyntax)
10. [Implementation Phases](#implementation-phases)
11. [Integration Strategy](#integration-strategy)
12. [Testing Strategy](#testing-strategy)
13. [References](#references)

---

## Motivation

### Limitations of syntax-rules

`syntax-rules` cannot:

1. **Inspect syntax programmatically**
   ```scheme
   ;; Cannot check if identifier is bound
   (define-syntax safe-set!
     (syntax-rules ()
       ((_ var val)
        ;; How to check if var is defined?
        (set! var val))))
   ```

2. **Generate identifiers dynamically**
   ```scheme
   ;; Cannot create foo-getter, foo-setter from foo
   (define-syntax define-property
     (syntax-rules ()
       ((_ name)
        ;; Need to construct name-getter, name-setter
        ...)))
   ```

3. **Use guards/fenders on patterns**
   ```scheme
   ;; Cannot reject non-identifier test
   (define-syntax my-when
     (syntax-rules ()
       ((_ test body ...)
        ;; What if test is not an identifier?
        (if test (begin body ...)))))
   ```

4. **Implement complex transformations**
   - Loop unrolling based on literal count
   - Conditional code generation
   - Error messages with source location

### Benefits of syntax-case

- Full Scheme available during expansion
- Pattern matching with guards
- Programmatic syntax construction
- Better error messages
- Foundation for syntax-rules (can be a macro!)

---

## Resolve Once, Before the Backends

*(Added 2026-08-31 — a decision this rewrite must make, recorded while the
lesson is fresh.)*

Today hygiene is distributed: the desugarer stamps scopes and keeps a
desugar-time environment, the tree-walker resolves scoped bindings **at
runtime** (`Environment`'s scoped tables, by-name views and fallbacks), the VM
resolves **at compile time** (`alpha_rename`), and the CPS transform carries
binder scopes of its own. Every defect in triage families 33–39 was an
interaction between two of those copies, and the chibi suite read 1226/1226 on
both backends through all of them because both backends share the desugarer.

The VM's `alpha_rename` proves the endgame locally: past that pass, scope sets
are *gone* — everything downstream is plain, uniquely-named references. The
decision for this rewrite: run expansion **and resolution** once, in the
shared pipeline, and hand *both* backends fully-resolved code. Then:

- `Environment` needs no scoped table, no `visible_by_name` views, and no
  by-name fallback at runtime — the entire class of fallback-capture defects
  (family 36) becomes unrepresentable rather than guarded against;
- hygiene lives in **one specified algorithm**, which is also the point at
  which [deferred mechanization (H5)](#deferred-mechanization-at-the-syntax-case-boundary-h5)
  can be tied to the implementation's specification;
- Track Q's consolidation queue (Q7) collapses from five items into the
  removal of code the resolved IR no longer reads.

What the design must solve before committing (the reasons this is a rewrite
boundary and not a refactor of the current architecture):

1. **Runtime code creation.** `eval`, `interaction-environment`, `load` and
   REPL redefinition create and resolve bindings at runtime; a resolve-once
   pipeline needs a story for late-arriving code (re-run the resolver per
   `eval` unit, as the VM effectively does today).
2. **Definition-environment relinking.** Macro-generated macros resolve
   library-private helpers by name today (the `jabberwocky` constraint —
   Track L §6). Resolved IR must carry those links as bindings. *Sized
   2026-09-13 in [Scoped Relinking, Sized](#scoped-relinking-sized): the
   steal defect once recorded here is already fixed; a mutation indicates the
   by-name views can be deleted without this rewrite, pending the Larceny
   lanes, the suite oracles and the compat corpus, which it did not run; and
   the relinking change left is contained.*
3. **Cross-backend contract.** The resolved IR becomes the backend interface;
   `hygiene_matrix.rs` and Track H's harnesses preserve the established correct
   binding behavior. The 28-shape matrix passes against chibi and Racket;
   additional shapes retain named defects. The rewrite must meet the justified
   answers and retire the corresponding quarantines when they pass.

---

## Scoped Relinking, Sized

*(Added 2026-09-13, corrected 2026-09-14. Track Q's Q7.5(b) is gated on "a
written design note", and prerequisite 2 above names the same work; this is
that note. The mutation below ran on `main` at `9105d328`. The Patina columns
of the first table were first measured with #318's build and re-measured on
`main` at `42fffa05`, after #321: every answer is unchanged, and the one
mechanism that changed is noted in its row. Oracles: chibi 0.12 and Gauche
0.9.15.)*

Prerequisite 2 says macro-generated macros resolve library-private helpers
*by name*, and that resolved IR must carry those links as bindings. Checked
against the code, the by-name path turns out narrower and less load-bearing
than that statement, so this section starts from measurements rather than from
the constraint as recorded.

### What answers a macro-introduced definition by its spelling

| | Where | What it does |
|---|---|---|
| **V1** | VM, `compile_pipeline` | `env.define_alias(bare, env, renamed)` for each macro-introduced *top-level* definition the renamer renamed. `Environment::get`/`set` consult it after plain bindings, so `LoadGlobal`'s cache-miss path reaches it. |
| **V2** | VM, `alpha_rename::rename_body` | Splices `(define bare renamed)` beside each renamed *body* definition. |
| **V3** | tree-walker, `step.rs` `Define` | `define_scoped_definition`: filed under its scopes *and* visible by name. `get`/`set` read the name-only view; `get_scoped_fallback`/`set_scoped_fallback` refuse it to a scoped reference that rejected it. |
| **V4** | desugarer, `desugar_define_syntax_tagged` | `env.define(name, macro)`: a keyword binds by spelling whatever scopes its name carries. |

V1–V3 exist for one consumer, `link_definition_env_refs`, and their docs say
so. It rewrites a template's free references so they resolve where the macro
was defined; it decides **once per spelling** over `template_symbols`, detects
a candidate with the name-only `def_env.get(name)`, and installs an alias whose
target `(def_env, name)` is looked up by name on every access. A definition
reachable only under its scopes is invisible to all three steps.

V3's table has two more users that are not in scope, and a change to it has to
leave them alone: a parameter written in source (`application.rs`) and an
internal define written in source, which the CPS transform's `define_scopes`
stamps with its body's scopes, are both stored scoped *and* visible by name,
which is how source references reach them. V4 is not a relinking artifact either. The
call never looks at the name's scopes, which makes it #269's `define-syntax`
half and the one expected failure in `introduced-definitions.scm`. It has been
grouped with the family-40 rows as "scopes surviving the renamer"; neither the
code nor the measurement below supports that, and it is fixable in the
desugarer on its own.

### What they serve, measured

| Shape | VM | TW | chibi | Gauche | Reaches V1–V4? |
|---|---|---|---|---|---|
| Jabberwocky in one program (chibi's `r7rs-tests.scm`) | 42 | 42 | 42 | 42 | No, since #321. The VM compiles the reference to the introduced global's identity (#315, `via=scoped`) and the tree-walker reads its scoped table. Before #321 the VM's *desugar-time* read of the same reference, the check for syntax used as a value, found no candidate and fell back by name through V1 (`phase=desugar … cands=0 via=byname`); since #321 it resolves `via=scoped` there too. That fallback only ever answered "not syntax", which a deleted V1 answers as well |
| Track L §6, "a later user global steals it" | 10 | 10 | 10 | 10 | No. Recorded there as 99; the VM's compile-time resolution now picks the introduced global's identity (`via=scoped`), so the user's global never competes, and §6 was stale |
| Track L §6, "two expansions share one binding" | (10 20) | (10 20) | (10 20) | (10 20) | No. Recorded as (20 20); same |
| Jabberwocky in a library, generated macro used inside it | 10 | 10 | 10 | 10 | No: one environment |
| **Jabberwocky in a library, generated macro exported to the program** | **error** | **error** | 10 | 10 | Never reached — below |
| Family 40, `(def-x)` then `(use-x)` | 10 | error | error | error | **V1** |
| Family 40, `use-x` compiled inside a procedure before `(def-x)` runs | 10 | error | error | error | **V1**, at run time |
| A source reference to a top-level introduced define | 1 | 1 | unbound | unbound | V1 / V3 — R7RS §4.3.2 permits either at top level |
| A library exporting a top-level introduced define by its bare name | 1 | 1 | unbound | unbound | V1 / V3 |
| #269, `(let () (def-var) hidden-v)` | 10 | 10 | unbound | unbound | V2 / V3 — a defect, since this is a body |
| #269, `(let () (def-mac) (hidden-m))` | m | m | unbound | unbound | V4 |

The exported-getter row is the shape the by-name path exists for, and it fails
before reaching it. `CompiledMacro::collect_template_symbols` skips
`Template::Literal`, which is how the template compiler emits an identifier
that already carries scopes — exactly the generated macro's reference to the
introduced definition. So it is never a template symbol and never relinked,
and it reaches the program carrying the library's scopes: `PATINA_SCOPE_TRACE`
shows `RESOLVE phase=desugar name="mh" ref={S136,S137} cands=0 … via=unbound`
on both backends. The only record of this was a comment in
`link_definition_env_refs` ("dies earlier at the Template::Literal skip").

**In every measured shape, V1–V3 either answer a reference chibi and Gauche
refuse, or are never reached.**

### Removing them, measured

What depends on the views was measured rather than argued, in a scratch
worktree at `origin/main` (`9105d328`) with three edits:

- **V1:** drop the `env.define_alias(bare, env.clone(), renamed.name)` call in
  `compile_pipeline` (`patina-vm/src/compiler/mod.rs`), keeping the
  `define_introduced_global` call beside it.
- **V2:** in `alpha_rename::rename_body`'s non-top-level arm, push only the
  renamed `Define`, without the `(define bare renamed)` spliced after it.
- **V3:** add `introduced: bool` to `CpsExprKind::Define`, set in the CPS
  transform's `Define` arm from the name's scopes *before* `define_scopes`
  replaces them, and in `step.rs`'s `Define` arm call `define_with_scopes`
  when it is set and `define_scoped_definition` otherwise.

The flag is the part to get right. A first attempt that keyed on non-empty
scopes in `step.rs` alone hid every source-written internal define too, since
`define_scopes` scopes those as well; its 40 failures were all that mistake,
all on the tree-walker, and none are counted here.

| Check | Result |
|---|---|
| `cargo test --all --lib --tests --no-fail-fast` | 1194 passed, 1 failed |
| The one failure | `hygiene.scm`'s three family-40 expected failures pass on the VM — the quarantine reporting a fix |
| chibi's R7RS suite, VM and tree-walker | 1226/1226 each, jabberwocky included |
| The probes above | family 40 refuses on both backends, both variants; #269's `define` half is unbound on both; a top-level source reference is unbound; a library exporting an introduced define fails to load, as it does in chibi and Gauche; the exported getter still errors; #269's `define-syntax` half is unchanged |
| Not run | the Larceny lanes, the suite oracles, the compat corpus |

**Nothing `cargo test` or the chibi lanes cover depends on V1–V3 for a
macro-introduced definition.** The Larceny lanes, the suite oracles and the
compat corpus have not been asked, and they are where third-party code relying
on a bare-name top-level definition would show. On this evidence, deleting the
views closes family 40 and #269's `define` half, and does not need the
relinker fixed first, because the relinker never reached those shapes.

### The design

A macro-introduced definition is reachable through its binding identity — its
name and the scope set it was introduced at — and never through its spelling.
Measured above, that is two independent changes, and a third that only looked
related.

1. **Delete the by-name views.** V1 and V2 on the VM. On the tree-walker a
   macro-introduced `define` binds at its scopes only, which needs the CPS
   `Define` to carry whether the name was introduced, since `define_scopes`
   erases that today. `define_introduced_global` stays: it is the identity. A
   scoped reference with no scoped candidate still compiles to a bare
   `LoadGlobal`, and with only plain bindings left to answer it the family-40
   rows refuse, including the compiled-before-defined variant that no
   compile-time check could reach. #269's `define` half closes on both
   backends. The doc comments justifying the views go with them
   (`Renamed::global_aliases`, `define_alias`'s bare-name kind,
   `define_scoped_definition`, `rename_body`). What stays: the scoped tables,
   `visible_by_name` for source-written parameters and internal defines, and
   the fallbacks' refusal arms that guard them. Removing those is
   resolve-once's work, not this.
2. **Relink an identifier to a binding.** *(Landed in two parts: #402 /
   #405 on 2026-09-18 made a scoped identifier in `Template::Literal` a
   mention at all — `CompiledMacro::inherited_identifiers` — and #408 on
   2026-09-19 made the alias name the binding. The note after this item says
   how the question it ends on was settled. Step 1 has **not** landed, so the
   by-name views are still there; nothing in step 2 as built leans on them —
   the relinker finds an introduced definition by its identity and falls back
   to the name only where the mention's scopes select what the name reaches.)*
   Needed for the exported getter, and for nothing else measured. `template_symbols` becomes the template's free
   *identifiers*, `(name, scopes)`, taken from `Template::Symbol` and from
   scoped identifiers in `Template::Literal`. Each resolves in the definition
   environment at its own scopes — `get_with_scopes` on the tree-walker's
   table, `for_each_introduced_global` for a VM-renamed global — and the alias
   installed for it names the binding found: a plain name for a plain or
   renamed global, `(name, scopes)` for a scoped one, which `alias_target`
   then reads with the scoped read rather than `get`. Rewriting stays keyed on
   the expansion scope (family 35's discriminator) and adds the scope set, so
   two occurrences of one spelling from different expansions get different
   aliases. With step 1 landed first there is no by-name view left for this to
   lean on, which is the point.

   One question the step has to settle before it is written. A scoped
   identifier in `Template::Literal` keeps the identity an *outer* macro gave
   it, so the binding it names lives where that outer macro's template was
   written. That need not be the generated macro's definition environment:
   a macro from one library can generate a macro inside another. Resolved in
   the generated macro's environment, such an identifier finds nothing, or a
   different binding of the same spelling. It has to carry the environment
   that gave it its identity, or the step has to show the two cannot differ.
   *Settled 2026-09-19 (#408), by measurement.* The binding a definer
   introduces lives where the definer was **run**, and that is the generated
   macro's definition environment, whichever library wrote the definer: a
   definer owned by one library and run in a second puts `(define total 0)`
   in the second, and the macro it generates there is defined there too. So
   resolving the mention in the generated macro's environment, at the
   mention's own scopes, finds it (`Environment::introduced_definition`). What
   the *writing* library contributes to such a template is its own private
   names — `step` beside `total` in `expansion/template-references.scm`'s
   three-library row — and those are plain bindings the outer expansion's
   relinking had already aliased by the time the inner macro was compiled.
   chibi 0.12 and Gauche 0.9.15 agree with both backends on that row and on a
   three-level generation. As built, only a definition held by the *root* of
   the definition environment's chain is aliased; a scoped binding in a child
   frame is lexical and stays with scoped resolution.
3. **V4, separately:** bind a `define-syntax` at its name's scopes. #269's
   keyword half, and independent of both.

### Behaviour it changes

Everything in the first table moves to the chibi and Gauche answer. Two of those
moves are at top level, where R7RS permits either answer and code written
against Patina could rely on the current one: a source reference to a
macro-introduced definition becomes unbound, and a library exporting one by its
bare name fails to load. So the compat corpus, whose snapshot is known to go
stale silently, has to run against step 1 before it lands, as do the Larceny
lanes and the suite oracles, which the experiment did not run.

### Guards

Step 1: the three family-40 rows in `hygiene.scm` lose `test-expect-fail`, and
new rows pin #269's `define` half in a body and the compiled-before-defined
family-40 variant. Step 2: a new row for the exported getter, with its library
under `test-lib/`. Step 3: the row in `introduced-definitions.scm` loses its
expectation. Required green throughout: `hygiene_matrix.rs`, both chibi lanes
(jabberwocky), SRFI 101's shadowing suite, the three Larceny lanes, the suite
oracles, and the compat corpus.

### What this sizes

Prerequisite 2 does not size the rewrite up. Step 1 is three small deletions, a
flag on one CPS node, and the doc comments that justified them; step 2 is one
desugarer function and its rewrite keying, `collect_template_symbols`, and an
optional scope set on `AliasTarget`. After both, every relinking alias names a
binding and no runtime path answers an introduced definition by its spelling,
which is what the prerequisite asks resolved IR to carry.

What resolve-once still has to solve is elsewhere. The obstacle the Q7 deferral
did not name is that the desugarer's binding table *is* the runtime
`Environment`; then there are the scoped tables and fallbacks that exist for the
tree-walker's per-read resolution; and prerequisite 1, which the pipeline
already meets by resolving per top-level form.

---

## Deferred Mechanization at the Syntax-Case Boundary (H5)

**Status:** deferred; transferred from the completed
[Track H assurance plan](../ARCHIVE/completed_planning/TRACK_H_HYGIENE_ASSURANCE_PRD.md)
on 2026-09-12. No proof implementation or new toolchain is committed by this
design, and no H5 issue is needed until the rewrite reaches this decision.

Revisit mechanization when expansion and resolution have one specified
algorithm and an explicit resolved-IR contract. A separate model of today's
distributed desugarer, relinker and backend fallbacks would need to track
several moving implementations. H4 evaluated that cost and declined an
additional verifier or reference expander for the current architecture.

Lean 4 is a candidate host: Ullrich and de Moura's
[*Beyond Notations: Hygienic Macro Expansion for Theorem Proving Languages*](https://arxiv.org/abs/2001.10490)
(IJCAR 2020; expanded in LMCS 2022) describes the hygienic macro system
implemented in Lean 4. That is relevant design experience, not an existing
correctness proof for Patina or a reason to select a proof assistant before
the specification is settled.

The future evaluation must establish:

1. **Binding specification and scope.** Define identifier identity, literal
   matching, introduced definitions, imports/phases, and generated macros.
   State how late code from `eval`, `load` and the REPL is resolved, and which
   forms, termination assumptions or expansion-fuel bounds are excluded.
2. **Connection to production Rust.** Specify the refinement, extraction or
   executable correspondence that keeps the formal algorithm aligned with the
   shared resolver. List trusted components and assumptions explicitly.
3. **Observable obligations.** Preserve binding relationships under renaming;
   read and assignment must select the same binding; ambiguous or unbound
   accesses must obey their specified outcomes. Keep H1–H3 and the matrix as
   executable checks, including historical failures and known-defect guards.
4. **Cost and maintenance decision.** Compare the candidate tools against the
   actual algorithm, give a scoped pilot and reproducible proof results, and
   name the ongoing contract/toolchain maintenance. A decision against
   mechanization remains valid if its benefit does not justify that cost.

This work belongs to the syntax-case rewrite. Of the runtime defects it was
waiting on, #289–#291 were closed by #316 and triage family 41 by #318 and
#321; family 40 remains open, and neither a future proof nor the archiving of
Track H closes it.

---

## Current State

### What We Have

1. **Scope-set hygiene** (Racket-style)
   - `ScopeId` - unique identifier per expansion
   - `ScopeSet` - set of scopes on each identifier
   - `IdentifierData` - name + scopes
   - Flip-scope algorithm for hygiene

2. **Pattern matching infrastructure**
   - `Pattern` enum with PVREF encoding
   - `Matcher` with Gauche-style optimization
   - `MatchEnv` tree structure

3. **Template expansion**
   - `Template` enum
   - `Expander` with scope marking
   - Ellipsis iteration

4. **Value::Macro**
   - Stores compiled macros
   - Integrated with desugarer

### What We Need

1. **Syntax objects** - First-class syntax with source info
2. **syntax-case form** - Pattern matching with fenders
3. **Syntax utilities** - `datum->syntax`, `syntax->datum`, etc.
4. **Quasisyntax** - Template construction

---

## Syntax Objects

### Design

A syntax object wraps a datum with lexical context and source location:

```rust
/// A syntax object - datum with lexical context
#[derive(Debug, Clone)]
pub struct SyntaxObject {
    /// The wrapped datum
    pub datum: Value,

    /// Lexical context (scope set)
    pub context: ScopeSet,

    /// Source location (optional)
    pub source: Option<SourceLocation>,
}

#[derive(Debug, Clone)]
pub struct SourceLocation {
    pub file: Option<Rc<str>>,
    pub line: u32,
    pub column: u32,
    pub span: Option<(u32, u32)>,  // start, end offset
}
```

### Value Integration

Add to the Value enum:

```rust
pub enum Value {
    // ... existing variants ...

    /// Syntax object (first-class syntax)
    Syntax(Box<SyntaxObject>),
}
```

### Wrapped vs Unwrapped

Syntax objects can wrap:
- **Atoms**: symbols, numbers, strings, etc.
- **Pairs**: each element is itself a syntax object
- **Vectors**: each element is itself a syntax object

```scheme
;; Creating syntax
#'(if test then else)
;; Produces:
;; Syntax(Pair(
;;   Syntax(Symbol("if")),
;;   Syntax(Pair(
;;     Syntax(Symbol("test")),
;;     ...))))
```

### Identifier Syntax Objects

When the datum is an identifier, the syntax object carries binding information:

```rust
impl SyntaxObject {
    /// Check if this is an identifier
    pub fn is_identifier(&self) -> bool {
        matches!(self.datum, Value::Symbol(_) | Value::Identifier(_))
    }

    /// Get the identifier name
    pub fn identifier_name(&self) -> Option<Rc<str>> {
        match &self.datum {
            Value::Symbol(s) => Some(s.clone()),
            Value::Identifier(id) => Some(id.name.clone()),
            _ => None,
        }
    }

    /// Get the combined scopes (context + datum scopes)
    pub fn scopes(&self) -> ScopeSet {
        match &self.datum {
            Value::Identifier(id) => self.context.union(&id.scopes),
            _ => self.context.clone(),
        }
    }
}
```

---

## Core Forms

### syntax (quote-syntax)

Creates a syntax object from a template:

```scheme
(syntax datum)    ; or #'datum
```

**Semantics:**
- Wraps datum in syntax object
- Captures lexical context at definition site
- Preserves source location from input

**Implementation:**

```rust
// In special_forms/
fn eval_syntax(form: &Value, env: &Rc<Environment>) -> Result<Value> {
    let datum = get_arg(form, 0)?;

    // Get current scope set from environment
    let context = get_current_scopes(env);

    // Get source location if available
    let source = get_source_location(datum);

    Ok(Value::Syntax(Box::new(SyntaxObject {
        datum: datum.clone(),
        context,
        source,
    })))
}
```

### syntax-case

Pattern matching on syntax objects:

```scheme
(syntax-case stx (literal ...)
  (pattern fender template)
  (pattern template)
  ...)
```

**Semantics:**
- `stx` - syntax object to match
- `literal ...` - literal identifiers (like syntax-rules)
- `pattern` - pattern to match (like syntax-rules)
- `fender` - optional guard expression (must return true)
- `template` - expression to evaluate if match succeeds

**Key difference from syntax-rules:** The template is an *expression* that is *evaluated*, not a template that is *expanded*.

**Implementation:**

```rust
fn eval_syntax_case(
    stx_expr: &Value,
    literals: &[Rc<str>],
    clauses: &[(Pattern, Option<Value>, Value)],
    env: &Rc<Environment>,
) -> Result<Value> {
    let stx = eval(stx_expr, env)?;

    for (pattern, fender, body) in clauses {
        // Try to match pattern against stx
        if let Ok(match_env) = match_syntax(pattern, &stx, literals) {
            // Bind pattern variables in environment
            let clause_env = extend_with_matches(env, &match_env);

            // Check fender if present
            if let Some(fender_expr) = fender {
                let fender_result = eval(fender_expr, &clause_env)?;
                if !is_true(&fender_result) {
                    continue;  // Fender failed, try next clause
                }
            }

            // Evaluate body
            return eval(body, &clause_env);
        }
    }

    Err(Error::NoMatchingSyntaxCase)
}
```

### with-syntax

Bind pattern variables for use in templates:

```scheme
(with-syntax ((pattern stx) ...)
  body ...)
```

**Example:**
```scheme
(with-syntax (((name val) #'(x 42)))
  #'(define name val))
;; => #'(define x 42)
```

**Implementation:** Syntactic sugar over `syntax-case`:

```scheme
(define-syntax with-syntax
  (syntax-rules ()
    ((_ ((pat expr) ...) body ...)
     (syntax-case (list expr ...) ()
       ((pat ...) (begin body ...))))))
```

---

## Hygiene Utilities

### identifier?

Check if syntax object is an identifier:

```scheme
(identifier? stx) → boolean
```

```rust
fn identifier_p(stx: &Value) -> Value {
    match stx {
        Value::Syntax(s) => Value::Boolean(s.is_identifier()),
        _ => Value::Boolean(false),
    }
}
```

### bound-identifier=?

Check if two identifiers have the same binding:

```scheme
(bound-identifier=? id1 id2) → boolean
```

**Semantics:** Returns true if both would refer to the same binding at macro use site.

```rust
fn bound_identifier_eq(id1: &Value, id2: &Value) -> Value {
    match (id1, id2) {
        (Value::Syntax(s1), Value::Syntax(s2)) => {
            let scopes1 = s1.scopes();
            let scopes2 = s2.scopes();
            let name1 = s1.identifier_name();
            let name2 = s2.identifier_name();

            Value::Boolean(
                name1.is_some() &&
                name1 == name2 &&
                scopes1 == scopes2
            )
        }
        _ => Value::Boolean(false),
    }
}
```

### free-identifier=?

Check if two identifiers refer to the same free binding:

```scheme
(free-identifier=? id1 id2) → boolean
```

**Semantics:** Returns true if both would resolve to the same top-level binding.

```rust
fn free_identifier_eq(id1: &Value, id2: &Value, env: &Rc<Environment>) -> Value {
    // Resolve both identifiers to their bindings
    let binding1 = resolve_identifier(id1, env);
    let binding2 = resolve_identifier(id2, env);

    Value::Boolean(binding1 == binding2)
}
```

### syntax->datum

Strip syntax wrapper, returning plain datum:

```scheme
(syntax->datum stx) → datum
```

```rust
fn syntax_to_datum(stx: &Value) -> Value {
    match stx {
        Value::Syntax(s) => unwrap_syntax_recursive(&s.datum),
        _ => stx.clone(),  // Already a datum
    }
}

fn unwrap_syntax_recursive(v: &Value) -> Value {
    match v {
        Value::Syntax(s) => unwrap_syntax_recursive(&s.datum),
        Value::Pair(p) => {
            let (car, cdr) = &*p.borrow();
            Value::Pair(Rc::new(RefCell::new((
                unwrap_syntax_recursive(car),
                unwrap_syntax_recursive(cdr),
            ))))
        }
        // ... vectors, etc.
        _ => v.clone(),
    }
}
```

### datum->syntax

Create syntax object with given context:

```scheme
(datum->syntax template-id datum) → syntax
```

**Semantics:** Creates syntax object from datum, copying lexical context from template-id.

```rust
fn datum_to_syntax(template_id: &Value, datum: &Value) -> Result<Value> {
    let context = match template_id {
        Value::Syntax(s) => s.scopes(),
        _ => return Err(Error::ExpectedIdentifier),
    };

    Ok(wrap_syntax_recursive(datum, &context))
}

fn wrap_syntax_recursive(datum: &Value, context: &ScopeSet) -> Value {
    Value::Syntax(Box::new(SyntaxObject {
        datum: match datum {
            Value::Pair(p) => {
                let (car, cdr) = &*p.borrow();
                Value::Pair(Rc::new(RefCell::new((
                    wrap_syntax_recursive(car, context),
                    wrap_syntax_recursive(cdr, context),
                ))))
            }
            // ... vectors, etc.
            _ => datum.clone(),
        },
        context: context.clone(),
        source: None,
    }))
}
```

### generate-temporaries

Create fresh identifiers:

```scheme
(generate-temporaries stx-list) → list of identifiers
```

```rust
fn generate_temporaries(stx_list: &Value) -> Result<Value> {
    let items = list_to_vec(stx_list)?;
    let mut result = Vec::new();

    for _ in &items {
        let name: Rc<str> = format!("g{}", fresh_id()).into();
        let scope = ScopeId::fresh();

        result.push(Value::Syntax(Box::new(SyntaxObject {
            datum: Value::Identifier(Box::new(IdentifierData {
                name,
                scopes: ScopeSet::new().with_scope(scope),
            })),
            context: ScopeSet::new(),
            source: None,
        })));
    }

    Ok(vec_to_list(result))
}
```

---

## Quasisyntax

Quasisyntax provides template-based syntax construction:

```scheme
#`(if #,test #,@body)   ; quasisyntax with unsyntax and unsyntax-splicing
```

### Reader Syntax

| Syntax | Expansion |
|--------|-----------|
| `#'datum` | `(syntax datum)` |
| `#`datum` | `(quasisyntax datum)` |
| `#,expr` | `(unsyntax expr)` |
| `#,@expr` | `(unsyntax-splicing expr)` |

### Implementation

Quasisyntax is similar to quasiquote but works with syntax objects:

```rust
fn expand_quasisyntax(template: &Value, env: &Rc<Environment>) -> Result<Value> {
    match template {
        // Unsyntax: evaluate and insert
        Value::Pair(p) if is_unsyntax(&p.borrow().0) => {
            let expr = &p.borrow().1;
            eval(car(expr)?, env)
        }

        // Unsyntax-splicing: evaluate and splice
        Value::Pair(p) if is_unsyntax_splicing(&p.borrow().0) => {
            // Similar but returns list to splice
        }

        // Pairs: recurse
        Value::Pair(p) => {
            let (car, cdr) = &*p.borrow();
            let new_car = expand_quasisyntax(car, env)?;
            let new_cdr = expand_quasisyntax(cdr, env)?;
            // Wrap in syntax
            Ok(Value::Syntax(Box::new(SyntaxObject {
                datum: Value::Pair(Rc::new(RefCell::new((new_car, new_cdr)))),
                context: get_current_scopes(env),
                source: None,
            })))
        }

        // Atoms: wrap in syntax
        _ => Ok(Value::Syntax(Box::new(SyntaxObject {
            datum: template.clone(),
            context: get_current_scopes(env),
            source: None,
        }))),
    }
}
```

---

## Implementation Phases

### Phase 1: Syntax Objects (Foundation)

**Goal:** Add syntax object type and basic operations

**Tasks:**
1. [ ] Add `SyntaxObject` struct to patina-runtime
2. [ ] Add `Value::Syntax` variant
3. [ ] Implement `syntax` special form
4. [ ] Implement `syntax->datum`
5. [ ] Implement `identifier?`
6. [ ] Add reader support for `#'`
7. [ ] Basic tests

**Estimated effort:** 2-3 days

**Files to modify:**
- `crates/patina-runtime/src/value/mod.rs`
- `crates/patina-runtime/src/syntax_object.rs` (new)
- `crates/patina-tree-walker/src/eval/special_forms/`
- `crates/patina-frontend/src/lexer/`
- `crates/patina-frontend/src/parser/`

### Phase 2: datum->syntax and Context

**Goal:** Enable context transfer for hygiene

**Tasks:**
1. [ ] Implement `datum->syntax`
2. [ ] Implement `bound-identifier=?`
3. [ ] Implement `free-identifier=?`
4. [ ] Implement `generate-temporaries`
5. [ ] Tests for hygiene utilities

**Estimated effort:** 2-3 days

**Dependencies:** Phase 1

### Phase 3: syntax-case

**Goal:** Core procedural macro form

**Tasks:**
1. [ ] Implement `syntax-case` special form
2. [ ] Adapt pattern matcher for syntax objects
3. [ ] Implement pattern variable binding
4. [ ] Implement fenders (guards)
5. [ ] Comprehensive tests

**Estimated effort:** 3-5 days

**Dependencies:** Phase 2

### Phase 4: Quasisyntax

**Goal:** Template-based syntax construction

**Tasks:**
1. [ ] Add reader support for `#``, `#,`, `#,@`
2. [ ] Implement `quasisyntax` expansion
3. [ ] Implement `unsyntax` and `unsyntax-splicing`
4. [ ] Integration tests

**Estimated effort:** 2-3 days

**Dependencies:** Phase 3

### Phase 5: syntax-rules as Macro

**Goal:** Implement syntax-rules using syntax-case

**Tasks:**
1. [ ] Write syntax-rules as syntax-case macro
2. [ ] Verify backward compatibility
3. [ ] Performance comparison
4. [ ] Deprecate old implementation (optional)

**Estimated effort:** 1-2 days

**Dependencies:** Phase 4

### Phase 6: Polish and Documentation

**Goal:** Production-ready implementation

**Tasks:**
1. [ ] Error messages with source locations
2. [ ] Debug output integration
3. [ ] Documentation updates
4. [ ] Performance optimization
5. [ ] Edge case testing

**Estimated effort:** 2-3 days

**Dependencies:** Phase 5

---

## Integration Strategy

### Backward Compatibility

The existing `syntax-rules` implementation will continue to work. After Phase 5, it can optionally be replaced by the syntax-case based implementation.

### Desugarer Integration

The desugarer already handles macros. For syntax-case:

```rust
fn desugar_application(&mut self, list: &Value, env: &Rc<Environment>) -> Result<CoreExpr> {
    let operator = car(list)?;

    // Check for syntax-case macro
    if let Some(Value::SyntaxCaseMacro(transformer)) = self.lookup_macro(&operator, env) {
        // Apply transformer (it's a procedure)
        let stx = self.to_syntax_object(list);
        let expanded = apply_transformer(transformer, stx, env)?;
        return self.desugar(&expanded, env);
    }

    // ... existing logic
}
```

### Scope Set Reuse

The existing `ScopeSet` and flip-scope mechanism work unchanged:

1. Before expansion: flip macro_scope on input syntax object
2. Transformer runs, may call `datum->syntax` to transfer context
3. After expansion: flip macro_scope on output syntax object

---

## Testing Strategy

### Unit Tests

```rust
#[test]
fn test_syntax_object_creation() {
    assert_eval_to("#'x", ...);
    assert_eval_to("(identifier? #'x)", "#t");
    assert_eval_to("(identifier? #'42)", "#f");
}

#[test]
fn test_bound_identifier() {
    assert_eval_to("(bound-identifier=? #'x #'x)", "#t");
    assert_eval_to("(bound-identifier=? #'x #'y)", "#f");
}

#[test]
fn test_syntax_case_basic() {
    assert_program_eval_to(r#"
        (define-syntax my-if
          (lambda (stx)
            (syntax-case stx ()
              ((_ test then else)
               #'(if test then else)))))
        (my-if #t 1 2)
    "#, "1");
}
```

### Integration Tests

```scheme
;; Test hygiene
(let ((if +))
  (my-if #t 1 2))  ; Should still use real if

;; Test fenders
(define-syntax id-only
  (lambda (stx)
    (syntax-case stx ()
      ((_ x) (identifier? #'x) #'x)
      ((_ x) (syntax-error "expected identifier")))))

;; Test generate-temporaries
(define-syntax swap!
  (lambda (stx)
    (syntax-case stx ()
      ((_ a b)
       (with-syntax ((tmp (car (generate-temporaries #'(tmp)))))
         #'(let ((tmp a))
             (set! a b)
             (set! b tmp)))))))
```

### Compatibility Tests

Run existing syntax-rules tests with the new syntax-case based implementation to ensure backward compatibility.

---

## References

### Specifications

- **R7RS Large Macro Fascicle** - https://r7rs.org/large/fascicles/macro/1/
  - Comprehensive syntax-case specification for R7RS-large (announced October 2024)
  - Includes explicit-renaming macros as alternative
  - New features: syntax parameters, identifier properties
  - Procedural interface to syntax object destructuring
  - Target completion: December 2025 (Scheme's 50th birthday)

- **R6RS Chapter 12** - Syntax-case
  - Original R6RS syntax-case specification
  - https://www.r6rs.org/

- **SRFI-93** - R6RS Syntax-Case Macros
  - https://srfi.schemers.org/srfi-93/srfi-93.html

- **SRFI-72** - Hygienic macros (reference)

### Papers

- **"Syntactic Abstraction in Scheme"** - Dybvig, Hieb, Bruggeman (1993)
  - Original syntax-case design paper

- **"Binding as Sets of Scopes"** - Flatt (2016)
  - Current hygiene approach (already implemented in Patina)
  - https://www.cs.utah.edu/plt/scope-sets/

### Reference Implementations

#### Portable syntax-case (psyntax) - **Primary Reference**

The canonical reference implementation, originally developed for Chez Scheme:

- **Main page**: https://www.scheme.com/syntax-case/
- **R6RS libraries version**: https://scheme.com/syntax-case/r6rs-libraries/index.html
- **Mirror**: https://conservatory.scheme.org/psyntax/r6rs-libraries/

Key characteristics:
- **Single-file implementation** (`psyntax.ss`) - easy to study
- Ported to 12+ Scheme implementations (Chez, Gambit, Gauche, Chicken, MIT, Larceny, etc.)
- `psyntax.pp` is pre-expanded version for bootstrapping
- Documented in "The Scheme Programming Language" Chapter 8

**Why study psyntax:**
- Shows the minimal core needed for syntax-case
- Portable design reveals essential algorithm without implementation-specific details
- Well-tested across many implementations

#### Chez Scheme - **Gold Standard**

The original and most mature syntax-case implementation:

- **User's Guide**: https://cisco.github.io/ChezScheme/csug9.5/intro.html
- Created by the syntax-case paper authors (Dybvig, Hieb, Bruggeman)
- Reference for edge cases and optimization strategies
- Source: https://github.com/cisco/ChezScheme

#### Racket - **Modern Scope-Set Implementation**

Uses the same "Binding as Sets of Scopes" approach as Patina:

- Expander source: `racket/src/expander/`
- Most relevant for Patina since we share the hygiene mechanism
- https://github.com/racket/racket

#### Chibi-scheme - **Alternative Approach**

Uses explicit renaming (ER) macros instead of syntax-case:

- **ER macro discussion**: https://github.com/ashinn/chibi-scheme/issues/114
- **datum->syntax PR**: https://github.com/ashinn/chibi-scheme/pull/496
- syntax-case can be implemented as a macro over ER transformers
- Simpler but less powerful than full syntax-case

ER macro example:
```scheme
(define-syntax unless
  (er-macro-transformer
    (lambda (expr rename compare)
      `(,(rename 'if) (,(rename 'not) ,(cadr expr))
        ,(caddr expr)))))
```

### Additional Resources

- **Macro systems in Scheme** - https://terbium.io/2020/05/macros-scheme/
  - Comprehensive overview of different macro systems

- **Explicit renaming tutorial** - https://wiki.call-cc.org/explicit-renaming-macros
  - CHICKEN Scheme wiki tutorial on ER macros

- **Scheme surveys - Syntax definitions** - https://docs.scheme.org/surveys/syntax-definitions/
  - Survey of macro support across Scheme implementations

### Recommended Study Order

1. **R7RS Large Macro Fascicle** - Target specification
2. **psyntax.ss** - Portable implementation to understand core algorithm
3. **Chez Scheme** - Edge cases and optimizations
4. **Racket expander** - Scope-set based implementation (closest to Patina)

### Patina Documentation

- `docs/MACRO_SYSTEM.md` - Current macro system architecture
- `internal/MACRO_SYSTEM_KNOWN_LIMITATIONS.md` - Known edge cases
