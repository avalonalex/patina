# Binding-based syntactic keywords

**Status:** stages 1, 1.5 and 2 landed as #86, #88 and #89 (2026-08-16). Stage 3 landed
2026-08-25 — the header said "not started" long after the stages below were marked done, so
take the per-stage marks as authoritative.

Give Patina's core syntactic keywords real bindings — a marker value in the environment — so
that export resolution, `only`, `except`, `prefix` and `rename` reach them through the ordinary
path, with no name list and no special case. Retires the three workarounds now in the tree and
fixes six conformance defects, one of them a recorded backend divergence.

Companion to `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md` §6, which surfaced the divergence, and to
`PRD/ARCHIVE/TRACK_L_FIXED_DEFECTS.md`, whose entry "A library could not re-export a core
syntactic keyword" costed this work and left the note this document expands.

## 1. The defect

Core syntax is recognized **by spelling, in every scope, unconditionally**. The desugarer matches
the head symbol against a 17-arm `match` (`patina-frontend/src/desugarer/mod.rs`,
`desugar_list_tagged`) after failing to find a macro; the keywords themselves have no entry in any
environment. Everything below follows from that one fact.

R7RS §5.6.1 lets a library export any identifier it imports, and §5.6.1's import sets bound what a
program can name. Neither can work for an identifier that has nothing to bind.

### Symptoms

All verified 2026-08-16 against `target/release/patina` at 9c9f2db, both backends unless noted,
cross-checked against chibi 0.12 and Gauche 0.9.15 (`gosh -r7`).

**A library cannot rename core syntax at import.** The recorded divergence, Track L §6.

```scheme
;; inside a library: (import (rename (scheme base) (begin blk)))
;; VM          => loads; (blk 1 2) then fails "unbound variable: blk"
;; tree-walker => Parse error: Identifier 'begin' not found for rename
;; chibi, Gauche => 2
```

Note where the references land: both make the rename *work*, so neither of our answers is right
and "make the backends agree by rejecting" would pick the wrong one.

**A top-level `define` cannot shadow core syntax — but `define-syntax` can.**

```scheme
(define (if a b c) (list 'proc a b c))
(if 1 2 3)     ;; Patina => 2        chibi, Gauche => (proc 1 2 3)

(define-syntax if (syntax-rules () ((_ a b c) (list 'mymacro a b c))))
(if 1 2 3)     ;; Patina, chibi, Gauche => (mymacro 1 2 3)
```

The two halves of redefinition disagree with each other, and the reason is visible in the source:
the macro lookup at `desugar_list_tagged` runs *before* the name `match`, so a macro binding wins
and a variable binding does not.

**`except` does not except.**

```scheme
(import (except (scheme base) begin))
(begin 1 2)    ;; Patina => 2        chibi, Gauche => undefined variable: begin
```

**`prefix` gets it backwards.**

```scheme
(import (prefix (scheme base) s:))
(s:begin 1 2)  ;; Patina => unbound  chibi, Gauche => 2
(begin 1 2)    ;; Patina => 2        chibi, Gauche => undefined variable: begin
```

Both references also prefix `quote`, so `'x` stops working under a prefixed base. That is
conformant, and it is the clearest measure of how much this changes: an import set really does
scope the reader's shorthand.

**`(null-environment 5)` is not null.**

```scheme
(eval '(cond-expand (else 42)) (null-environment 5))
;; Patina => 42        chibi => error
```

`cond-expand` is R7RS and has no business in an R5RS null environment. Every name in
`DESUGARED_FORMS` but not in `R5RS_SYNTAX` leaks the same way: `import`, `include`, `include-ci`,
`syntax-error`, `expand`.

**The `else` workaround is observable.**

```scheme
(list else)
;; Patina => (else)                  ; a plain symbol — the workaround leaking
;; chibi  => error: invalid use of auxiliary syntax: else
;; Gauche => (#<syntax null#else>)
```

### The three workarounds

Each is the same hole patched at a different surface.

All three are gone as of stage 2.

| # | Workaround | Location | Retired |
|---|---|---|---|
| 1 | `DESUGARED_FORMS` + `SYNTAX_RULES_KEYWORDS` + `is_core_syntax`, consulted at four sites across both backends | `patina-runtime/src/library_loader.rs` | stage 2 |
| 2 | `R5RS_SYNTAX` and its silent skip (`if let Some(tv) = base_lib.get_export_tagged(name)`) | `patina-primitives/src/primitives/eval.rs` | stage 1 (skip), stage 2 (leak) |
| 3 | `(define else 'else)` / `(define => '=>)`, so `(import (only (scheme base) else))` resolves | `lib/scheme/base.sld` | stage 1 |

Workaround 1 is the newest, added by PR #81 (2026-08-16) to make `(r6rs base)` load. Its own
closing note said it should be the last such list.

## 2. Design

### 2.1 The marker

A new leaf variant on the existing heap-object enum:

```rust
HeapObjectData::CoreSyntax(CoreForm)
```

`CoreForm` is a `Copy` C-like enum of 23 variants in two groups, and the split is the one
`library_loader.rs` already draws by hand — the redesign gives it a reason:

- **16 dispatching forms** — `quote quasiquote lambda if set! define define-syntax let-syntax
  letrec-syntax begin import cond-expand include include-ci syntax-error expand`. In head position
  each selects a `desugar_*` method.
- **7 auxiliary keywords** — `unquote unquote-splicing syntax-rules _ ... else =>`. These have
  meaning only inside another form. In head position each is an error
  (chibi's "invalid use of auxiliary syntax"), which is strictly better than today, where `(else 1)`
  reports that a symbol is not a procedure.

`apply` stays out, as it does today: the desugarer special-cases it, but it is also a real
procedure binding, so it resolves the ordinary way.

The 23 markers are interned once per heap and held in a `Heap` field. Keeping them alive has an
exact precedent three lines long: `GcVisitor::new` (`patina-core/src/heap/gc.rs`) already marks the
symbol intern table as roots, mark-only, because entries are leaves by construction. The same
argument and the same loop apply here — a dangling intern-table index would break any collector, so
this is a heap invariant rather than collector policy.

**Rejected: a `TAG_SPECIAL` immediate.** All eight primary tags are taken, and the special payload
space is read by `is_special` consumers that do not expect new inhabitants. The saving would be a
heap indirection on a desugar-time lookup that already happens.

### 2.2 Dispatch

`desugar_list_tagged` already fetches `env.get(sym)` for every head symbol, to test for a macro.
The marker arm goes there, beside the macro arm, and dispatches on `CoreForm` rather than on the
spelling. A rename then needs no further work: `(blk 1 2)` finds `CoreForm::Begin` and calls
`desugar_begin_tagged`.

No new lookups on any path. The 17-arm string `match` becomes the fallback in stage 1 and is
deleted in stage 2.

Lexical shadowing keeps its present mechanism: local bindings are not in the desugarer's
environment, so `shadowed_names` / `is_shadowed` must guard the marker arm exactly as it already
guards the macro arm and the name `match`.

### 2.3 The import machinery needs no changes

This is the load-bearing finding, and it is why the work is smaller than it looks. All five import
combinators — `Library`, `Only`, `Except`, `Prefix`, `Rename` — already work by copying name→value
pairs out of a temporary environment (`process_import_set` in `patina-tree-walker/src/eval/mod.rs`,
and its two near-copies in `patina-vm/src/backend.rs` and `patina-vm/src/runtime/vm_state.rs`).
`build_library` (`patina-runtime/src/library_loader.rs`) resolves exports the same way.

The import system is *already* binding-based. Core syntax is simply not in it. Once the markers are
ordinary bindings they ride through untouched, and the `is_core_syntax` exemptions inside `Only`
and `build_library` — plus the explicit `Cannot rename core syntax '…' on export` refusal — are
deleted rather than rewritten.

### 2.4 Bootstrap

A Rust-built `(patina internal syntax)` on the existing builder pattern
(`patina-runtime/src/rust_library_loader.rs`, `stdlib/internal_*.rs`) defines the 23 markers and
exports their names. `lib/scheme/base.sld` imports it and re-exports the R7RS-appropriate subset,
replacing the `(define else 'else)` block.

There is no chicken-and-egg. `load_bootstrap` (`patina-vm/src/backend.rs`, and the tree-walker's
equivalent) calls `load_library` from Rust and copies the resulting exports into `global_env`
directly; it never evaluates a Scheme `(import …)` form.

Two names need a decision because R7RS does not put them in `(scheme base)`: `import`, which is a
top-level and library declaration keyword rather than an export, and `expand`, a Patina extension.
Both must reach `global_env` for a bare script to work; re-exporting them from `(scheme base)` is
the least machinery, at the cost of a non-standard entry in that library's export list.

### 2.5 Value position: unified in stage 1, made an error in stage 1.5  *(done)*

`(list cond)` already evaluates to `(#<macro>)` on both backends. A marker loading as
`#<syntax:if>` is the same convention, costs nothing, and makes `if` and `cond` behave alike —
before this work `if` errored and `cond` did not, which was an accident of implementation rather
than a rule: `if` had no binding to load, so the reference failed as unbound.

**Unifying the two was stage 1's job; choosing which answer they share was not.** Stage 1 took
Gauche's, because it was what Patina already did for macros and it cost nothing. Stage 1.5 changed
both to Chez's and chibi's — `reject_syntax_as_value` at the two `CoreExprKind::Var` construction
sites — kept separate because it changes behaviour at every existing macro reference, not only at
the keywords this design is about.

The predicted cost did not materialise. An environment lookup per variable reference during
desugaring is one or two `FxHashMap` probes (the desugarer's environment is the definition
environment, one or two levels deep, never the local frame), and an interleaved A/B over a full
stdlib bootstrap — the most desugar-heavy thing Patina does — could not separate the two binaries:
~10 ms either way.

**What R7RS actually says**, checked in `spec/r7rs-small-spec/` rather than recalled:

- §3.1 (`basic.tex`): an identifier names *either* a type of syntax *or* a location. Disjoint, so a
  keyword in expression position is not a variable reference at all.
- §1.3.3 (`struct.tex`), auxiliary syntax: "Any use as an independent syntactic construct or
  variable is an error."
- §1.3.2 (`struct.tex`) is the decisive part. "An error **is signaled**" means implementations
  *must* detect and report; a plain "**it is an error**" means they are "not required to detect or
  report the error, though they are encouraged to do so." The keyword-as-expression rule uses the
  plain form.

So both answers are conformant, and the report encourages the stricter one without requiring it.
Chez 10.4.1 and chibi 0.12 raise, for macros as well as keywords; Gauche 0.9.15 does not. Patina
now raises.

**This is therefore a choice, not a conformance fix, and the write-up should keep saying so.** A
scheme-reports thread on syntax objects records that an implementation may "accept it and
initialize the variable with some object whose properties are not specified by R7RS" — which
blesses the behaviour stage 1 had. Searching turned up no other substantive discussion: the R7RS
errata list has nothing on it across all 33 entries, and neither does the working group's issue
tracker. `crates/patina-tests/tests/syntax_as_a_value.rs` carries the reasoning so that reversing
it later is a decision rather than a discovery.

**`set!` is refused too, and that asymmetry is the report's.** §5.3.1 is normative that a
*definition* over a syntactic keyword binds a new location; nothing licenses `set!`, whose
⟨variable⟩ must already be one. chibi draws the line in exactly that place, rejecting `(set! if 5)`
with the message it gives for `(list if)`; Gauche accepts it and then breaks inside its own startup
code. Without this, reading syntax was an error while overwriting it silently succeeded.

**Two residuals, recorded rather than implied.** The check asks what a name resolves to *while the
form is being desugared*, so it misses a name that is not syntax yet (a forward reference to a
later `define-syntax`) and one whose spelling is shadowed by an unrelated binding — both still load
`#<macro>`. Both fail safe, never producing a wrong rejection, and both are pinned at the bottom of
`syntax_as_a_value.rs`. Closing them means checking at every variable *read* instead: `LoadGlobal`
on the VM, whose per-site inline cache stores a slot, so a fill-time check would be unsound and a
per-load tag test would sit on the interpreter's hottest instruction. That price is the reason, and
it is written down so the `#<macro>` formatting in `datum_writer.rs` is not mistaken for dead code.

**One lookup, three askers.** Head position, value position and the `set!` target had grown three
resolutions of the same question with two different shadowing rules and two different environment
queries — the head path dropped the reference's scopes, the value path used them. They are now one
`Desugarer::resolve_syntax`. Flattening them surfaced the rule that had been implicit: a
*macro-introduced* reference is exempt from the spelling-keyed shadow test, because hygiene means
`(let ((if 'captured)) (my-cond #t 'ok))` must still see the template's `if` as the special form.
The hygiene suite caught it immediately, which is the argument for having merged them.

Nothing in `lib/` or the corpus reads `else` as a value; the only two hits are a `syntax-rules`
literals list and a `cond-expand` clause, both structural.

**R7RS is not neutral about §1's second symptom, however.** §5.3.1 (`prog.tex`) is normative and
exact: "if ⟨variable⟩ is not bound, *or is a syntactic keyword*, then the definition will bind
⟨variable⟩ to a new location before performing the assignment." A top-level `define` over a keyword
is specified, not merely conventional, and Patina contradicted it.

### 2.6 What stays name-based, on purpose

`unquote`, `unquote-splicing` and nested `quasiquote` **inside a template** stay recognized by name
— three sites: the desugarer's depth tracking, `patina-vm/src/compiler/quasiquote_expand.rs`, and
`patina-tree-walker/src/eval/cps_eval/quasiquote.rs`. So does `else` inside `cond-expand`. R7RS
specifies these structurally, as parts of an enclosing form rather than as references that a use
site resolves. Making them binding-sensitive would be a different and probably wrong change.

The consequence is a knowingly incomplete corner: under `(prefix (scheme base) s:)` a template's
`,x` keeps working where chibi and Gauche would want `s:unquote`. Nobody has written that program.

## 3. A correction to the earlier costing

The archive entry says the real work is that library bodies desugar in a parentless environment at
three sites, "which would have to chain to a root env seeded with the markers."

No chaining is needed. All three paths resolve imports **into `lib_env` before desugaring the
body** — `evaluate_parsed_library` in `patina-tree-walker/src/eval/mod.rs`, and its counterparts in
`patina-vm/src/backend.rs` and `patina-vm/src/runtime/vm_state.rs`. A library that imports
`(scheme base)` has the markers by the time its first form is desugared, and the parentlessness is
*correct*: a library sees only what it imports.

Seeding a root environment would in fact defeat the point. It would restore "syntax is everywhere"
and lose the `except`, `prefix` and `null-environment` fixes, leaving only the export-side ones.

The rest of that entry's reconnaissance holds and was re-checked: the desugarer already looks the
head symbol up before its name `match`, `Desugarer::new()` (no environment) has no non-test callers
— every construction site outside tests passes one — and `HeapObjectData::Macro` is the right shape
to copy.

## 4. Staging

Two PRs. The split is not ceremony: stage 1 cannot break a working program, stage 2 can, and they
should be bisectable apart.

### Stage 1 — a binding wins where there is one  *(done)*

Rule: if the head symbol resolves to a marker, dispatch on the marker; if it resolves to anything
else, it is that thing; **if it is unbound, fall back to the spelling as today**.

Fixes the import rename, rename-on-export, the `define`/`define-syntax` inconsistency, and
auxiliary keywords in head position. Retires workaround 3 outright and the *skip* half of
workaround 2 (`get_export_tagged` starts finding the keywords, so the `if let` becomes a plain
lookup).

Cannot break a working program: anything unbound behaves exactly as it does now.

The one behaviour change is `(list else)` → `#<syntax:else>` instead of `(else)`, which is Gauche's
answer and which nothing depends on.

**One thing the plan did not foresee, and it was flagged as an argument rather than a test.** §6
said making `_` and `...` bindings "should be inert, because the matcher compares name + scopes and
never consults an environment." The matcher does not — but `resolve_literal_bindings`, which runs
at macro *compile* time, does, and a literal that resolves is treated as bound. That broke
`test_srfi46_ellipsis_as_literal_with_custom_ellipsis`, exactly the case §6 named.

The cause was a pre-existing defect with nothing to do with keywords. `resolve_literal_bindings`
recorded the *definition site's* scopes for a literal found in the plain, unscoped bindings, so a
macro defined inside any scope could not match a global literal:

```scheme
(let ()
  (define-syntax m (syntax-rules (car) ((_ car) 'matched) ((_ x) 'not-matched)))
  (m car))
;; chibi, Gauche => matched      Patina => not-matched
```

`car` is an ordinary procedure; nothing about this is syntactic. Making `...` a binding merely gave
the macro a literal that resolved for the first time. Fixed by recording no scopes for an unscoped
binding, which needed a new `Environment::has_scoped_binding` — `get_with_scopes` cannot answer the
question, because it falls back to the plain bindings and returns a value either way, so asking it
labelled every global binding as scoped.

### Stage 1.5 — syntax in value position  *(done)*

A syntactic keyword *and a macro* are an error where a value is expected, as Chez and chibi do.
Separate from stage 1 because it changes behaviour at every macro reference, not only at keywords.

Retired two stage-1 assertions that had pinned the interim answer: `(list else)` returning
`#<syntax:else>`, now refused, and `(eqv? begin blk)` proving a renamed keyword is the same
interned marker — a property that is unchanged but no longer observable from Scheme, so it moved to
`core_syntax_is_interned_per_heap` in `patina-core`'s heap tests.

### Stage 2 — delete the fallback  *(done)*

Removes the 17-arm `match`, and with it `DESUGARED_FORMS`, `SYNTAX_RULES_KEYWORDS` and
`is_core_syntax` (workaround 1) and the `R5RS_SYNTAX` leak (workaround 2). A keyword is now
recognized through a binding and by nothing else, which is the whole point of the design.

The desugarer's environment stopped being optional along the way. With no fallback, a desugarer
with no environment cannot desugar `(quote x)` — it would compile a call to an unbound `quote` —
so `Desugarer::new()` now seeds one holding exactly the keywords: the desugarer's
`(null-environment)`. That removed the last `Option<Rc<Environment>>` branches, including the two
"define-syntax requires environment" errors, which could no longer happen.

**One entry above was misattributed, and the fix is what showed it.** The `except` and `prefix`
symptoms in §1 were measured at the *top level*, and deleting the fallback did not change them:

```scheme
(import (except (scheme base) begin))  (begin 1 2)   ;; => 2, still
(import (except (scheme base) car))    (car '(1 2))  ;; => 1, and this is the tell
```

An ordinary procedure survives `except` exactly as a keyword does, so this was never a keyword
defect. Its cause is `load_bootstrap` seeding every `(scheme base)` export into the global
environment before a program runs — the affordance that lets a script with no `import` work at all,
which Patina supports and chibi does not. Removing it is a separate decision about what Patina's
top level is, not part of this design.

Where an import set actually governs a scope, stage 2 does what it promised. Inside a library:

```scheme
(define-library (syn ex)
  (import (except (scheme base) begin))
  (export go)
  (begin (define (go) (begin 1 2))))
;; => unbound variable: begin        chibi, Gauche => the same
```

and `(null-environment 5)` — which builds a fresh environment rather than inheriting the global one
— is null: `(eval '(cond-expand (else 42)) (null-environment 5))` now errors, as chibi does.

**The export exemptions went with the fallback**, in `build_library` and in the three copies of
`(only …)`. A library that imports a keyword exports it like any other binding; one that does not
cannot export it, and saying so is right rather than lenient — the importer would have got nothing
either way, and only the spelling fallback hid that.

What it took:

- `lib/scheme/lazy.sld`, exactly as predicted — the one bundled `.sld` with a body and no
  syntax-providing import.
- 16 test fixtures with the same shape (14 inline in `sld_file_loading.rs`, 2 on disk under
  `resources/test-libraries/`). All were `.sld` files whose bodies used `define` while importing
  nothing; each now imports `(scheme base)`, which is what a real library must do.
- Two tests whose premise the change removes: `test_eval_empty_environment_if` asserted that
  "special forms should work even in empty environment", and two desugarer unit tests built a bare
  `Environment::new()` and expected `define-syntax` to be intercepted. chibi and Gauche both report
  `undefined variable: if` for the first; it is now asserted as an error.
- `core_syntax_list.rs` repointed a second time. It now pins the classification *and*
  `keywords_are_not_recognized_without_a_binding`, the direct regression guard: with an empty
  environment every keyword must compile to a call of itself.

`import` and `expand` needed no work here — the stage-1 cleanup pass had already made
`seed_top_level_syntax` real after review caught the doc claiming a seeding that did not exist.

### Stage 3 — the other half of the same rule: a *local variable* is a binding too  *(done, 2026-08-25)*

Stages 1 and 2 gave keywords real bindings, but left the other side of the comparison as it was:
a local variable was a `HashSet<Rc<str>>` of **spellings** (`shadowed_names`) that vetoed syntax
resolution outright. Nothing about that set was ordered, and it was consulted only for a
reference written *without* scopes, so an outer variable beat an inner `let-syntax` keyword, and
a macro-introduced reference skipped it entirely — which is why a template naming a
definition-site local spelled `if` was reported as a keyword.

Locals are now scoped bindings in the desugarer's environment, carrying a marker value that is
neither syntax nor a macro, and a reference resolves in the scopes it stands in. Ordering then
comes from the existing set-of-scopes rule — the binding with the largest scope set the
reference contains — with no separate shadowing rule to keep in step. Closes Larceny triage
families 14, 15 and 23, which together gated the `base` suite.

Three things fell out of it that were not obvious from the design:

- **The default ellipsis is decided per token, by binding** (R7RS 4.3.2), not per macro. A token
  that is an *identifier* came from an expansion and is read against its own scopes; a plain
  symbol was written here and is read against the macro's definition scopes. A declared SRFI 46
  ellipsis is exempt — it is a declaration, not a reference.
- **A `let-syntax` keyword is bound both scoped and unscoped.** Scoped so it can outrank an
  enclosing variable; unscoped because a reference the expander introduced carries none of this
  body's scopes and no scoped binding could match it.
- **Free-identifier relinking had to become scope-aware.** A template's free identifiers are
  linked back to the definition environment by *name*, and the name-only view of an environment
  hides local variables — so it could not tell `(let ((f …)) …)` around a macro from a global
  `f`, and aliased to the global. It asks with the macro's definition scopes now.

`shadowed_names` survives, feeding one remaining consumer: the *literal* matcher still compares
spellings (`is_literal_shadowed_tagged`). Deciding literal membership by binding is the last
piece, and is recorded in the triage doc's "not ours" section rather than attempted here.

## 5. Migration cost, measured

- **Bundled:** 1 file (`lib/scheme/lazy.sld`), as above.
- **Corpus:** 4 of 256 `.sld` files under `compat/vendor/` have a body and do not import
  `(scheme base)`. Three import `(chibi)`, which Patina does not bundle, so they already score
  `missing-library`; the fourth is `srfi-78`, which imports `(srfi 23)` and `(srfi 42)`.

The reason it is this cheap is structural, not luck: the corpus was written for chibi, and chibi is
strict. Any package that loads there already imports its syntax.

## 6. Risks

**Stage 2 changes what a working program means.** Widening a *binding* rule can only ever narrow
what resolves, which is the kind of change that can break code that runs today — the same
reasoning that has kept the lexer delimiter set open in Track L §6. The mitigation is the
measurement above plus a compat re-run, not confidence.

**~~`_` and `...` acquire bindings.~~** ✅ **Materialized in stage 1, and the argument was wrong.**
It said markers "should be inert, because the matcher compares `IdentifierKey` (name + scopes) and
never consults an environment," and closed by saying the SRFI-46 tests were the check and had to be
run before the claim was repeated. The tests were run, the claim was wrong, and the reason it was
wrong is instructive: the *matcher* does not consult an environment, but
`resolve_literal_bindings` at macro compile time does, and it was never in view. The write-up is
under Stage 1 above.

Worth keeping as a pattern rather than as a closed risk: an argument that names one mechanism
("the matcher") and concludes about a behaviour ("literal matching") has not covered the behaviour.

**Three near-copies of `process_import_set`.** The export loop was deduplicated into `build_library`
by PR #81 precisely because three byte-identical copies drift. The import side still has its three.
This work does not require merging them, but it does touch all three, and doing it here is the
cheapest it will ever be.

## 7. Verification

- `./scripts/run_chibi_tests.sh` — 1226/1226, the gate for both stages.
- `cargo test --all --lib --tests`; `cargo clippy --all-targets --all-features -- -D warnings`.
- New guard tests for each symptom in §1, on both backends. The import-rename case moves out of
  Track L §6 Open and becomes a passing test rather than a pinned divergence — it was never pinned,
  deliberately, because the correct answer was a design decision and this document is that decision.
- `cargo run -p patina-compat -- run` on stage 2 only. Baseline 143/184 pass. **Held exactly** —
  `compat/reports/results.scm` came back byte-identical, so not one package was relying on the
  leniency. The §5 reasoning is why: the corpus was written for chibi, and chibi is strict, so
  anything that loads there already imports its syntax.

**Both stages are done.** What the design set out to fix is fixed; what remains is recorded
elsewhere rather than here — stage 1.5 (§2.5, syntax in value position), `apply`'s desugarer
special case (§2.2 and the note in `desugar_list_tagged`, which is now the only head symbol
recognized by spelling), and the top-level import-set question stage 2 turned up (§4).
