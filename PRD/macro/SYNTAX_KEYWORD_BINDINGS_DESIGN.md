# Binding-based syntactic keywords

**Status:** designed, not started (2026-08-16). Two staged PRs.

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

| # | Workaround | Location |
|---|---|---|
| 1 | `DESUGARED_FORMS` + `SYNTAX_RULES_KEYWORDS` + `is_core_syntax`, consulted at four sites across both backends | `patina-runtime/src/library_loader.rs` |
| 2 | `R5RS_SYNTAX` and its silent skip (`if let Some(tv) = base_lib.get_export_tagged(name)`) | `patina-primitives/src/primitives/eval.rs` |
| 3 | `(define else 'else)` / `(define => '=>)`, so `(import (only (scheme base) else))` resolves | `lib/scheme/base.sld` |

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

### 2.5 Value position: unified in stage 1, decided separately

`(list cond)` already evaluates to `(#<macro>)` on both backends. A marker loading as
`#<syntax:if>` is the same convention, costs nothing, and makes `if` and `cond` behave alike —
before this work `if` errored and `cond` did not, which was an accident of implementation rather
than a rule: `if` had no binding to load, so the reference failed as unbound.

**Unifying the two is stage 1's job; choosing which answer they share is not.** Stage 1 takes
Gauche's, because it is what Patina already does for macros and it costs nothing. A follow-up
changes both to Chez's and chibi's — a desugar-time "syntactic keyword used as an expression"
error — which is deliberately *not* in stage 1: it changes behaviour at every existing macro
reference, not just at the keywords this work is about, and it should be bisectable from them.
It is small in code (the two `CoreExprKind::Var` construction sites in the desugarer) and costs an
environment lookup per variable reference during desugaring.

**What R7RS actually says**, checked in `spec/r7rs-small-spec/` rather than recalled:

- §3.1 (`basic.tex`): an identifier names *either* a type of syntax *or* a location. Disjoint, so a
  keyword in expression position is not a variable reference at all.
- §1.3.3 (`struct.tex`), auxiliary syntax: "Any use as an independent syntactic construct or
  variable is an error."
- §1.3.2 (`struct.tex`) is the decisive part. "An error **is signaled**" means implementations
  *must* detect and report; a plain "**it is an error**" means they are "not required to detect or
  report the error, though they are encouraged to do so." The keyword-as-expression rule uses the
  plain form.

So both answers are conformant, and the spec encourages the stricter one without requiring it.
Chez 10.4.1 and chibi 0.12 raise (for macros as well as keywords); Gauche 0.9.15 and Patina do not.

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

### Stage 1.5 — syntax in value position  *(follow-up, see §2.5)*

Make a syntactic keyword *and a macro* an error where a value is expected, as Chez and chibi do.
Separate because it changes behaviour at every macro reference, not only at keywords.

### Stage 2 — delete the fallback

Removes the 17-arm `match`, and with it `DESUGARED_FORMS`, `SYNTAX_RULES_KEYWORDS` and
`is_core_syntax` (workaround 1) and the `R5RS_SYNTAX` leak (workaround 2).

**Narrower than planned, because stage 1 took more than expected.** What is left is one thing: the
*bare* spelling still resolves after an import set has excluded or moved it. `(except … begin)`
still admits `begin`, `(prefix … s:)` still admits `begin`, and `(null-environment 5)` still admits
`cond-expand` — all three because the name is unbound there and the fallback claims it.

The *other* half of `prefix` needed no stage 2 at all and is conformant as of stage 1: `s:begin`
resolves, because `prefix` copies the marker under the new name and the desugarer dispatches on the
form. `s:let`, `s:if` and `s:quote` compose correctly too, which is the real check — `s:quote`
carries the reader's `'` shorthand, so a prefixed base that got this wrong would fail on the first
quoted datum. Pinned in `crates/patina-tests/tests/core_syntax_bindings.rs`.

Requires:

- `lib/scheme/lazy.sld` to import a library exporting syntax. It imports only
  `(patina internal lazy)`, but its included `lazy/promises.scm` uses `define-syntax`,
  `syntax-rules` and `lambda`. It is the only one of 49 bundled `.sld` files affected — every other
  either imports `(scheme base)` or has no body.
- `import` and `expand` reachable from `global_env` (§2.4).
- `patina-frontend/tests/core_syntax_list.rs` retired or repointed: it pins the list against the
  desugarer, and after stage 2 there is no list. The property it protects — a name that claims to
  be syntax must actually be intercepted — should survive as a test over `CoreForm`.

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
- `cargo run -p patina-compat -- run` on stage 2 only. Baseline 143/184 pass. Stage 2 is expected
  to hold; a drop names the packages that were relying on Patina's leniency, which is information
  worth having either way.
