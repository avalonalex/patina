# Track L — fixed defects, in full

Moved out of `PRD/TRACK_L_SNOW_LIBRARIES_PRD.md` §6 on 2026-08-15, when that document reached 1100
lines and the fixed narratives were two thirds of its largest section. Nothing is dropped: §6 keeps
a one-line index of every entry, and each one is here in full.

These are worth keeping rather than deleting because several record a *wrong* first diagnosis
alongside the right one. The recurring lesson across the track is that a recorded premise is worth
re-running before designing against it — three separate entries below were filed with a cause that
turned out not to be the cause, and one test was written that passed against unfixed code.


**A generated macro's template collapsed identifiers from different expansions** — ✅ **fixed**
(2026-08-20). Both backends. The `match-letrec` defect, finally run to ground — and the fix is one
line in the template compiler.

```scheme
(match-letrec ((x 1) (y 2)) (list x y))        ;; was: Duplicate parameter 'p-ls' in lambda
(match-letrec (((x y) (list 1 2))) (list x y)) ;; was: match failure, reported as a type error
;; chibi, Gauche, and now Patina => (1 2)
```

`match-extract-vars` pairs each pattern variable with a template-introduced temporary spelled
`p-ls`; hygiene must keep each expansion's copy distinct, and their identity lives only in their
expansion scopes — `p-ls{S138}` vs `p-ls{S142}`. Four synthetic repro attempts (recorded in the
previous entry) all preserved the distinction, because they routed the temporaries through
*pattern-variable substitution*, which takes the scope-preserving `mark_substituted` path. What the
real chain does differently: `match-identifier=?` embeds the continuations holding the temporaries
into the **rules of a generated macro** (the Petrofsky `eq` trick), so at that macro's compilation
the temporaries are template *text*, not substitutions. `compile_template`'s symbol case stamped
every non-pattern-variable identifier with the macro's one `definition_scopes` set — **replacing**
the scopes it carried — so both temporaries came out `p-ls{S139, S143, S149}`: indistinguishable.
Two bindings then met as duplicate lambda parameters; a list pattern became the equality constraint
`(p-ls p-ls)`, which `(1 2)` fails.

The fix: union the identifier's own scopes with the definition scopes instead of replacing
(`compile_template`, patina-macros). Distinctness rides along; everything definition-scope
resolution looks for is still present. `(chibi match)`'s suite goes **74 → 75 of 75** on both
backends and the package's corpus row flips to pass (125 → 126).

Two method notes worth keeping. The diagnosis that four synthetic repros could not deliver took
one instrumented run: `(macro-debug-mode 'on)` prints scope-set annotations per expansion, and
grepping the log for `p-ls{` showed the two introductions (S138, S142) and then a third scope
family containing neither — the collapse, timestamped to the exact expansion. Start there next
time. And the previous entry's closing guess — "probably wants the vector-hygiene/relinking fix
first" — was wrong: both of those §6 entries still reproduce after this fix, which is independent
of them. Predictions about unexplored defects belong in the entry as questions, not as sequencing
advice.

Guard: `hygiene.rs::test_generated_template_capture_keeps_expansions_distinct`, a distilled
vendor-free shape verified to fail pre-fix ("Duplicate parameter 'tmp'") and to agree with chibi
and Gauche post-fix.

**`read-line` rejected chibi's max-chars argument; textual reads rejected binary ports** — ✅
**fixed** (2026-08-19). Both backends. Found by running chibi-mime's suite, the first of the
wrong-result rows: 5 of 9, all four failures raising through the same two gaps.

```scheme
(read-line port 4096)                          ;; was: arity error — chibi honors the limit,
                                               ;; Gauche accepts and ignores the argument
(read-line (open-input-bytevector bv))         ;; was: "read-char: not a textual port" —
                                               ;; both references allow textual reads there
```

R7RS makes both "an error" — implementation freedom — and both references take it; chibi-mime
leans on each (`mime-line-length-limit` on every header read, and header parsing on binary message
ports before switching to `read-u8` for bodies). The max-chars semantics match chibi exactly —
at most n chars, newline still terminates and is consumed, the remainder stays in the port — and
binary-port textual reads decode UTF-8 in place (`decode_utf8_at` in `port.rs`), invalid bytes
erroring as on the file path. Widening only: no previously-working program changes.

Result: chibi-mime 5/9 → **9/9**. Guards: `binary_port_textual_reads.rs`, six tests on both
backends.

**`list-sort` reversed ties** — ✅ **fixed** (2026-08-19). Both backends. Found by chibi-voting's
suite: `sort-pairs` returned the right counts in the wrong tie order.

```scheme
(list-sort (lambda (x y) (> (cdr x) (cdr y))) '((a . 1) (b . 2) (c . 1) (d . 2) (e . 1)))
;; chibi, Gauche => ((b . 2) (d . 2) (a . 1) (c . 1) (e . 1))
;; Patina was    => ((d . 2) (b . 2) (e . 1) (c . 1) (a . 1))
```

The input to the failing sort was fully deterministic (candidate vectors, not hash order), which
pointed at `list-sort` itself: Shivers' reference implements it with `vector-heap-sort!`, and heap
sort reverses ties. SRFI 132 permits that — stability is `list-stable-sort`'s contract — but
**neither reference actually ships the reference's list-sort**: chibi delegates to its stable
native sort, Gauche likewise. chibi-voting, written against chibi, breaks residual ties by input
order. Fixed by aliasing `list-sort` to `list-merge-sort`, the file's own stable sort — a marked
`PATINA LOCAL EDIT` in `lib/srfi/132/sort.scm`, recorded in `132.sld`'s header, both pins updated.
The 221-assertion upstream SRFI 132 suite is unaffected (it never asserts instability).

Result: chibi-voting 5/7 → 6/7 — level with Gauche; the residual failure is upstream's
order-sensitive `instant-runoff-rank` expectation (§6 "Upstream, not ours"). Guards:
`srfi_132_list_sort_stability.rs`.

**`case` rejected a clause with an empty body** — ✅ **fixed** (2026-08-19). Both backends.
Found by auditing the corpus's "No matching pattern for macro case" rows; chibi-tar's is a
metadata clause with datums and no expressions:

```scheme
(case n ((1)) (else 'other))   ;; chibi-tar's shape: ((#\g #\x)) — skip tar metadata blocks
;; before => No matching pattern for macro case
;; chibi  => unspecified on match     Gauche => unspecified on match
```

R7RS's grammar wants `((datum …) <sequence>)` with at least one expression, but **both references
accept the empty body** with an unspecified result — the both-agree bar this track uses — and the
same goes for empty `(else)`. Fixed with three new `syntax-rules` arms in
`lib/scheme/base/conditionals.scm` (single/multi-clause datum forms and `(else)`), all yielding
`(if #f #f)` on match and continuing dispatch on miss. Guard:
`compliance/derived.rs::test_case_empty_body_clause`.

Chibi-tar advanced from `parse-error` to `unbound-identifier: read-padded-string` — a reference
that (chibi binary-record)'s generated accessors make into their *importer's* environment. The
one- and two-level definition-environment repros both pass on Patina, so the failing shape is
something deeper in that package's `define-binary-type`/auxiliary-syntax chain; undiagnosed, and
recorded as such rather than guessed at.

The sibling shape found in the same audit — chibi-app's `else` *followed by more clauses* — stays
rejected on purpose: Gauche rejects it too, and on chibi the trailing clause is silently dead
(§6's "Upstream, not ours"). What that row is owed is a location in the error message, not
acceptance.

**SRFI 14 `ucs-range->char-set` discarded its base set** — ✅ **fixed** (2026-08-19). Both
backends. Found by `(srfi 14 test)` on its first run, the day the suite was restored to
`scheme_tests/upstream/` — it had never run against Patina before.

```scheme
(ucs-range->char-set 97 103 #t (string->char-set "12345"))
;; before => chars a-f only        SRFI 14, chibi => a-f plus 1-5
```

`%default-base` takes the maybe-base *rest list* — `pair?` is how it tells "given" from
"defaulted", and a non-pair falls through to an empty 256-slot string. Four of its five callers
pass the rest list; `ucs-range->char-set`, the one procedure whose optional handling was rewritten
(it has `error?` in front of the base), extracted the char-set first and passed *it* — a record,
not a pair — so the base silently defaulted to empty. The `!` variant takes its base positionally
and mutates it, so it was unaffected, and the suite's neighbouring `test-not` passed for the wrong
reason (the broken result was unequal to the decoy too).

Fix: pass `(cdr rest)`, restoring the convention (`lib/srfi/14.scm`, comment at the site). Guard:
`upstream_srfi_suites.rs::srfi_14_char_set`, 72 assertions. Worth keeping for the pattern: the
defect sat precisely in the one spot where a port departed from the reference's own shape — the
same lesson as SRFI 132's three defects, all in hand-written replacements.

**`(scheme r5rs)` did not export the R5RS syntax keywords** — ✅ **fixed** (2026-08-19). Both
backends. Found when the `(srfi 23)` shim let srfi-78's reference implementation load far enough
to fail for its real reason.

```scheme
(define-library (t r5rs-only)
  (export f)
  (import (scheme r5rs))
  (begin (define (f) 42)))
;; before => Parse error in ...: runtime error: unbound variable: `define`
;; chibi => fine
```

`lib/scheme/r5rs.sld` carried a NOTE saying `define`, `lambda`, `if` and the rest "are core syntax
handled by the desugarer and cannot be re-exported as bindings … implicitly available in every
environment". Both halves had expired: the sibling entry below ("A library could not re-export a
core syntactic keyword") made such exports *legal* on 2026-08-16, and once syntax keywords became
real bindings, a library body genuinely has only what it imports — so a library importing nothing
but `(scheme r5rs)` had no `define` at all. Top level never showed it, because core syntax is
seeded there. srfi-78's `78.sld` — `(import (srfi 23) (srfi 42) (scheme r5rs))`, then an include
full of defines — is exactly this shape in the wild.

The fix is inventory, not mechanism: the keyword set R5RS defines, taken from chibi's
`(scheme r5rs)`, added to the export list. Guard test:
`scheme_r5rs.rs::test_r5rs_provides_core_syntax_inside_a_library_body`. Worth keeping for the
pattern: a comment recording *why a list is short* is a premise, and when the premise expires
nothing fails — the list just stays silently wrong until a corpus package walks into the gap.

**The lexer rejected a non-ASCII identifier** — ✅ **fixed** (2026-08-16). Both backends. Found once
srfi-197's misfiled `missing-library` row was corrected and the package could report its real
failure.

```scheme
(define …₁ 1)   ;; before => Lexer error: Unexpected character: …
                ;; chibi, Gauche, Chez => fine
```

R7RS 7.1.1 spells `<identifier>` with ASCII letters, but §7.1 lets an implementation extend the
grammar and does not say how far. R6RS 4.2.4 draws a line by Unicode general category; **all three
references go past it** and read *any* character above ASCII — chibi, Gauche and Chez each accept
`“` (a curly quote) as an identifier, which no category list admits. Matching them needs no
category tables and can only widen the accepted language, so the rule is now: above ASCII,
everything except whitespace.

**Two independent causes, and the second was invisible from the first.** `is_identifier_start`
already called `is_alphabetic`, so `λ` and `café` worked all along; only non-letters like `…` (Po)
were rejected there. But `₁` (U+2081, category No) never reached that predicate at all — the token
dispatch tests `char::is_numeric`, which is Unicode-aware, so a subscript was routed to
`read_number` and reported as `Invalid number: ₁`. Fixing the identifier predicate alone would have
left SRFI 197 failing with a different message. The dispatch and both numeric peeks now test
`is_ascii_digit`, which is what R7RS number literals actually are.

Whitespace is the one exclusion, and it is where the references part company: chibi reads U+00A0 as
an identifier character, Gauche treats it as a separator. Patina does neither — it stays a lex
error, now reported as `U+00A0` rather than as an invisible glyph. Stricter than both on purpose: a
stray non-breaking space is a typo, and both welding two identifiers into one and silently
splitting a token are invisible in the source.

**The score did not move, and the entry said so in advance.** srfi-197 was the only package needing
this and it advanced from `parse-error` to `runtime-error`: its test program also
`(include "./test.scm")`s a file the package does not ship, so it cannot pass whatever we do. Done
because rejecting Unicode identifiers is wrong, not for the number.

**A library could not re-export a core syntactic keyword** — ✅ **fixed** (2026-08-16). Both
backends. `(r6rs base)` opens by re-exporting the whole of R7RS's syntax, which R7RS §5.6.1 permits
and every implementation accepts; Patina rejected the library outright.

```scheme
(define-library (re exp) (export begin) (import (scheme base)))
;; before => Exported identifier 'begin' not defined
```

Exactly the keywords the desugarer recognizes *by name* failed —
`quote quasiquote unquote unquote-splicing lambda if set! define define-syntax let-syntax
letrec-syntax syntax-rules begin import cond-expand include include-ci syntax-error expand _ ...` —
while every Scheme-level macro (`let`, `cond`, `case`, `do`, `when`, `and`, `or`) exported fine.
That split is the whole diagnosis: these keywords have no binding in any environment, so export
resolution had nothing to look up. `lib/scheme/base.sld` shows the same hole from the other side —
it omits them from its own export list, and works around it for the auxiliary syntax by binding
`else` and `=>` as *variables* (`(define else 'else)`).

The fix accepts them with no export entry, because in Patina a core keyword is recognized in every
scope already: an importer gets working syntax whether or not the export table mentions it.

**The import side had to move with it, and a first draft of this entry said the opposite.** It
claimed `(only …)` and `(except …)` "cannot hide" a core keyword. `except` behaves that way;
`only` did not — it *rejected the program* (`Identifier 'begin' not found in import set`), because
selecting a name it cannot find is an error there. That is exactly what the first consumer of a
blanket re-export does: narrow it. `only` now treats a core keyword the same way the export path
does — nothing to select, and nothing wrong — so `(import (only (r6rs base) begin …))` works.

Renaming on export is refused explicitly (`Cannot rename core syntax 'begin' on export`) rather
than silently exporting a name that would not work: the new name would have to be recognized as
syntax at the use site, and that recognition is by name. The message deliberately does not start
with `Exported identifier`, which is the prefix `patina-compat`'s classifier keys on — a deliberate
"unsupported" is not a package's missing export.

Landed with the three copies of the export loop — one in the tree-walker, two in the VM, all
byte-identical — replaced by a single `build_library` in `patina-runtime`, so the two backends
cannot drift on what a valid export is.

**This should be the last such list.** It is the third workaround in the tree for one hole — the
other two are `null-environment`'s `R5RS_SYNTAX` skip-list (`patina-primitives/src/primitives/
eval.rs`) and `lib/scheme/base.sld` binding `else` and `=>` as *variables* so `(only (scheme base)
else)` works. That last one is the deeper fix applied to two names: give syntactic keywords real
bindings — a marker value in the environment — and export resolution, `only`, `except`, `prefix`
and `rename` all work through the normal path with no list and no special case. Review costed it:
the desugarer already looks the head symbol up for macros *before* its name `match`, `Desugarer::
new()` (no env) is test-only, and `HeapObjectData::Macro` shows the shape a `CoreSyntax` sibling
would take; the real work is that library bodies desugar in a *parentless* env at three sites,
which would have to chain to a root env seeded with the markers. It would also give `_` and `...`
a real identity, which base.sld notes it could not do with variables without breaking SRFI-46
custom ellipsis. The next surface that needs to name syntax should do that instead of adding a
fourth list.

**VM: escapes out of `eval`, `load` and a `parameterize` converter were unhandled** — ✅ **fixed**
(2026-08-16). The completion of the entry below, which installed the boundary check in *one* of four
re-entry points and claimed the class closed but for one shape. Review enumerated the trait and ran
each method; two shipped R7RS procedures were wrong:

```scheme
;; load kept executing the file after the continuation was invoked
(call/cc (lambda (k) (set! kk k) (load "f.scm") 'fell-through))
;; before => (escaped (form1 form2 form3))     chibi => (escaped (form1))

;; the parameter *set* path lost the enclosing top-level define outright
(define r (call/cc (lambda (k) (set! kk k) (parameterize ((p 1)) 'done))))
;; before => Error: unbound variable: `r`      chibi, Gauche, tree-walker => from-converter
```

`make-parameter`'s converter was covered because construction goes through `apply_proc`; the set
path goes `try_call_parameter` → `call_any_sync`, a different boundary.

All four now route through one `across_reentry` helper — `apply_proc`, `eval_expr`,
`load_scheme_library` and `call_any_sync` — which takes the caller's pre-call frame depth, stashes
the escaping value on `VmState::pending_escape`, and returns a `Reentry::Escaped` the callers turn
into the non-catchable `ContinuationEscape` sentinel. Passing the depth in rather than sampling it
inside is load-bearing: `call_any_sync` has already pushed a frame by then, and sampling locally
made every ordinary return look like an escape — caught by the chibi suite as one failing
`parameterize` test, which is what that suite is for.

With every boundary signalling, the leaf-level frame check the previous entry added to
`exec_call_primitive` became unreachable — verified by instrumenting it and finding zero hits across
the chibi suite, the full test suite and each repro — and is deleted. One mechanism, at the
boundary, where the previous attempt had two at different altitudes with a comment that had already
gone stale describing them.

`pending_escape` is traced as a GC root, mirroring the tree-walker's `trace_pending_escape`: while
set, the value is reachable from nowhere else. No safe point runs in that window today, so it is
future-proofing on the same reasoning `scratch_args` is rooted under.

Guarded by `crates/patina-tests/tests/escape_from_primitive.rs`. The tree-walker's own versions of
these defects are pinned in `backend_divergence.rs` and recorded under Open.

**VM: an escaped-from primitive kept running, and `apply`/value-position escapes were wrong** —
✅ **fixed** (2026-08-16). The follow-up to the crash fix below, which closed only the call-position
path and left the class open. Two defects, one cause and one fix.

A Rust primitive re-enters the VM through `ApplyContext::apply_proc`, whose signature —
`Result<TaggedValue, EvalError>` — cannot say "a continuation escaped". So the escape arrived at the
primitive looking like an ordinary return value, and the primitive carried on:

```scheme
(member 9 '(1 2 3) (lambda (a b) (log b) (k #f)))
;; before => the callback runs for all three elements, each re-invoking the continuation
;; after  => runs once, like the tree-walker, chibi and Gauche
```

Because each of those extra invocations re-entered the continuation, the `dynamic-wind` thunks ran
once per element with it — four `in`/`out` pairs where one was expected.

The fix signals from the *boundary* rather than the leaf. `VmApplyContext::apply_proc` compares
frame depth across `run_apply_proc`; on a shrink it stashes the value on `VmState::pending_escape`
and returns `EvalError::ContinuationEscape` — which already existed and is already non-catchable, so
no `guard` in between can swallow it, and the primitive unwinds through its own `?`. The primitives
that do cleanup rather than propagate (`call-with-port`, `call-with-input-file`,
`call-with-output-file`) close their port first and then return the sentinel, which is the wanted
order. One check in `run_loop_until`'s error arm converts it back into the existing
`Ok(Some)/Ok(None)` protocol — placed *before* the catchability test, because the sentinel crosses
the registry boundary as an ordinary `VmError` and would otherwise look catchable.

Being at the boundary, it also fixed the `apply` and value-position shapes for free: those reach
primitives through `call_primitive_proc`, which has no depth check of its own and now needs none.

Not fixed, and recorded under Open: the `call-with-values` consumer shape, where a frame-count
comparison cannot see the escape at all. The diagnosis is in that entry.

Guarded by `crates/patina-tests/tests/escape_from_primitive.rs`, including the case a cruder rule
would break — a callback that captures and invokes its *own* continuation and returns normally, where
the primitive must still run to completion.

**VM: an escape out of a re-entrant primitive corrupted the register base** — ✅ **fixed**
(2026-08-15). Seven shipped procedures crashed the *process*, not merely a future hazard. A first
draft of the entry said "nothing structural stops the *next* higher-order primitive from doing so";
review enumerated the class and ran each one, and every one panicked identically —
`index out of bounds` at `set_reg_at` — where the tree-walker returned the right answer:

| Procedure | escape repro | VM | tree-walker |
|---|---|---|---|
| `member` with comparator | `(call/cc (lambda (k) (member 2 '(1 2 3) (lambda (a b) (k 'x)))))` | panic | `x` |
| `assoc` with comparator | same shape | panic | `x` |
| `force` | `(call/cc (lambda (k) (force (delay (k 'x)))))` | panic | `x` |
| `make-parameter` converter | `(call/cc (lambda (k) (make-parameter 1 (lambda (v) (k 'x)))))` | panic | `x` |
| `call-with-port` | `(call/cc (lambda (k) (call-with-port p (lambda (q) (k 'x)))))` | panic | `x` |
| `call-with-input-file` | `(call/cc (lambda (k) (call-with-input-file f (lambda (p) (k 'x)))))` | panic | `x` |
| `call-with-output-file` | `(call/cc (lambda (k) (call-with-output-file f (lambda (p) (k 'x)))))` | panic | `x` |

The last two are the functions *immediately above* the pair that had been moved to Scheme, in the
same file. That is what made a VM-level fix the only real one: relocating a procedure removes it
from the class but does not scale, since `eval` and `load` re-enter through `ctx.eval_expr` and
cannot be written in Scheme at all.

A higher-order primitive re-enters the VM to run its callback. `exec_call_primitive` hoisted the
frame's `register_base`, called the primitive, then wrote the result through that base — under a
comment asserting "primitives are frame-neutral on the `Ok` path (a re-entrant call runs its nested
frames to completion)". A `call/cc` out of the callback breaks exactly that: the nested frames are
*popped*, not completed, so the base addresses a register window that no longer exists. In release
the write landed out of bounds; in debug the `debug_assert_eq!(base, self.frame_base())` that guards
the rule fired. The bad offset tracked frame depth, so it was not a fixed-offset accident.

The fix: `exec_call_primitive` notices the frame stack shrank under it and delivers the value the
way an invoked continuation does — `Ok(Some(v))` when the unwind reached this loop's `exit_depth`,
which is the protocol `call_value` already documented, and `Ok(None)` when control resumed in a
frame between the two, where the continuation invocation has already delivered the value. Both
halves of the plumbing existed and nothing joined them: `exec_call_primitive` already had the
`Some` return path, `run_apply_proc` already captured `depth_before`.

**Scope, corrected by review before merge.** A draft of this entry said "one check covers all
seven". It covers the *call-position* path — the `CallPrimitive` arm, and the `inline_primitive!`
slow path that routes into it. The same primitives reached through `apply`, through a value, or
through `call-with-values` go via `call_primitive_proc`, which has no equivalent check; those were
already wrong before this change and remain so, recorded under Open. Nor does any path *abandon*
the primitive — it runs to completion on a stack it no longer owns, re-invoking the continuation
and re-running wind thunks with it. What this entry fixed is the crash, not the class.

Guarded by `crates/patina-tests/tests/escape_from_primitive.rs`, including a repeat-and-nest case —
an off-by-one that only worked at one frame depth would otherwise pass.

**`with-output-to-file` crashed the VM on an escape** — ✅ **fixed** (2026-08-15). Recorded as
"does not restore through `dynamic-wind`, both backends"; both halves were wrong, which is why the
first step was running it.

The tree-walker restored correctly all along — a `call/cc` escape comes back as a Rust `Err`, and the
restore sat after the call, so it ran. The VM did not miss a restore either: it **panicked**,
`index out of bounds` in `set_reg_at`. The primitive re-enters the VM to run the thunk, and the
escape pops the frames whose register base the interrupted `CallPrimitive` was still holding. That
root cause is now its own entry above, since it outlives this fix.

`with-input-from-file` and `with-output-to-file` are Scheme now
(`lib/scheme/file/redirect.scm`), which is what the original entry proposed — and it removes the
re-entrant primitive call rather than repairing it. The Rust primitives are deleted, not merely
unexported, so there is no second implementation to drift.

**This fixes one instance, not the class.** Seven other procedures still crash the same way, listed
under Open above; two of them are the functions directly above these in the file they were deleted
from. The comment left at that deletion site says so, so it cannot be read as the class being
handled.

The shape is chibi's — one `dynamic-wind`, before-thunk installing the port so a re-entered
continuation re-establishes it, after-thunk restoring the old one. Two details were found by testing
rather than reasoning, and both are wrong in the obvious first draft:

- **Restore before closing.** Closing first leaves the closed port current for the length of the
  unwind, and anything that runs in that window writes into it and disappears.
- **Close at all.** chibi does not need to; it flushes open ports at exit and Patina does not, so a
  port left open on a non-local exit loses everything buffered. R7RS §6.13.1 permits leaving it
  open, not dropping the output. The cost is that re-entering a continuation captured inside the
  thunk finds the port closed.

A first draft nested `parameterize` inside a closing `dynamic-wind` and lost a `guard` handler's
output entirely; chasing that is what turned up the handler-ordering divergence recorded above.
Guarded by `crates/patina-tests/tests/with_file_redirection.rs`, checked against chibi and Gauche.

**Tree-walker: the control primitives' own errors escaped `guard`** — ✅ **mostly fixed**
(2026-08-15); one case remains open, see the end of this entry.
The last of the class fixed in #71: a catchable error returned as a Rust `Err` instead of routed
through the Scheme exception handlers. That fix swept `step.rs`; these lived in
`cps_eval/application.rs`, which it never reached — so on the tree-walker `guard` could not catch a
control primitive's *own* arity or type error, while the VM caught all of them.

**The references decided which backend was right.** "Match the other backend" is not a reason to
believe the other backend, so all six positions were run against chibi, Gauche and Chez first:

| probe | chibi | Gauche | Chez | Patina VM | Patina tw (before) |
|---|---|---|---|---|---|
| `(with-exception-handler 5 (lambda () 'ok))` | `ok` — no check | `ok` — no check | caught | caught | escaped |
| `(dynamic-wind (lambda () 1))` | caught | caught | caught | caught | escaped |
| `(call-with-values (lambda () 1))` | caught | caught | caught | caught | escaped |
| `(raise)` | caught | caught | caught | caught | escaped |
| `(error)` | caught | caught | caught | caught | escaped |
| `((make-parameter 1) 1 2 3)` | `#<undef>` | caught | caught | caught | escaped |

Where the references disagree it is about whether to raise **at all** — chibi and Gauche accept a
non-procedure handler and just run the thunk, and chibi returns `#<undef>` for a parameter called
with three arguments, which R7RS §4.2.6 makes explicitly implementation-dependent. None of them
disagrees about catchability *once something is raised*. The old tree-walker behaviour matched no
implementation in any of the six, which is the cleanest mandate this track has had.

**Done as a sweep, not as six patches** — #71's lesson was that fixing reported instances leaves the
class alive. But the first attempt at that sweep defined the class *syntactically* — "every bare
`return Err(EvalError::…)` in the file" — and review caught that this is not the same set as "every
catchable error that can reach Scheme". `application.rs` has five `?`-propagation sites and the grep
audited none of them. Three were live escapes, two of them inside functions the sweep had already
edited:

- `(error 5)` — the message type check, fifteen lines below the arity check that *was* routed, in
  the same function. Fixed here; chibi, Gauche and the VM all catch it.
- a parameter converter that raises — `apply_from_direct_tagged(conv, …)?` in `apply_parameter`,
  thirteen lines above the arity arm that was routed. Fixed here; chibi and Gauche catch it.
- an error raised inside a `dynamic-wind` thunk — `run_wind_handlers(…)?`. **Not fixed**, see below.

Two sites are deliberately left propagating, both checked rather than assumed: `apply_other_primitive`'s
"called with non-primitive procedure" is statically unreachable (its only caller has already matched
`Procedure::Primitive`), and `eval_primop`'s arity checks have no continuation in scope but are
already routed by their caller — the `try_catchable!` arm #71 added to `step.rs`. Review also found
the first of those was `TypeError`, which `is_catchable()` treats as a *user* condition; it is now
`InternalError`, so the code and the rationale agree.

One site needed more than a mechanical rewrite. `apply_with_exception_handler` decided its type
error inside a live `heap.borrow()`, and the router allocates the exception object
(`heap().borrow_mut().alloc_exception`), so routing in place would have panicked on the RefCell.
The check now yields the message and the borrow ends before the routing call — the
extract-to-a-`let`-first discipline in `CLAUDE.md`. The same shape recurred in `apply_error`.

**Narrowed, not closed.** An error raised by user code inside a wind thunk still escapes `guard` on
the tree-walker. Recorded as its own entry under **Open** above, rather than only here — an open
defect filed inside a "Fixed" entry is not in the inventory anyone reads.

Guarded by `crates/patina-tests/tests/callability.rs`, where every body is asserted twice — caught
when guarded, still an error when not — so a future change that *swallowed* errors instead of
routing them would fail. The divergence row pinned when this was found retired itself on the fix,
printing "NO LONGER DIVERGES"; a new row now pins the wind-thunk case that remains.

**A caveat on that new row.** Its pinned value is what the VM returns, not an established correct
answer: chibi loops forever on the repro, so no reference could arbitrate. It asserts only that the
two backends disagree. The same caveat applies to the continuation-arity site routed here —
`(call/cc (lambda (k) (k 1 2)))` now yields a catchable error on the tree-walker where Gauche and
the VM return `1`, which is the known multi-value-continuation gap, not something this change
settled.

**`make-parameter` objects were not procedures** — ✅ **fixed** (2026-08-15). Both backends. Found
while checking whether the three standard ports had become *real* parameter objects or only ones
that satisfy our particular `parameterize` macro. They had, and the check turned up the mirror gap:

```scheme
(procedure? current-output-port)   ;; => #t   (correct)
(procedure? (make-parameter 1))    ;; => #f   (wrong)
```

R7RS §4.2.6 defines `make-parameter` as returning "a newly allocated parameter object, **which is a
procedure** that accepts zero arguments and returns the value associated with the parameter object";
Gauche agrees. (§7.3's sample implementation builds one out of a `lambda`, which is the same answer
from the other direction. An earlier draft of this entry cited §7.3 as the *definition* and
paraphrased it as "a newly allocated procedure" inside quotation marks — both wrong, and the sort of
thing that survives because nobody re-opens the spec.)

Only the *predicate* was wrong — `(p)`, `(apply p '())` and passing `p` through `map` all worked
before the fix, which is the shape of the defect: `Heap::is_procedure` enumerated callable variants
(closure tag, native `Procedure`, VM closure, VM continuation refs) and omitted `Parameter`.

The fix is one variant added to that enumeration, not a special case in `procedure?`, because the
other callers mean the same thing by the question. Observably that changed three decision points:
`with-exception-handler`'s argument checks on *each* backend (both rejected a parameter before the
fix — verified by running it), and `make-parameter`'s own converter check, which had worked around
the gap with `is_procedure(c) || is_parameter(c)`; that `||` is now redundant and gone. Adjacent,
and fixed alongside because the two are one thought: a parameter fell through the datum writer's
chain to `#<unknown>`, harmless until `procedure?` began answering `#t` about it.

**Two counting errors, caught by review and corrected rather than quietly dropped.** A draft of this
entry claimed eight sites "became correct together", listing `dynamic-wind` and `force` among them.
Checked one at a time: `dynamic-wind` type-checks nothing on either backend — neither
`apply_dynamic_wind` nor `VmControlPrimitive::DynamicWind` inspects its arguments, they just call
the thunks — so it accepted a parameter all along; the `patina-primitives` `with_exception_handler`
is shadowed on the normal call path by each backend's own control primitive, and reachable only
through `apply`, where its two checks do run but the body then returns `InternalError("not yet
implemented")` regardless, so they can never turn a rejection into a success; and `force`'s thunk
check is reachable only for a `Delayed` promise, which `make-promise` does not build. Counting call
sites in a grep is not the same as counting decisions, and the test that was going to guard
`dynamic-wind` would have passed against unfixed code.

**The previous entry's own claim was also wrong.** It held that the bespoke per-backend dispatch
exists *because* parameters fail `is_procedure`, so fixing the predicate would remove the need for
it. Both backends route on `get_parameter` (`try_call_parameter` in the VM, the `param_opt` branch
in the tree-walker's `application.rs`), never on `is_procedure`, and that dispatch exists because
reading and setting a parameter is its own calling convention. So step (2) — re-expressing the three
ports as `make-parameter`-backed objects — is unblocked but buys only the per-port collapse.

**The claims above are now pinned by tests**, in `crates/patina-tests/tests/callability.rs` — added
because three statements in this entry's own first draft were wrong in ways no test could have
contradicted. `dynamic-wind` performing no validation is proven by *ordering* (its before- and
body-thunks run before a bad after-thunk is reached), not by matching an error message; and the
"still half-applied" limit below is pinned as behaviour — a continuation answers `procedure?` with
`#t` yet is rejected as a `make-parameter` converter. That last test is written to fail when the gap
is closed, verified by closing it locally: whoever folds `Continuation` in gets a failure telling
them to update the test, the doc comment and this entry together.

**Still half-applied, and left open deliberately.** `is_procedure` is not yet the single source of
truth for callability: tree-walker continuations (`HeapObjectData::Continuation`) are callable but
live in `is_continuation`, so five callers still spell the question `is_procedure(x) ||
is_continuation(x)` while four omit the disjunct. Folding that variant in would let all five drop
it, but it widens the four that omit it today — a behaviour change wanting its own tests, not a
rider on this one. Recorded in the `is_procedure` doc comment so the next reader does not mistake
the enumeration for complete.

Guarded by `crates/patina-tests/tests/parameters.rs`, which was ported from a tree-walker-only local
helper to the shared both-backend helpers in the same change (Track Q Q1) — a single-backend file
could not have stated the property it was missing. It pins the widening as a *predicate*: things
that are not callable still answer `#f`, and the standard ports still answer `#t`.

**Tree-walker: an unbound variable escaped `guard` in most positions** — ✅ **fixed**
(2026-08-15). A bare `undefined-var` was a catchable condition, but `(undefined-fn)` was not — on the
tree-walker only. chibi, Gauche and Chez catch both, and the VM already did.

The defect was a class, not an instance: `step.rs` hand-copied the route-through-handlers dance in
some arms (`Var`) and `?`-propagated the same lookup failure in others, so catchability depended on
where the variable sat. Auditing every arm after fixing the reported `LetVal`/`App` case found five
more escaping positions — `if` tests, `set!` targets, `define` values, `call/cc` operands, and
unquotes — each confirmed divergent (VM caught them, tree-walker did not). The fix is structural: a
single `try_catchable!` macro in `step.rs` that every fallible user-level evaluation goes through,
routing failures into `maybe_route_error_through_cps` (which already held the catchability policy —
`InternalError` and continuation escapes still propagate). A new arm can no longer quietly
reintroduce the escape, and continuation-environment lookups stay `?` because a missing continuation
is a compiler invariant, not a user error.

Found by a test written for the import-set fix below, which used `guard` to assert a name was
unbound and got two different answers from the two backends. Pinned in
`crates/patina-tests/tests/backend_divergence.rs` as a converged row covering all nine positions.

**Rust registry primitives ignored the import set at top level** — ✅ **fixed** (2026-08-15).
VM only; the tree-walker was right all along, which turned out to be the whole story. A program
importing only `(scheme base)` could still call `cadddr` from `(scheme cxr)` or `bitwise-and` from
`(srfi 151)`, because `VmBackend::with_fs` called `install_primitives()` — a loop binding *every*
registered primitive into globals by short name. The registry knows each primitive's owning library
(it is right there in `qualified_name`, and the loop even reads it) but nothing consulted it, so
`import` never got a say.

Scheme-level exports were scoped correctly and libraries enforced their imports properly, so the
hole was specific to registered primitives at the top level — and that asymmetry is why it survived.
The same expression succeeded at the top level and failed inside a `define-library`, and the top
level is where one naturally checks. It had already cost a real bug: `(srfi 132)`'s `vector-merge`
called `cadddr` without importing `(scheme cxr)` and failed only inside the library.

**The fix was deleting a call.** `load_bootstrap()` already ran immediately afterwards, loading
`(scheme base)` and `(patina debug)` and defining exactly their exports — the correct model, already
written, already what the tree-walker did. `install_primitives()` was redundant with it and
destructive to it. Primitives reached through a library carry `registry_index: None`, which
`resolve_index_cached` fills in on first call, so the VM's primitive fast path is untouched; the
function stays for the VM unit tests, which build a bare `VmState` with no library machinery.

The predicted fallout did not arrive: **77 test binaries green, 1226/1226 on both backends, and the
corpus unchanged at 138**. In hindsight that was foreseeable — the suite runs every case on both
backends and asserts they agree, so anything relying on the leak was already failing. One corpus
package swapped `srfi-128` for `load` in its unbound list, which is the leak closing in plain view.

New guard: `crates/patina-tests/tests/primitives_reachable_by_import.rs` asserts every registered
primitive is exported by *some* shipped library. Before this change an under-exporting library
builder cost nothing, because the blanket install handed the primitive out regardless; now such a
primitive is simply unreachable, and nothing else would notice. It deliberately checks reachability
rather than the primitive's own `library` label — several are labelled with a library adjacent to
the one R7RS exports them from (`char->integer` says `scheme.char`; R7RS puts it in `(scheme base)`),
so asserting the label would be asserting a naming convention instead of the property that matters.

**Still open, and deliberately not folded in:** `lib/scheme/base.sld` exports the eight three-deep
`cxr` procedures (`caaar`…`cdddr`), where R7RS-small puts them only in `(scheme cxr)`. That is a
documented extension rather than a leak, and tightening it is a separate decision.


**The standard port procedures were not parameter objects** — ✅ **fixed** (2026-08-15). Both
backends. R7RS §6.13.1 requires `current-input-port`, `current-output-port` and `current-error-port`
to be parameter objects overridable with `parameterize`; all three were plain 0-argument procedures,
so `parameterize` rejected them outright.

The fix is smaller than this entry previously predicted, and the reason is worth keeping. It warned
that accepting a second arity would not be enough, because the ports are backed by Rust-side globals
that other primitives read directly. But `parameterize` (`lib/scheme/base/parameters.scm`) drives a
parameter *through the object itself* — `(p)` to read, `(p v)` to install, `(p old)` from
`dynamic-wind`'s after-thunk to restore. So writing the thread-local from the setter arity is not a
shortcut around the dynamic binding: the thread-local **is** the binding, and it is exactly what
`display` with no port argument consults. The version that would have been broken is the opposite
one — a Scheme-level rebinding that left the thread-local alone would redirect nothing.

`(chibi test)`'s SRFI 158 suite defines `with-input-from-string` in precisely these terms, so its
expectation drops 1 → 0 and **every upstream suite now runs at zero failures**; the expectations
table in `scheme_tests/upstream/README.md` has no non-zero row left. Redirecting the current output
port is also the capability L3 wanted for capturing a package's output without touching real stdout.

**Adjacent, and left open: `(current-output-port)` allocates a fresh wrapper per call**, so
`(eq? (current-output-port) (current-output-port))` is `#f`. Pre-existing — verified against the
build before this change — and untouched by it, since `parameterize` compares nothing and the
restore path passes the same underlying port back. It is still odd for a parameter to return a
different object each read, and `(eq? p (current-output-port))` is a reasonable thing for a package
to write. The tests in `crates/patina-tests/tests/standard_ports.rs` assert restoration by where
output *lands* rather than by identity, which is the better test regardless.

**Relinking stopped at a `quote` *argument*** — ✅ **fixed** (2026-08-14). Both backends. Found the
moment the `@` fix let `(chibi match)` load, and the only §6 entry that was blocking a corpus
package. Recorded first as "a library-internal macro loses its definition environment through a
nested `let-syntax`", which is where it was found rather than what it was — the narrowing is the
useful part, so both the symptom and the cause are kept here.

The cause is one line of tree-walking. `rewrite_refs` recursed on each **cdr** as though a cdr were
a form, and its first act on a form is to read the head for a quoting operator. A tail is not a
form: in `(pass quote (helper 1 2))` the tail is `(quote (helper 1 2))`, which read as a quote form,
so the walk returned it untouched and everything after the argument `quote` was left unrelinked.
`helper` stayed a bare name and was resolved, unsuccessfully, at the use site. Three lines are
enough:

```scheme
;; library (t m) exports only `top`; `helper` and `pass` are internal
(define-syntax helper (syntax-rules () ((_ a b) (list 'expanded a b))))
(define-syntax pass   (syntax-rules () ((_ a b) b)))
(define-syntax top    (syntax-rules () ((_) (pass quote (helper 1 2)))))
;; (import (t m)) (top)
;; Chez, Gauche, and now Patina => (expanded 1 2)
;; before                       => Error: unbound variable: helper
```

Nothing about it needed `let-syntax`, nested macros, or libraries beyond one private helper; the
three-macro chain it was found in only made `quote` land in argument position. Any template that
passes `quote` along was affected. The fix walks a form's spine once — head read in head position,
arguments read as arguments — instead of re-reading each tail as a new form, which is also how the
evaluator reads `(f quote x)`. The old walk simply disagreed with it.

**Do not read the original diagnosis as a near miss.** It blamed the relinking *basis* — a
`HashSet` of template names that "cannot express which occurrence came from that template" — and
proposed designing this together with the entry below. That would have been a rewrite of the
machinery to fix a defect in how a list is traversed. The lesson is the ordinary one: the three-macro
repro was reduced further before anything was designed, and reducing it changed the answer.

`(chibi match)` reaches this for **any pattern with a quoted tail**, since `(x . 'bad)` reads as
`(x quote bad)` and is threaded through `match-two` as three arguments. The symptom there was worth
reading too, because it named neither the cause nor the right macro: with `match-two` unlinked it
was no longer a macro, so the desugarer treated the form as an application and desugared its
arguments — including the inert getter/setter pair `((cons 1 2) (set! (cons 1 2)))` that only the
`(set! setter)` rule ever destructures — and reported `set! expects 2 arguments, got 1`.

Result: chibi-match went from a dead suite to **74 of 75**, and the corpus from parse-error to
wrong-result on that package. The one remaining failure is `match-letrec`, recorded below. The
entry below — relinking rewriting *by name* — is untouched by this and stays open.

The two entries after this one were found on 2026-08-14 by auditing for the defect class behind the
fixed items in this section — an identifier comparison made on the wrong basis. Each is cross-checked
against **two** reference implementations (Chez Scheme and Gauche), not one. Neither is reachable
from the corpus queue today; both were found by construction, so nothing in §4 depends on them. The
two after those predate this track's macro work and are unchanged.

**Literal matching had no "both unbound" case** — ✅ **fixed** (2026-08-14). Both backends.
R7RS 4.3.2 gives literal *matching* `free-identifier=?` semantics: an input matches a literal when
both denote the same binding, **or both are unbound and have the same name**. That second clause was
missing. `tagged_matches_literal` instead required the literal's scopes to be a subset of the
input's, so an *introduced* literal (carrying the enclosing expansion's scopes) could never match a
*substituted* input (carrying none), even with both unbound:

```scheme
(define-syntax m
  (syntax-rules ()
    ((_ e) (let-syntax ((n (syntax-rules (k) ((n k) 'lit) ((n x) 'notlit))))
             (n e)))))
(list (m k) (m other))
;; before => (notlit notlit)
;; after / Chez / Gauche => (lit notlit)
```

Bindings are not visible in that function; the caller's `is_literal_shadowed_tagged` veto runs first
and is the binding-aware half. So the fix is to compare names, matching what the function's three
other arms already did — the identifier-vs-identifier arm was the lone exception.

Note this is a *different question* from the one below, and conflating them is the trap: **membership**
("is this pattern identifier a literal at all?") compares identity, while **matching** ("does this
input match that literal?") is `free-identifier=?`. Predates the identity work below — verified on
`ac6f6d2`.

Moved `arvyy-interface` off `Name mismatch in interface implementation … proc0 proc0` — two
identifiers spelled alike that compared unequal — onto an unrelated failure.

**Identifier identity was decided by name inside a macro that writes a macro** — ✅ **fixed**
(2026-08-14). Both backends. Three places compared identifiers by name alone, so an identifier
*substituted* from the outer use site and one *introduced* by the outer template were confused
whenever they were spelled the same. Every case below was cross-checked against Chez Scheme.

| Site | Was | Now |
|---|---|---|
| `matcher/literal.rs` | a pattern literal with empty scopes matched **any** identifier | matches the same identifier, via the general name-and-scopes comparison that was already below it |
| `compiler/pattern.rs` | any substituted identifier became a *literal* pattern | classified by the literals list alone, per R7RS 4.3.2 — so a substituted identifier that is not a literal is an ordinary pattern variable |
| `compiler/{helpers,template,escape}.rs` | pattern variables keyed by name | keyed by `(name, scopes)`, so a substituted identifier never collides with an introduced pattern variable spelled alike |

The literals list now keeps the scopes each identifier was written with (`LiteralSpec` in
`patina-core/src/compiled_macro.rs`); both literals parsers previously discarded them.

This unblocks the `new-symbol?` guard in `(chibi parse)`'s `grammar-bind`. `syntax-rules` cannot
compare two identifiers, so the guard builds an inner macro whose literals list holds the names bound
so far and calls it with an identifier that matches nothing: a literal cannot match, so the fallback
rule answers "already bound", while a non-literal is a pattern variable, matches, and answers "new".
With a literal that matched anything, the answer was always "new" and the same grammar nonterminal
got a variable more than once — surfacing as `Duplicate parameter 'space' in lambda`.

Watch the interaction with the fix below: the two `space` parameters were *correct* duplicates, so
rejecting them was right. The defect was upstream of that, in the guard that should have stopped the
second one being created.

**A recursive macro's per-expansion definitions collapsed onto one binding** — ✅ **fixed**
(2026-08-23). Both backends. The sibling of the entry below, and the half it left undone:
`ScopedParam` and `Var` carried their hygiene scopes, `CoreExprKind::Define` did not. A template
that introduces a binding gets a fresh scope per expansion, so a recursive macro defining one
temporary per element must produce that many bindings; with the name alone identifying it, each
definition overwrote the last.

```scheme
(define-syntax bind-each
  (syntax-rules ()
    ((_ () ((name val tmp) ...))
     (begin (define tmp val) ... (define name tmp) ...))
    ((_ ((n v) . rest) (acc ...))
     (bind-each rest (acc ... (n v tmp))))))
(bind-each ((a 1) (b 2) (c 3)) ())
(list a b c)
;; before        => (3 3 3)
;; chibi, Gauche => (1 2 3)
```

Found by probing Track L's "bundlable" queue rather than from the queue itself: `(srfi 166)` was
filed as blocked on a missing `(srfi 165)`, and staging that library showed the real blocker was
ours. SRFI 165's `define-computation-type` builds exactly this shape, so it failed at three or more
fields — and `(srfi 166 base)` declares 22. The corpus reported it as
`Parse error in srfi/166/base.sld: runtime error: Type error: cdr expects a pair`, three steps from
the cause.

`Define` now carries a `ScopeSet`, populated by the desugarer from the identifier it defines and
consumed differently by each backend, matching how each already handles a scoped parameter:

- **Tree-walker** binds at run time through the new `Environment::define_scoped_definition`.
- **VM** renames in `alpha_rename`, which had two holes of its own. It collected a body's
  definitions with `ScopeSet::new()` — discarding what `Define` had no field to carry — and it
  looked one level into `Begin`, so a macro expanding via `define-values` (itself a `begin` of
  definitions) hid every definition inside a group the scan did see. Top-level definitions got a
  frame they never had.

**The R7RS suite caught the fix's own overreach, which is the part worth keeping.** A first version
bound the definition under its scopes *only* — correct-looking, and it broke `jabberwocky`:

```scheme
(define-syntax jabberwocky
  (syntax-rules ()
    ((_ hatter)
     (begin (define march-hare 42)
            (define-syntax hatter (syntax-rules () ((_) march-hare)))))))
(jabberwocky mad-hatter)
(mad-hatter)  ;; => 42
```

The reasoning that failed was "an introduced identifier is hygienic, so only the expansion that
produced it can refer to it, and that expansion is inside this one form". A macro-generated *macro*
outlives the form, and the reference from its template is resolved **by name**, through
`link_definition_env_refs` — which Track L §6 separately records as rewriting by name. So a
scope-only binding is unreachable from exactly the code that most needs it. Both backends keep a
name-only view of the scoped definition — the most recent one, which is what happened when
definitions carried no scopes at all — and both reach the *binding* rather than a copy of its
value, so a mutation through either path is visible from the other.

**The first attempt at that view was a copy, and review caught it.** Storing the value under the
bare name as well as under its scopes made a `set!` through the scoped path leave the two
disagreeing — `main`, chibi and Gauche answer `2` where it answered `1`. `Environment` already
documents why, at the field the relinking uses: *"resolving through the alias on every lookup means
a later `set!` on the original binding is visible, which copying the value at expansion time would
silently freeze."* The lesson is narrower than "don't copy": the mechanism this needed already
existed and said so at the site, and the second cell was invented beside it.

**The read/write asymmetry was the second thing review caught.** `get` gained the name-only view of
a scoped definition and `set` did not, so a definition became readable and unassignable — visible
when the `set!` sits inside a macro the expansion *generated*, which arrives relinked to the bare
name rather than carrying scopes. `alias_bindings` states the rule this broke: "Reads follow
aliases in `get`; writes have to as well or the two disagree." Both now go through one
`visible_scoped_index`, so the two cannot pick different bindings.

**Two VM shapes remain wrong, both strictly better than before this change**, and both trace to the
same place — a renamed top-level definition is a global under a new name, minted from a counter
that `alpha_rename` resets per top-level form:

```scheme
(mk ((a 1) (b 2)) ()) (mk ((c 3) (d 4)) ()) (list (a) (b) (c) (d))
;; chibi, tree-walker (1 2 3 4) · VM (3 4 3 4) · before this change (4 4 4 4)

(define tmp 5) (mk ((a 1) (b 2)) ()) (list tmp (a) (b))
;; chibi, tree-walker (5 1 2) · VM (2 1 2) · before this change (2 2 2)
```

The first is the counter; the second is the name-only alias the rename forces, which defines the
bare name and so clobbers a source-written global. The deeper fix names itself: derive the unique
name from the binding's scope set — `ScopeId`s are already process-unique — which makes renaming
unconditional and unique across forms, and install the alias through `Environment::define_alias`,
which is checked *after* real bindings and is an indirection rather than a copy. That is a VM
design change rather than a defect fix, so it is recorded here rather than attempted.

Regression tests in `crates/patina-tests/tests/compliance/macros_advanced.rs`, both backends, seven
of them: the collapse at top level and in a body, the same through `define-values`, the two
`jabberwocky` shapes that must not regress, and the two mutation cases the first attempts broke.

**Recursive macros could not introduce a fresh binding per expansion** — ✅ **fixed** (2026-08-14).
Both backends. `check_no_duplicates_scoped` in `patina-frontend/src/desugarer/utils.rs` keyed its
`HashSet` on the parameter *name*, so two params introduced by different expansions of the same
template were rejected as duplicates even though their scope sets differed:

```scheme
(define-syntax gen
  (syntax-rules ()
    ((_ () (args ...)) (lambda (args ...) (list args ...)))
    ((_ (x . rest) (args ...)) (gen rest (args ... a)))))
((gen (1 2) ()) 10 20)
;; before => Error: Duplicate parameter 'a' in lambda
;; after / Gauche / Chez => (10 20)
```

Nothing downstream needed changing: `ScopeId::fresh()` per expansion already gave each introduced
`a` its own scope, and both the VM's `alpha_rename` pass and the tree-walker bind params by name
*and* scopes. Only the eager desugarer check disagreed. The two rest-parameter checks beside it
compared by name alone for the same reason and now share one `binds_identifier` rule. Genuine
duplicates — `(lambda (q q) q)`, `(lambda (q . q) q)` — still error. Regression tests in
`crates/patina-tests/tests/compliance/macros_advanced.rs` run on both backends and include SRFI
156's `extract-placeholders` shape, which is what surfaced this.

**Tree-walker: nested `guard` across a procedure boundary** — ✅ **fixed**. Was 15 errors in the
R7RS suite, all `Undefined variable: k_N`. Pre-existing (reproduced on `main` before this track) and
invisible until upstream `(chibi test)` was adopted, because its applier calls every test thunk from
inside its own `guard`.

Cause: `capture_cont_bindings` returned `None` for the six `ContValue` variants that decorate another
continuation with an effect — popping an exception handler, caching a forced promise, running a wind
thunk, delivering multiple values. Returning `None` dropped the whole entry from the captured
continuation environment, so the captured body's reference to that binder was stranded. Those
variants now capture through to the continuation they wrap, via one shared
`ContValue::wrapped_cont()` rule rather than six arms. The wrapper's effect is still not serialized,
which matches the escape path's existing behaviour of rebuilding handler and wind state.

The VM was never affected: `call/cc` snapshots whole machine state instead of serializing
continuations name by name, so there is no per-variant case to leave unimplemented.

```scheme
(define (run thunk)
  (guard (e (#t (list 'outer e)))
    (list 'ok (thunk))))

(run (lambda () (guard (x (else 'inner)) (error "boom"))))
;; tree-walker => Error: Undefined variable: k_7
;; VM          => (ok inner)
```

`(chibi test)` hits it because `test-default-applier` calls the test thunk from inside its own
`guard`, so every suite test whose body uses `guard` trips it — which is why the errors cluster in
6.10 Control Features and 6.11 Exceptions. A single `guard`, a `guard` under `dynamic-wind`, a
`guard` under `call/cc`, and two sequential `guard`s all work; only the nesting-across-a-call case
fails.

**Bare `@` rejected as an identifier** — ✅ **decided and fixed** (2026-08-14). Both backends. We now
read a leading `@`, which was the largest parse-error bucket at 9 packages and moved the corpus
129 → 133.

Patina was *strictly correct*: R7RS 7.1.1 makes `@` a ⟨special subsequent⟩ and not an ⟨initial⟩, so
a bare `@` is not a conforming identifier. The decision to relax it rests on two facts, not on
counting packages:

1. **Accepting it cannot change the meaning of any conforming program**, because no conforming
   program contains a bare `@` token. This is a pure widening — unlike the `syntax-rules` shape
   buckets above, where matching chibi would mean accepting genuinely malformed input.
2. **The ecosystem treats `@` as a datum, not as sloppiness.** SXML specifies `@` as the attribute
   marker (and `@raw` as a tag); `(chibi match)` uses `@` as a `syntax-rules` literal for its
   named-record-field pattern. Chez, Gauche and chibi all read it.

The reader/writer asymmetry that follows is deliberate and matches the references: we read `@` but
still **write** it as `|@|`, exactly as Gauche does (Chez writes `\x40;`). The invariant worth
holding is that our writer's output reads back as itself under our own reader, not that we mimic
another implementation's spelling.

Scope was one `matches!` arm in `is_identifier_start` (`patina-frontend/src/lexer/mod.rs`), which
already admitted `.` and non-ASCII letters — both themselves deviations from a literal reading of
7.1.1, so this is consistent with what the lexer already did rather than a new posture. `,@` is
lexed before identifiers and so is unaffected, as is `⟨real⟩@⟨real⟩` polar notation (`@` is an
identifier *start*; a leading digit still reads as a number). Both are covered by unit tests
alongside `crates/patina-tests/tests/at_identifiers.rs`.

