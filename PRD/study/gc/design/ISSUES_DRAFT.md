# GC redesign: GitHub issue drafts

Drafted 2026-10-01 against `main` at `28a94f8` (clean tree). **Nothing here has been filed.** The drafts follow
`design/DESIGN.md` (the recommended design) and `DIGEST.md` §1.11, and the project rules in AGENTS.md: a defect gets an
issue before its fix, and the PR closes it; PRDs stay high level and work items are issues; behaviour is scored
against chibi and Gauche and recorded in `DIVERGENCES.tsv`; performance claims come from interleaved main/branch/main
runs.

## How to use this file

- **Placeholders.** `#S0`, `#ST`, `#D`, `#A1`…`#A9` and `#W0.1`… stand for issue numbers that do not exist yet.
  Replace them when filing. "PR *n.m*" is a pull request in the plan below; the issue it closes is listed in the PR map.
- **One issue per PR.** When a PR fixes a group-A defect, that defect issue is the PR's issue, and it carries the
  work-item detail under "Fix". Those PRs have no separate group-B issue. The PR map shows which issue each PR closes.
- **Design references.** "GC design §n" uses the numbering of `design/DESIGN.md`. Issue #D lands Parts I and II
  of that file as `docs/GC_DESIGN.md` and `PRD/future/GC_STAGE5_PRD.md`, and it renumbers Part I (decision 22).
  Whoever files after #D merges should turn these references into links.
- **Filing order.**
  1. The group-A defects can be filed at once. They are present-day defects and do not wait on the owner's approval
     of the design.
  2. Next come #S0, #ST and #D.
  3. Then the stage 0–2 work items and the later-stage issues.
  4. Last, #D's PR replaces each S-label in the design text with its issue link.
- **Labels** are taken from the repository's existing set: `bug`, `documentation`, `enhancement`, `performance`
  and `question`.
- **Searched before drafting** (open and closed issues, `PRD/ARCHIVE/`, `docs/`, `scheme_tests/reports/`):
  - The port `eq?` defect was noted and "left open" in `PRD/ARCHIVE/TRACK_L_FIXED_DEFECTS.md` under the
    standard-ports entry, but it was never filed. #A3 cites that note.
  - #597 (open) already covers documentation drift, including "GC status". The GC and encoding drift is therefore
    drafted as a comment on #597 (#A6), not as a new issue.
  - GC_STAGE5_PRD Priority 2 (nested-loop collection) has no issue. PR 2.5 and stage 4e supersede it.
  - #587 (open) covers callback and prompt tests under GC stress. It is related to the lanes but is not a duplicate.
  - #423 (register precision), #338/#353 (code release) and #471/#476–#478 (resumable primitives) are closed
    prerequisites.
  - #438 (closed) is about library macro references and is distinct from #A5.
  - No issue covers the embedder use-after-free, the teardown leak, `EMFILE`, the byte-blind trigger, the
    ephemeron fixpoint or the rebinding divergence.
- **Verification.** Every defect in group A was re-run on 2026-10-01:
  - against a release binary built from `28a94f8` into a separate target directory;
  - with small Rust probe crates that depend on the repository by path, built in debug and release;
  - on both backends;
  - against chibi 0.12.0 and Gauche 0.9.15 where a program can run on them.

  The commands and raw results are in the appendix. The machine is an Apple M4 Pro running macOS 27.2 arm64.

## Index

| ID | Title | Kind | Labels | Closed by |
|---|---|---|---|---|
| #A1 | Embedding: a value returned by `eval_str` is freed by a later collection | defect | bug | PR 2.3 |
| #A2 | Embedding: dropping an `Interpreter` leaks its heap, and output buffered in its open file ports is lost | defect | bug | PR 2.2 |
| #A3 | `(eq? (current-output-port) (current-output-port))` is `#f`; chibi and Gauche answer `#t` | defect | bug | stage 4a |
| #A4 | Opening files fails at the 1,021st unclosed port under `ulimit -n 1024`; chibi and Gauche finish | defect | bug | PR 1.3 |
| #A5 | Code compiled before a redefinition or re-import follows the new binding; chibi and Gauche keep the old one, and no `DIVERGENCES.tsv` row records it | divergence record | — | PR 0.5 |
| #A6 | GC and value-encoding statements contradict the source (comment on #597) | docs | documentation | PR 0.6 |
| #A7 | The collection trigger counts objects, not bytes: large vectors and deep continuation captures grow memory without a collection | defect | bug, performance | PR 1.1 |
| #A8 | One `(gc)` over a chain of 16,000 ephemerons takes 214 ms in one order and 1 ms in the other | defect | performance | stage 5a, or earlier |
| #A9 | A top-level call to a procedure the program named `define-library` is parsed as a library definition | defect | bug | PR 2.4a |
| #S0 | GC redesign: tracking issue (stages, measurement table, off-heap holder inventory) | tracking | enhancement | — |
| #ST | Threading model: SRFI 18 as M:1 green threads, GC interfaces written for N mutators | decision record | question | — |
| #D | Docs: replace `docs/GC_DESIGN.md` and `PRD/future/GC_STAGE5_PRD.md` with the GC redesign | docs | documentation | PR D |
| #W0.1 | GC stage 0: phase timings, a GC log, pause keys in `(gc-stats)` and the deferral high-water counters | work item | enhancement | PR 0.1 |
| #W0.2 | GC stage 0: a `gc` mode for the benchmark runner and the GC benchmark set | work item | enhancement, performance | PR 0.2 |
| #W0.3 | GC stage 0: vendor the memory-mapping and continuation probes under `scripts/gc_probes/` | work item | enhancement | PR 0.3 |
| #W0.4 | GC stage 0: a `gc-census` cargo feature for allocation, survival and store-mix measurements | work item | enhancement | PR 0.4 |
| #W1.2 | GC stage 1: `(gc)` collects at its call, through `Step::Collect` | work item | enhancement | PR 1.2 |
| #W1.4 | GC stage 1: delete the source-location stores nothing reads | work item | performance | PR 1.4 |
| #W2.1 | GC stage 2: slot-based root visitor, open root registration, `CallFrame.closure` as a traced value | work item | enhancement | PR 2.1 |
| #W2.4 | GC stage 2: process bare top-level imports outside the desugarer, recognized by the binding of `import` | work item | enhancement | PR 2.4 |
| #W2.5 | GC stage 2: library bodies loaded from top-level imports collect between and inside their forms | work item | performance | PR 2.5 |

The stage issues for stages 3–9 and P are listed after group B. Stage 0 files them as stubs.

## PR map for stages 0–2

Efforts are focused engineer-weeks [I]. The design estimates stage 0 at 3–4 weeks, stage 1 at 4–5 and stage 2 at 5–7.

| PR | What | Issue | Depends on | Effort |
|---|---|---|---|---|
| D | GC design text replaces `docs/GC_DESIGN.md` and `GC_STAGE5_PRD.md` (documentation only) | #D | owner approval; #S0 and the stage issues filed | 0.5 |
| 0.1 | phase timings, `PATINA_GC_LOG`, pause keys, the deferral high-water counters | #W0.1 | — | 0.5–1 |
| 0.2 | `gc` mode of `scripts/benchmarks.py`, the GC benchmark set (GBS) | #W0.2 | 0.1 | 1–1.5 |
| 0.3 | OS and continuation probes under `scripts/gc_probes/` | #W0.3 | — | 0.25 |
| 0.4 | `gc-census` feature | #W0.4 | — | 0.5–1 |
| 0.5 | rebinding suite file and its `DIVERGENCES.tsv` rows | #A5 | — | 0.5 |
| 0.6 | GC and encoding documentation drift | #597 (comment #A6) | — | 0.25 |
| 1.1 | byte trigger, byte keys in `(gc-stats)`, rewritten reclamation proofs | #A7 | 0.1, 0.2 | 1.5–2 |
| 1.2 | `(gc)` through `Step::Collect` | #W1.2 | 0.1 | 1–1.5 |
| 1.3 | `EMFILE` collect-and-retry; descriptor pressure | #A4 | 1.2 (1.1 for the byte charge) | 1 |
| 1.4 | delete the unread provenance stores | #W1.4 | 0.2 | 0.5 |
| 2.1 | slot visitor, `RootSet::register`, `CallFrame.closure` as a value | #W2.1 | 0.1 | 1.5 |
| 2.2 | heap teardown when an interpreter drops | #A2 | — | 0.5–1 |
| 2.3 | `Owned` handles for embedder-held values | #A1 | 2.1, 2.2 | 1.5 |
| 2.4a | top-level `define-library` routed by binding | #A9 | — | 0.25 |
| 2.4 | bare top-level imports hoisted out of the desugarer, by binding | #W2.4 | 2.4a | 0.5–1 |
| 2.5 | rooted loading: collection between and inside library forms (VM) | #W2.5 | 2.1, 2.4 (1.1 for the K16 counters in bytes) | 1–2 |

Stages 0–2 thus total about 12.5–17 weeks [I]. Parallelism: PRs 0.3–0.6, 1.4, 2.2 and 2.4a touch disjoint files and can
go in any order. PR 1.2 and PR 2.1 both edit `heap/gc.rs`, so they are serialized.

---

# Group A: present-day defects

## #A1 · Embedding: a value returned by `eval_str` is freed by a later collection

**Labels:** bug · **Closed by:** PR 2.3 · **Depends on:** PR 2.1 (open root registration), PR 2.2 (teardown)

#### Symptom

A host keeps a value that an `Interpreter` returned and then evaluates more code that collects. The host then reads
freed memory: debug builds panic, and release builds print whatever reused the slot. Both backends do this.

```rust
use patina_interpreter::{Backend, Interpreter};

fn run<B: Backend>(interp: Interpreter<B>) {
    interp.eval_program("(import (scheme base) (patina debug))").unwrap();
    let held = interp.eval_str("(list 'held (vector 1 2 3))").unwrap();
    interp
        .eval_program(
            "(define (churn n) (if (> n 0) (begin (cons n n) (churn (- n 1)))))
             (churn 200000) (gc) (define keep (list 'a 'b 'c 'd))",
        )
        .unwrap();
    println!("{}", interp.display_tagged(held)); // expected (held #(1 2 3))
}

fn main() {
    run(Interpreter::new_vm()); // and Interpreter::new_tree_walker()
}
```

| Build | VM | Tree-walker |
|---|---|---|
| debug | panics: `use-after-free: pair slot 15992 was reclaimed by the GC` (`crates/patina-core/src/heap/mod.rs:721`) | the same |
| release | prints `(45264 . 45264)` | the same |

The interpreter's own entry points do the same. `eval_program_resilient` returns the last value that succeeded. If a
later form collects and then fails, the value it returns has been freed:

```rust
// after defining `churn` as above
let v = interp.eval_program_resilient("(list 'first 1 2) (begin (churn 200000) (gc) (car 5))");
println!("{}", interp.display_tagged(v)); // expected (first 1 2)
```

The debug build panics with `use-after-free: pair slot 16012`, and the release build prints `(45290 . 45290)`.

All of this was measured on 2026-10-01 at `28a94f8`.

#### Cause

- Every `eval_*` method returns a bare `TaggedValue` (`crates/patina-interpreter/src/lib.rs:278` and the methods
  after it).
- Nothing roots a value the host holds. The embedding API has no handle or root type. The only accidental root is
  `global_env().define(name, v)`.
- `run_forms` keeps the previous form's value in a Rust local while the next form is evaluated
  (`lib.rs:491-517`). A later form's safe point can therefore collect it.

#### Fix (PR 2.3)

Scope:
- An `Owned` handle holds `{heap_id, index, generation}` plus a `Weak` to a per-heap handle table. The table is a
  root provider registered through `RootSet::register` (PR 2.1). `Owned` supports `get`, `Clone` and `Drop`; a
  `Drop` after teardown (PR 2.2) does nothing.
- `Interpreter<B>` gains `Owned`-returning forms of the `eval_*` methods. The bare-value forms are deprecated under
  #601's convention: `#[deprecated(note = …)]`, existing callers keep compiling, and `#[allow(deprecated)]` is added
  where internal callers remain, so that `clippy -D warnings` stays green. `display_tagged` accepts both forms.
- `run_forms` and `eval_program_resilient` hold the running value in a handle across forms.
- The public `Backend` trait is not changed in this PR. Stage 3 moves it to raw words through the `pub unsafe` API.
  `scripts/check_embedding_features.sh` stays green.
- Crates: `patina-core` (the handle table), `patina-runtime`, `patina-interpreter`, and tests.

Acceptance:
- [ ] Both shapes above pass as tests, on both backends, in debug and release builds. They also pass under
  `PATINA_GC_STRESS=1`.
- [ ] Dropping a handle after its interpreter has been dropped neither panics nor leaks. This is checked in the
  debug-poison lane.
- [ ] A handle from one interpreter that is used with another is refused deterministically, by the heap-id check.
- [ ] The ~200 existing test call sites of the bare forms still compile.
- [ ] The GC differential lanes stay byte-identical (release and debug-poison, both backends). Both chibi scripts
  pass. `check_embedding_features.sh` passes.

Measurement:
- `eval_str` on a trivial form, run main/branch/main interleaved for at least 10 rounds, stays within 1% in
  instructions retired.

Docs:
- `PRD/FFI_DESIGN.md`: handles replace the exported `HeapIndex` and `SharedHeap`.
- The `patina-interpreter` crate docs and examples.

Effort: about 1.5 weeks [I].

---

## #A2 · Embedding: dropping an `Interpreter` leaks its heap, and output buffered in its open file ports is lost

**Labels:** bug · **Closed by:** PR 2.2

#### Symptom

After an `Interpreter` is dropped, its heap stays alive, with 36 strong references remaining. A file port that is
still reachable from a global therefore never drops, and nothing flushes its buffer. A host that drops its
interpreter and returns from `main` finds the file empty. The CLI is unaffected, because `end_process` flushes every
open output port (`crates/patina-runtime/src/exit_status.rs:63`), but embedders do not call it.

```rust
use patina_interpreter::{Backend, Interpreter};
use std::rc::Rc;

fn run<B: Backend>(interp: Interpreter<B>, path: &str) {
    let heap = {
        let interp = interp;
        interp.eval_program(&format!(
            "(import (scheme base) (scheme file))
             (define p (open-output-file {path:?}))
             (write-string \"hello\" p)")).unwrap();
        Rc::downgrade(interp.global_env().heap())
    }; // the interpreter is dropped here
    println!("heap alive = {}, strong = {}", heap.upgrade().is_some(), heap.strong_count());
}
```

| Backend | After the drop | File after the process exits |
|---|---|---|
| VM | heap alive, `strong = 36` | empty (0 bytes); expected `hello` |
| Tree-walker | heap alive, `strong = 36` | empty (0 bytes) |

Measured on 2026-10-01 at `28a94f8`. A separate probe that holds only a closure gives `strong = 37`.

How often this happens in practice:
- In one `cargo test -p patina-tests` process, up to 318 heaps are alive at once.
- 272 of 276 test processes exit with every heap still alive (workload census, 2026-10-01).

#### Cause

The cause is an `Rc` cycle:
- A heap object holds an `Rc<Environment>`: `VmClosure.globals`, `EnvironmentSpecifier`, tree-walker `Procedure`
  payloads, and `Macro` → `CompiledMacro.definition_env`.
- The environment holds the heap through `Environment.heap: SharedHeap` (`crates/patina-core/src/environment.rs:489`).
  `CompiledMacro.heap` (`compiled_macro.rs:483`) does the same.

The collector breaks such cycles only while the heap is alive and collecting, by tombstoning dead slots.
`Heap` has no `Drop` that clears its arenas. `docs/GC_DESIGN.md`'s goal "cycles are reclaimed" therefore holds
within a running interpreter but not across its teardown.

#### Fix (PR 2.2)

Scope:
- `Heap::teardown()` is run from the `Drop` of each backend (`VmBackend`, `TreeWalker`) and of `Interpreter`.
- Teardown tombstones every arena slot. That drops the `VmClosure.globals`, `EnvironmentSpecifier`, `Procedure` and
  `Macro` payloads, which breaks the cycle, and drops every `Port` payload, which flushes and closes it.
- Teardown is idempotent.
- Any `SharedHeap` or `Rc<Environment>` clone that outlives the interpreter sees an empty heap. Reading a value
  through it is a use-after-free that the debug-poison build reports. That is the documented contract: values do
  not outlive their interpreter.
- Deleting the `heap` fields themselves is left to the stage-3 codemod.

Acceptance:
- [ ] After dropping an interpreter, a `Weak` of its heap no longer upgrades, on both backends.
- [ ] `dropped_interpreter_flushes_ports`: an embedder drops its interpreter while an unclosed file port is
  reachable from a global, and returns from `main`. The file then holds the output, on both backends. The test
  runs in a subprocess and outside the byte-identical lane.
- [ ] Creating and dropping 300 interpreters in a loop keeps peak RSS flat. This is the `many-heaps` shape from
  #W0.2.
- [ ] The lanes are byte-identical. `cargo test --all --lib --tests` passes. `check_embedding_features.sh` passes.

Measurement:
- Startup and teardown time (`phases/startup/bootstrap_and_drop` in `docs/VM_TESTING.md`), run interleaved, may
  grow by the cost of the tombstoning pass. The PR reports that cost.

Effort: 0.5–1 week [I].

---

## #A3 · `(eq? (current-output-port) (current-output-port))` is `#f`; chibi and Gauche answer `#t`

**Labels:** bug · **Closed by:** GC stage 4a (canonical identity and ports), which `#S0` tracks

#### Symptom

```scheme
(import (scheme base) (scheme write) (scheme file))
(define p (open-output-file "eq-out.txt"))
(write (list (eq? (current-output-port) (current-output-port))
             (let ((q #f))
               (with-output-to-file "eq2.txt"
                 (lambda () (set! q (eq? (current-output-port) (current-output-port)))))
               q)
             (eq? p p)
             (parameterize ((current-output-port p)) (eq? (current-output-port) p))))
(newline)
```

| | Result |
|---|---|
| Patina VM | `(#f #f #t #f)` |
| Patina tree-walker | `(#f #f #t #f)` |
| chibi 0.12.0 | `(#t #t #t #t)` |
| Gauche 0.9.15 | `(#t #t #t #t)` |

Measured on 2026-10-01 at `28a94f8`. R7RS §6.13.1 makes `current-output-port` a parameter. A parameter that returns
a different object on each read is odd, and `(eq? p (current-output-port))` is a reasonable test for a package to
write.

#### Cause

- `current-input-port`, `current-output-port` and `current-error-port` each allocate a fresh heap wrapper around the
  same `Rc<Port>` on every call (`crates/patina-primitives/src/primitives/io/ports.rs:444,463,482`).
- `values_eq` (`crates/patina-core/src/heap/mod.rs:2118-2146`) has no `Port` arm.

This was recorded as "Adjacent, and left open" in `PRD/ARCHIVE/TRACK_L_FIXED_DEFECTS.md` (the standard-ports entry),
but it was never filed.

#### Fix

The fix comes in GC stage 4a, the canonical-identity work:
- A port becomes one heap object `{header, port id}` over a per-heap `PortTable`.
- The standard ports become parameter objects that hold that object (requirement R6).

That stage's issue carries the scope. A wrapper cache would need a rooted thread-local, and stage 4a replaces the
thread-locals anyway, so a fix now on today's representation is not proposed.

Pinning it now is cheap, and a fix would turn the pin red:
- add the row above to `crates/patina-tests/tests/scheme/stdlib/ports.scm` as `test-expect-fail`;
- add `patina-defect` rows for chibi and Gauche in `DIVERGENCES.tsv`.

Acceptance (stage 4a):
- [ ] The row answers `(#t #t #t #t)` on both backends.
- [ ] The quarantine and the two divergence rows are removed.
- [ ] `one_port_one_object` (GC design, Appendix F) passes on both backends.

---

## #A4 · Opening files fails at the 1,021st unclosed port under `ulimit -n 1024`; chibi and Gauche finish

**Labels:** bug · **Closed by:** PR 1.3 · **Depends on:** PR 1.2 (`Step::Collect`); PR 1.1 for the 8 KiB external charge

#### Symptom

A loop that opens a file and drops the port runs out of descriptors, although every dropped port is garbage:

```scheme
(import (scheme base) (scheme file) (scheme write))
(define count 0)
(write
 (guard (e (#t (list 'failed-at count (file-error? e))))
   (let loop ()
     (if (< count 100000)
         (begin (open-input-file "data.txt") (set! count (+ count 1)) (loop))
         (list 'ok count)))))
(newline)
```

Run under `/bin/sh -c 'ulimit -n 1024; exec …'`:

| | Result |
|---|---|
| Patina VM | `(failed-at 1021 #t)` |
| Patina tree-walker | `(failed-at 1021 #t)` |
| chibi 0.12.0 | `(ok 100000)` |
| Gauche 0.9.15 | `(ok 100000)` |

Measured on 2026-10-01 at `28a94f8`.

The research run added more shapes, each failing where chibi completes:
- `ulimit -n 128` with 5,000 input or 5,000 output opens: Patina fails at 125 on both backends and Gauche at 124, while chibi completes all 5,000.
- 20,000 opens, each followed by 50 conses: Patina fails at 1,660 on the VM and 1,661 on the tree-walker.

#### Cause

- The collection trigger counts heap slots. A port's descriptor and its 8 KiB buffer are invisible to it, so
  collections are rare in a loop like this one.
- No open retries after a collection.
- Today's collection does close a dead file port: the sweep drops the payload, and `BufWriter`/`File` close it.
  Collecting at the right moment is therefore enough.
- chibi collects once and retries on `EMFILE` (`eval.c:1300-1323`), and its port finalizer flushes and closes.
- Gauche's trigger counts bytes, including port buffers. It also forces a collection when its port table fills
  (`port.c:1301-1333`).

#### Fix (PR 1.3)

Scope:
- `open-input-file`, `open-output-file`, the binary variants and the internal open used by the `call-with-*` and
  `with-*` file procedures answer `Step::CollectAndRetry` on `EMFILE` or `ENFILE`. The machine runs a full
  collection at the call's return pc, with the arguments still in the suspended frame, and calls the primitive once
  more. A second failure raises the `file-error` the program sees today.
  - On the VM this goes through `resume_stub`. On the tree-walker it goes through `ResumePrimitive`.
  - The retry collects in `PATINA_GC=0` too, as `(gc)` does, so the differential lanes stay comparable.
- **Descriptor pressure.** Opens minus closes since the last collection are counted. Reaching
  `min(128, RLIMIT_NOFILE / 4)` posts a collection.
- **External bytes.** Each open, unclosed file port charges 8 KiB to the byte trigger of PR 1.1, if that PR has
  landed first.
- Where collection is deferred (a nested loop, or a library load before PR 2.5), the first `EMFILE` still raises.
  This is documented. The loader's own opens retry from stage 4a.
- Crates: `patina-primitives` (`primitives/io/ports.rs`, `registry.rs`), `patina-vm` (`control.rs`
  `resume_stub`), `patina-tree-walker` (`ResumePrimitive`), and `patina-core` (the counter).

Acceptance:
- [ ] `descriptor_exhaustion_retries`: the program above completes with `(ok 100000)` on both backends. It runs in
  a subprocess with `RLIMIT_NOFILE` set to 1024. The `port-churn` probe of #W0.2 is the same shape.
- [ ] `garbage_port_flushed_by_collection` (R2) pins today's GC-time flush: a file port is written and then
  dropped. The file is empty before a collection and holds the output after it. The test runs once with an
  explicit `(gc)` and once with a collection triggered by allocation. Both port tests stay outside the
  byte-identical lane, because GC-time flushing is observable.
- [ ] `open-close-10k` (opens that are closed) causes no descriptor-pressure collection.
- [ ] The control-flow matrix passes on both backends, and so does `escape_from_primitive.rs`, with a continuation
  captured around a retried open.
- [ ] Lanes, both chibi scripts and `unclosed_output_ports.rs` pass.

Measurement:
- The I/O workloads of #W0.2 (Larceny cat, wc, string, slatex, bibfreq, read1), run interleaved for at least 10
  rounds, stay within ±1% in instructions.

Effort: about 1 week [I].

---

## #A5 · Code compiled before a redefinition or re-import follows the new binding; chibi and Gauche keep the old one, and no `DIVERGENCES.tsv` row records it

**Labels:** none (a divergence record, not a defect) · **Closed by:** PR 0.5

#### Observation

```scheme
;; p1: a procedure compiled and called, then the name it uses is redefined
(import (scheme base) (scheme write))
(define (f p) (car p))
(write (f '(1 2))) (newline)
(define car (lambda (p) 'mine))
(write (f '(1 2))) (newline)
```

```scheme
;; q2: the same name re-imported from another binding
(import (scheme base) (scheme write))
(define (f p) (car p))
(write (f '(1 2))) (newline)
(import (rename (only (scheme base) cdr) (cdr car)))
(write (f '(1 2))) (newline)
```

| | p1 | q2 |
|---|---|---|
| Patina VM | `1 mine` | `1 (2)` |
| Patina tree-walker | `1 mine` | `1 (2)` |
| chibi 0.12.0 | `1 1` | `1 1` (with "WARNING: importing already defined binding: car") |
| Gauche 0.9.15 | `1 1` | `1 1` |

Measured on 2026-10-01 at `28a94f8`. The binding-cells study measured more shapes the same day. Several use a small
`(counter)` library that exports `count`, `bump!` and `get-count`.

| Shape | Patina (both backends) | chibi | Gauche | Chez |
|---|---|---|---|---|
| p1b (as p1, `f` not called before the redefinition) | follows | old | follows | old |
| p4 (program `define`s an imported variable; library `bump!`) | `(b 100 100 2)` | `(b 2 100 2)` | `(b 2 100 2)` | `(b 0 100)` |
| p8 (reference compiled before a mid-program import supplies the name) | `0` | error | error | error |
| p9 (program defines `count`, then imports `(counter)`) | `(1 1)` | `(mine 1)` + warning | `(mine mine)` | n/a |
| q1 (`call-with-values`/`dynamic-wind` sites run, then redefined) | `mine mine` | old | old | n/a |
| d1 (`(begin (error "boom") (define list-copy 5))`, then `(list-copy '(1 2))`) | `(1 2)` | error | error | error |

Six Rust tests pin Patina's answers:
- in `crates/patina-tests/tests/vm_callprimitive.rs`: `import_rebind_deoptimizes`,
  `tail_deopt_returns_correct_result`, `tail_deopt_runs_deep_mutual_recursion`, `define_after_use_deoptimizes` and
  `control_forms_define_after_use_deoptimize`;
- in `import_modifiers.rs`: `modifier_rebinding_invalidates_already_compiled_primitive_calls`.

No row in `DIVERGENCES.tsv` records the difference. `PRD/TRACK_P_PERFORMANCE_PRD.md` §P8 describes Patina's answer as
"exact R7RS top-level redefinition semantics". R7RS §5.2 says otherwise:
- In a program or library it is an error to redefine an imported binding, to import one identifier with different
  bindings, or to refer to an identifier before it is imported.
- A REPL "should permit" these actions.

The rows are therefore `latitude`, not defects on either side.

The answer stays as it is for now. Owner decision 2 keeps "follow the name" (variant R) through global-cells stage
4b and recommends compile-time binding (variant C) after stage 5. Decision 3 settles the remaining cases one by one.
This issue only records the divergence before any of that work starts.

#### Fix (PR 0.5)

Scope:
- Add a suite file, `crates/patina-tests/tests/scheme/stdlib/rebinding.scm`, for the shapes that need no library:
  p1, p1b, q1, q2, q3 and d1. Each row asserts Patina's current answer.
- Rows must not disturb the SRFI 64 harness. Rebind names the harness does not use (`list-copy`, `car` inside a
  procedure the harness never calls), or run each shape through `eval` in a fresh mutable environment.
- The library shapes p4, p8 and p9 go into `stdlib/library-bindings.scm`, which Gauche arbitrates. chibi cannot
  define a library in a script, and that file is already `*` for chibi. chibi's answers are recorded by hand in the
  notes, as that file does today.
- Add `DIVERGENCES.tsv` rows of class `latitude`, citing R7RS §5.2 and this issue:
  - p1, p1b (chibi only), q1, q2, q3 and d1, against chibi and Gauche;
  - p8 and p9, against Gauche.

  Each note gives the measurement date and the Chez answer where there is one.
- Point the TRACK_P §P8 sentence at the rows, as a dated note. The record itself stays.

Acceptance:
- [ ] `./scripts/run_suite_oracles.sh` passes against the new rows under chibi 0.12 and Gauche 0.9.15.
- [ ] Both backends answer every row as asserted.
- [ ] The six pinned Rust tests are unchanged.

Effort: about 0.5 week [I].

---

## #A6 · GC and value-encoding statements contradict the source (comment on #597)

**Labels:** documentation · **Posted as:** a comment on #597, whose scope ("GC status … match the source")
already covers it. If the owner prefers a separate issue, the same text works under the title "Docs: GC and
value-encoding statements contradict the source".

> Further GC and encoding drift found on 2026-10-01 at `28a94f8`, each checked against the source:
>
> 1. `AGENTS.md:11` says `TaggedValue` values are "NaN-boxed". They are a 64-bit word with a 3-bit low tag: fixnum
>    `000`, heap references as arena index `<< 3 | tag` (`crates/patina-core/src/tagged_value.rs:55-110`).
>    `docs/GC_DESIGN.md:84` already says it is not NaN-boxing.
> 2. `AGENTS.md:180` says `control_flow_matrix.rs` has "24 transfer shapes". It has 64: 2 extents × 2 positions ×
>    16 transfers (`control_flow_matrix.rs:309-328`, checked by `the_matrix_is_a_complete_cross_product`).
>    `docs/VM_RUNTIME.md:715` already says 64.
> 3. `docs/GC_DESIGN.md:3` reads "Status: Approved design, not yet implemented", but stages 1–4c landed on
>    2026-08-01/03. Line 65 counts "26 variants" of `HeapObjectData`, which has 28 (`heap/mod.rs:143-229`).
> 4. `crates/patina-core/src/heap/gc.rs:25` says the threshold is "never, for the default `Off` mode". `On` has been
>    the default since stage 4c (`gc.rs:284-288`).
> 5. `docs/VM_RUNTIME.md` describes a `value_buffer` at `:395`, at `:549` and in §6's root table (`:896`). No such
>    field exists in `crates/patina-vm/src`. Stale mentions remain in comments at
>    `crates/patina-tests/tests/gc_vm.rs:11,104` and `crates/patina-vm/src/types/instruction.rs:416`.
> 6. `PRD/TRACK_P_PERFORMANCE_PRD.md:51` and `PRD/TRACK_Q_QUALITY_PRD.md:54` give `~/Project/r7rs-benchmarks` as
>    the benchmark harness. It is not present on the development machine. These are dated records, so they should
>    be marked historical rather than rewritten, and should point at the checked runner (`scripts/benchmarks.py`)
>    and the Larceny suites under `~/Project/reference/larceny`.
>
> Items 3 and 4 disappear if the GC redesign's documentation PR (#D) lands first, because it rewrites
> `docs/GC_DESIGN.md`. Items 1, 2, 5 and 6 are independent of it.

---

## #A7 · The collection trigger counts objects, not bytes: large vectors and deep continuation captures grow memory without a collection

**Labels:** bug, performance · **Closed by:** PR 1.1 · **Depends on:** PR 0.1 (logging), PR 0.2 (the GBS, for the
neutrality gate)

#### Symptom

```scheme
;; 500 vectors of 100,000 elements, each garbage at once
(import (scheme base) (scheme write) (patina debug))
(let loop ((i 0))
  (when (< i 500) (make-vector 100000 0) (loop (+ i 1))))
(write (assq 'collections (gc-stats))) (newline)
```

```scheme
;; 20,000 continuations captured 1,000 frames deep and dropped at once
(import (scheme base) (scheme write) (patina debug))
(define (at-depth d thunk) (if (= d 0) (thunk) (+ 1 (at-depth (- d 1) thunk))))
(at-depth 1000
  (lambda ()
    (let loop ((i 0))
      (when (< i 20000) (call/cc (lambda (k) k)) (loop (+ i 1))))
    0))
(write (assq 'collections (gc-stats))) (newline)
```

Peak RSS, from `/usr/bin/time -l`. The chibi and Gauche versions drop the `(patina debug)` import and the final
line.

| Program | Patina VM | Patina tree-walker | chibi 0.12.0 | Gauche 0.9.15 |
|---|---|---|---|---|
| 500 × `(make-vector 100000 0)` | 414 MB, 0 collections | 414 MB, 0 collections | 17 MB | 44 MB |
| 20,000 captures at depth 1,000 | 3.2 GB, 1 collection | 46 MB, 1 collection | 8.8 MB | 35 MB |
| empty program (for scale) | 11.7 MB | — | — | — |

Measured on 2026-10-01 at `28a94f8`. The research run measured more, without re-running it here:
- Larceny's `gcold` reaches 628 MB peak RSS with at most 12 MB live.
- 80,000 captures at depth 1,000 reach 5.7 GB.

#### Cause

- `Heap::note_alloc` counts allocations (`crates/patina-core/src/heap/mod.rs:581-584`). Collection fires after
  `max(65,536, 2 × live slots)` of them (`heap/gc.rs`, `auto_threshold`).
- A vector, string, bignum or bytevector is one slot whatever its payload. A 100,000-element vector costs the
  trigger the same as a pair.
- A VM `call/cc` copies the register stack and frames into a side table (about 160 KB at depth 1,000) and allocates
  one `VmContinuationRef` slot.
- The tree-walker's continuations share `Rc` frames, which is why its column stays small.

#### Fix (PR 1.1)

Scope:
- `Heap::note_alloc(bytes)`. Every `alloc_*` charges its slot plus the capacity of its payload: vector, string and
  bytevector buffers, bignum limbs, record field vectors, closure free-variable vectors, and the registers and
  frames of a continuation snapshot.
- The threshold becomes `max(8 MiB, 2 × L)`. L is the live bytes after the last collection: the marked slots, the
  payloads of marked objects, and the continuation payload bytes that `trace_weak_ids` proves live. Counting the
  last term keeps retained continuations from forcing a collection every 8 MiB of capture.
- `PATINA_GC_STRESS` keeps counting allocations until the arenas are gone (stage 5e). `PATINA_GC=0` is unchanged.
- `(gc-stats)` gains `live-bytes`, `bytes-allocated`, `bytes-reclaimed` and `committed-bytes`. The current keys
  (`pairs`, `free-pairs`, `last-swept`, …) stay as diagnostics.
- **The reclamation proofs are rewritten in this PR.** The 8 MiB floor is above the default-mode proof's churn
  (200 K conses, about 3.2 MB of slots), so `scripts/run_gc_differential.sh:160-175` would stop collecting at all.
  The proofs at `run_gc_differential.sh:139-175` and `crates/patina-tests/tests/common/mod.rs:644,706,844` move to
  representation-independent keys:
  - `live-bytes` after a full `(gc)`, minus a baseline, below a bound;
  - `committed-bytes` not growing across a churn loop of at least 16 MiB;
  - `collections` above 0;
  - a guard that `bytes-reclaimed` is above 0, so no later stage can pass vacuously.
- Crates: `patina-core` (`heap/{mod,gc}.rs`), `patina-primitives` (`primitives/gc.rs`), `patina-vm` (the snapshot
  charge, `vm_state/gc_roots.rs`), `scripts/run_gc_differential.sh`, `crates/patina-tests/tests/common/mod.rs`.

Acceptance:
- [ ] The 500-vector program peaks below 100 MB on both backends (414 MB today).
- [ ] The capture program (`samedepth1000` in #W0.2) peaks at least 10× lower on the VM (3.2 GB today).
- [ ] `gcold` peaks at 120 MB or less (628 MB in the research run).
- [ ] `retained-continuations`, whose captures stay live, does not collect every 8 MiB of capture.
- [ ] The rewritten reclamation proofs pass with `bytes-reclaimed` above 0, in the release and debug-poison lanes,
  on both backends.
- [ ] The lanes are byte-identical, and both chibi scripts and both Larceny lanes pass.

Measurement:
- The GBS (#W0.2), run main/branch/main interleaved for at least 10 rounds with bootstrap 95% confidence
  intervals, has a geomean within ±1% in cycles.
- The PR reports per-workload peak RSS and collection counts.

Docs:
- `docs/TEST_ORGANIZATION.md` (the rewritten proofs).
- The `(gc-stats)` docstring.

Effort: 1.5–2 weeks [I].

---

## #A8 · One `(gc)` over a chain of 16,000 ephemerons takes 214 ms in one order and 1 ms in the other

**Labels:** performance · **Closed by:** GC stage 5a (the weak contract), or earlier on today's collector if wanted

#### Symptom

```scheme
(import (scheme base) (scheme write) (scheme time) (scheme process-context)
        (srfi 124) (patina debug))
;; e_i = (make-ephemeron k_i k_(i+1)); the program holds k_0 and the ephemerons,
;; in the order given on the command line.
(define n (string->number (cadr (command-line))))
(define forward? (string=? (caddr (command-line)) "forward"))
(define keys (let loop ((i n) (acc '())) (if (< i 0) acc (loop (- i 1) (cons (list i) acc)))))
(define k0 (car keys))
(define ephs                                   ; e_(n-1) ... e_0
  (let loop ((ks keys) (acc '()))
    (if (null? (cdr ks)) acc (loop (cdr ks) (cons (make-ephemeron (car ks) (cadr ks)) acc)))))
(define held (if forward? (reverse ephs) ephs))
(set! keys #f) (set! ephs #f)
(define t0 (current-jiffy))
(gc)
(define t1 (current-jiffy))
(write (list n (exact (round (/ (* 1000 (- t1 t0)) (jiffies-per-second)))) 'ms)) (newline)
```

| n | VM, forward | VM, reverse | Tree-walker, forward | Tree-walker, reverse |
|---|---|---|---|---|
| 1,000 | 2 ms | 1 ms | 2 ms | 1 ms |
| 4,000 | 14 ms | 1 ms | 14 ms | 1 ms |
| 8,000 | 54 ms | 1 ms | 53 ms | 1 ms |
| 16,000 | 214 ms | 1 ms | 217 ms | 1 ms |

Measured on 2026-10-01 at `28a94f8`. The forward column grows by a factor of 4 for each doubling of n, which is
quadratic.

#### Cause

The weak fixpoint (`crates/patina-core/src/heap/gc.rs:1022-1101`) rescans every pending ephemeron in every round. In
the forward order each round resolves one more link of the chain, so the work is O(n²).

#### Fix

Use key-indexed resolution:
- An ephemeron whose key is not yet marked is chained on that key, and the key is flagged.
- When the marker marks a flagged key, it traces the values of the ephemerons waiting on it (Whippet
  `gc-ephemeron.c`; Chez uses per-segment triggers, `c/gc.c:747-766`).

The redesign does this in stage 5a with a `KEYHINT` bit in the side metadata byte (GC design §6.6). It can also land
on today's collector with a pending-key bitset beside the mark bits, if the owner wants it before stage 5. Either way
the fix keeps one fixpoint for ephemerons and weak continuation ids together; sequencing them separately was the
use-after-free fixed in commit `1d18c49`.

Acceptance:
- [ ] Work on the 16 K chain is linear in both orders, counted as marked ephemerons or fixpoint rounds rather than
  timed. This becomes a test in `ephemerons.rs`.
- [ ] All `ephemerons.rs` tests and the weak-continuation tests pass, and Larceny's `ephemeron` suite passes 6 of 6
  on the VM.
- [ ] The lanes are byte-identical.

Effort: about 0.5–1 week on today's collector [I]; inside stage 5a's estimate otherwise.

---

## #A9 · A top-level call to a procedure the program named `define-library` is parsed as a library definition

**Labels:** bug · **Closed by:** PR 2.4a

This was found while checking the precedent that #W2.4 would follow. It is small, but it touches the routing code
that #W2.4 changes, and AGENTS.md's rule that a fast path keys on the binding, never on the spelling.

#### Symptom

```scheme
(import (scheme base) (scheme write))
(define (define-library . args) (write (list 'called args)) (newline))
(define-library 1 2)
(write 'end) (newline)
```

| | Result |
|---|---|
| Patina VM | exit 1: `Parse error in <inline define-library>: Invalid syntax: Expected proper list in feature requirement` |
| Patina tree-walker | exit 1: `define-library failed: Parse error in <inline define-library>: …` |
| chibi 0.12.0 | `(called (1 2))` then `end` |
| Gauche 0.9.15 | `(called (1 2))` then `end` |

The same call in a non-top-level position, such as `(write (define-library 1 2))`, already calls the procedure on
both backends. Measured on 2026-10-01 at `28a94f8`.

#### Cause

`patina_frontend::is_define_library_form` (`crates/patina-frontend/src/library_support.rs:61-70`) matches the head
by its spelling, `define-library` or `library`. Both backends route a top-level datum that it matches to the library
loader before desugaring (`crates/patina-vm/src/backend.rs:238`, `crates/patina-tree-walker/src/backend.rs:114`).

#### Fix (PR 2.4a)

Scope:
- Route a top-level datum to the library loader only when its head identifier has no variable or macro binding in
  the evaluation environment. That condition is "recognized by binding".
- Put the check in one helper that #W2.4 reuses for `import`.

Acceptance:
- [ ] The program above prints `(called (1 2))` and `end` on both backends.
- [ ] Inline `define-library` in scripts and the REPL still loads.
- [ ] `library-bindings.scm`, `expansion/template-references.scm` and the library-availability tests pass.
- [ ] Both chibi scripts pass.

Effort: about 0.25 week [I].

---

# Group B: tracking and decision issues

## #S0 · GC redesign: tracking issue (stages, measurement table, off-heap holder inventory)

**Labels:** enhancement

> The garbage collector is being redesigned around a measured finding: the costs are in the representation, not
> in mark-sweep.
> - The 72 B enum slots, `Rc` payloads, relocating `Vec` arenas and `Rc<RefCell<Heap>>` cost about 2.05× the bytes
>   of a headered layout.
> - The worst pause today is 178 ms, after loading 25 R7RS-large libraries. About 18 ms of it is sweep; the rest is
>   releasing 2.9 M Rust `Drop` payloads and pruning provenance.
>
> The design lives in `docs/GC_DESIGN.md` and the plan in `PRD/future/GC_STAGE5_PRD.md`, both landed by #D. This
> issue tracks the stages and carries two records that belong to no single stage: the measurement table every later
> claim is judged against, and the inventory of everything outside the heap that holds a Scheme value today.
>
> **Owner decisions so far.**
> - The tree-walker is kept but may lag. Its heaps stay whole-heap and non-moving, and it collects only at its own
>   safe points.
> - Throughput comes first, with bounded stop-the-world pauses. There is no concurrent or incremental marking.
> - Pluggability is a contract, not a catalogue. One production collector, `MarkRegion`, is selected statically.
>   `NullGc` exists only in the conformance suite. There are no load barriers, no `dyn` on fast paths and no code
>   patching.
> - The engine is bespoke and in-tree, with MMTk-shaped seams but no MMTk dependency.
> - Threads follow #ST.
>
> **Open:** decision 7, whether shared-memory parallelism under SRFI 18 is a goal (#ST). The remaining decisions
> (2, 3, 10–23) have proposed defaults in the design's decision table.
>
> **Stages** (one line each; the stage issue carries the detail):
>
> | Stage | Issue | Gate |
> |---|---|---|
> | 0 Ground truth: GC benchmark mode and set, GC log, census, probes, defect and stage issues | this issue; #W0.1–#W0.4, #A5, #597 | baselines below reproduced within their confidence intervals; no behaviour change |
> | 1 Quick wins on today's collector: byte trigger, `(gc)` at its call, `EMFILE` retry, unread provenance stores deleted | #A7, #W1.2, #A4, #W1.4 | trigger blindness and descriptor exhaustion fixed; reclamation proofs non-vacuous; GBS ±1% |
> | 2 Root and boundary contract: slot visitor, handles, teardown, top-level import hoisting, rooted loading | #W2.1, #A2, #A1, #A9, #W2.4, #W2.5 | embedder use-after-free and teardown leak fixed; library bodies loaded through `import` collect; GBS ±1% |
> | 3 `Mutator`, collect capability, `Cx`, store funnel, polls at frame entry, `InterruptHandle`, `Interpreter::call` | S3 | see S3 |
> | 4a–4g canonical identity and ports; global cells (variant R); identifiers as ids; frames and stacks; continuations as heap objects; tree-walker host payloads; inline payloads | S4a–S4g | see each |
> | 5 the new heap, kind by kind | S5 | kill criterion K9 against the stage-4 exit baseline |
> | 6 JIT ABI spike and freeze | S6 | K6, K7 and K11 decided |
> | 7 sticky generations behind a switch | S7 | M2 and M5; K1, K2 and K8 |
> | 8 opportunistic evacuation | S8 | move-all lane green for 4 weeks; K3 |
> | 9 threads readiness | S9 | two-mutator lane; thread-lifetime tests |
> | P parallel stop-the-world marking, if `large-live` exceeds 100 ms | SP | ≥ 2.5× on 4 workers, byte-identical output |
>
> **Present-day defects found during the research:** #A1, #A2, #A3, #A4, #A5 (record), #A7, #A8 and #A9.
> Documentation drift is on #597.
>
> **Measurement table.** Measured on an Apple M4 Pro under macOS 27.2 arm64, release build at `28a94f8`. Rows marked
> ✓ were re-run on 2026-10-01 while these issues were drafted. Stage 0 re-measures every row through PR 0.2 and PR
> 0.4 and posts the results here.
>
> | Quantity | Value | Workload or probe |
> |---|---|---|
> | bytes against a headered layout | 2.05× | allocation census, 348 M objects over the 20 GBS workloads |
> | object sizes | 56% exactly 16 B; 99.2% ≤ 128 B; 148 objects > 8 KiB | the same census |
> | allocation mix | closures 35.3%, pairs 27.9%, flonums 19.8% | the same census |
> | survival at a 64 K-allocation interval | ≤ 1.5% on 10 of 20 workloads; nboyer 43%; mperm 58%; queue3 and deeprec 100% | survival census |
> | store mix | the immediate-value filter removes 64% of heap stores; 99.4% of the median workload's stores hit young holders | store-mix census |
> | pauses | 41 ms (queue3); 178 ms on the first collection after loading 25 libraries; 14.8–15.4 ms for a `(gc)` right after it | collector instrumentation |
> | collector rates | sweep 2.4 ns per slot; mark 2.8–3.0 ns per live pair, ≈ 4.7 ns per live vector, 8.1 ns per live closure | collector instrumentation |
> | deep stacks | deeprec: 12.3 ms mean mark over 960 K frames and 11.5 M registers | collector instrumentation |
> | live heaps | peak 50 MB (queue3), 41 MB (mperm), 26 MB (deeprec), 22 MB (nboyer), in the headered layout | per-collection census |
> | 10 M-deep non-tail recursion | 0.50 s, 1.37 GB | deep-recursion probe |
> | ✓ 500 × `(make-vector 100000)` | 414 MB peak RSS, 0 collections, both backends | #A7 |
> | ✓ 20,000 captures at depth 1,000 | VM 3.2 GB peak RSS, 1 collection; tree-walker 46 MB | #A7 |
> | gcold | 628 MB peak RSS with ≤ 12 MB live | Larceny gcold |
> | ✓ unclosed opens under `ulimit -n 1024` | `EMFILE` at the 1,021st, both backends | #A4 |
> | capture at depth 1,000 | 24 µs and about 171 KB each | `samedepth1000` |
> | library loading (26 libraries, VM) | RSS 627 MiB; malloc peak 765 MiB, 316 MiB of it provenance; 464 of 501 MiB empty capacity after the load | library-load census |
> | ✓ `(import (nieper rbtree))` | peak RSS 200 MB on the VM, 202 MB on the tree-walker, 1 collection; the inlined program peaked at 211 → 118 MiB (malloc) with between-form collection | #W2.5 |
> | ✓ ephemeron chain, 16 K | 214 ms against 1 ms by order | #A8 |
> | ✓ embedder use-after-free | debug panic; release prints a reused slot | #A1 |
> | ✓ teardown | `strong = 36` after the drop; port output lost | #A2 |
> | many heaps | up to 318 live heaps in one test process; 4,096 × 16 GiB reservations succeed | process and `mmap` probes |
> | decommit on macOS | `MADV_FREE` alone leaves resident size unchanged; `MADV_FREE` + `PROT_NONE` or `mmap(MAP_FIXED)` returns it | `scripts/gc_probes/madv.c` (#W0.3) |
> | safe-point cost | +1.1–1.4% for today's per-instruction check | `docs/GC_DESIGN.md` §6.1 (not re-run) |
>
> **Off-heap holders of Scheme values today, and their fate.** This is the inventory that "hidden references"
> (Julia's lesson) are checked against. A new holder must be added here.
>
> | Holder today | Fate | Stage |
> |---|---|---|
> | VM register `Vec` and `frames: Vec<CallFrame>` | per-thread reserved stack with interleaved, initialized frames | 4d |
> | `CallFrame.closure: Option<HeapIndex>` (special `visit_object_index` path) | a traced value | 2 (#W2.1) |
> | `CallFrame.code: Rc<CodeObject>` | 4d: a raw pointer plus a per-thread `Rc` side vector; 4e: a descriptor reference | 4d / 4e |
> | `CodeObject.constants: Vec<TaggedValue>` in an `Rc` | inline descriptor constants | 4e |
> | environments, `FORWARDED`/`Owner` import links, `GlobalCacheEntry`, the `env_id` cache | binding records over immortal cells; per-code-unit link tables | 4b |
> | `VmClosure.globals: Rc<Environment>` (one link of #A2's cycle) | deleted | 4b |
> | `Library.exports: HashMap<String, TaggedValue>` | name → cell | 4b |
> | `CompiledMacro` literals, `CompiledMacro.heap`, untraced `foreign_expansions` environments (`compiled_macro.rs:540`; a latent gap with no repro, because they are library environments the registry roots) | literal vector in a host handle; `heap` field deleted | 3 / 4g |
> | `syntax_sources` (316 MiB at peak), `SourceMap.locations`, child spans | the two unread stores and the throwaway per-expansion `SourceMap` deleted (#W1.4); provenance inline in identifiers | 1 / 4c |
> | transient raw-bits sets (writer, parser, `quoted`, `OpenNodes`, memos, `PrimitiveCallMap.by_value`) | unchanged: valid because nothing collects inside a `Cx` window and nothing moves before stage 8 | — |
> | `CoreExpr`/`CpsExpr` literals, `Step` state, macro literals | `pub unsafe` raw-word API under `NoGcScope`, or a traced root; later the `CoreExpr` literal pool | 3 |
> | VM continuation side tables, `VmContinuationRef` | deleted: continuations are heap objects | 4e |
> | `WindRecord.handlers: Rc<[…]>`, copied per `dynamic-wind` | a heap vector | 4e |
> | `symbol_table`, `core_syntax_table` (re-marked every collection) | interner over immortal symbols | 5c |
> | `Parameter { values: Rc<RefCell<Vec>> }` | transitional heap layout (4g); deep-bound parameterization as heap data (9) | 4g / 9 |
> | `%parameterize-swap!` calling parameter-like procedures from Rust (`primitives/parameters.rs:195-245`) | standard ports become parameter objects; other procedures go through a Scheme loop with a `guard` undo | 4a |
> | record `Rc<RTD>` + `Rc<RefCell<Vec>>`, promise `Rc`, `MutableCell` `RefCell` | inline heap fields through the store funnel | 4g / 5c |
> | `Port(Rc<Port>)` and the `thread_local!` current ports | `PortTable` plus a port object; current ports in the dynamic environment | 4a / 9 |
> | tree-walker `StepResult`, CPS graphs, `PENDING_ESCAPE` | pinned roots in non-moving tree-walker heaps; payloads as host ids; `PENDING_ESCAPE` becomes an evaluator field | 2 / 4f |
> | values held by embedders | `Owned` handles | 2 (#A1) |
> | tracer snapshots, debugger hook storage, profiler samples, green-thread scheduler | registered `RootProvider`s | 2 (#W2.1) |
>
> **Stage-0 exit.** PR 0.1–0.4 have landed, and the rows above have been re-measured here with confidence
> intervals, including K16's two high-water marks on the GBS, libload and both Larceny lanes.

---

## #ST · Threading model: SRFI 18 as M:1 green threads, GC interfaces written for N mutators

**Labels:** question (decision 7 is open)

> This issue records the threading model the GC redesign assumes, and asks the one question that decides how far it
> goes.
>
> **Verdict.** Build one mutator now, and design the GC's interfaces for N mutators sharing one heap.
> - SRFI 18 ships as M:1 green threads on one OS thread, VM first. The estimate is 8–12 weeks after the GC work, then
>   +2–3 for the tree-walker and +2–4 for non-blocking I/O [I].
> - The design target is N carriers over one shared heap with stop-the-world collection. That is the shape of
>   Gambit SMP, OCaml 5, Racket's parallel threads and Loom. It would be reached through M:N and is not built now.
> - Isolates are optional and GC-neutral.
>
> **Why a shared heap, and why not parallelism now.** SRFI 18 allows parallelism but does not require it. Invoking
> another thread's continuation is well defined, a new thread inherits the dynamic environment, and neither a
> switch nor `thread-terminate!` runs `dynamic-wind` thunks. Green threads over one shared heap meet all of this,
> as Gambit, the reference implementation, shows. Isolates cannot, because SRFI 18 assumes shared mutable state.
> Real shared-heap OS threads are estimated at 9–18 engineer-months from today's code [I].
>
> **The rule for the GC:** every per-thread concept gets an explicit owner object, and every protocol is written for
> all mutators, compiling to plain code while there is one.
>
> - **The carrier and the thread are separate objects.**
>   - The `Mutator` (carrier, `#[repr(C)]`) is also the JIT's ABI object. It owns the allocation buffer, the store
>     buffer, the root scopes and the safepoint state.
>   - The `GreenThread` owns its register stack, its watermark, a `ran_since_gc` bit and its dynamic environment.
>   - `allocs_since_gc`, `gc_threshold`, `gc_pending` and `gc_defer_depth` move off `Heap`.
>   - No runtime state stays in `thread_local!`.
> - **N-ready now, at zero single-thread cost:**
>   - per-mutator allocation and store buffers, never shared;
>   - a block pool behind an uncontended mutex;
>   - metadata-byte read-modify-writes through one type, plain under M:1;
>   - heap words through one slot type;
>   - one poll word with a deterministic tick mode for tests;
>   - safe regions around blocking I/O, no-ops with one mutator;
>   - deep-bound dynamic state;
>   - the two-mutator test lane (stage 9).
> - **Obligations of a future `threaded` build:**
>   - atomic sub-word stores for `string-set!` and `bytevector-u8-set!`;
>   - acquire and release ordering at the store funnel;
>   - safe regions entered only with no unrooted values;
>   - a dual-mapped code reservation on Linux.
>
>   The cargo feature exists from stage 3, so clippy lints it, but no lane runs it.
> - **Deferred until decision 7 says yes:** OS-thread carriers, handshakes, a `Send`/`Sync` heap, and TSan, loom and
>   Miri-concurrency lanes. Parallel stop-the-world *marking* (stage P) needs GC worker threads, not mutator
>   threads, so it does not wait on this.
> - **SRFI 18 objects.**
>   - Threads, mutexes and condition variables are heap objects, and the scheduler's root provider reports every
>     started thread that has not terminated.
>   - A thread blocked for ever on objects nothing else reaches stays rooted until it terminates, as in Gambit
>     (thread groups) and chibi (its list of blocked threads), so no `DIVERGENCES.tsv` row is needed (decision 23).
>   - A terminated thread gives its stack back at the next poll.
>
> **Cost of real shared-heap threads (decision 7), per item** [I]:
>
> | Item | Weeks | Delivered by stages 1–9? |
> |---|---|---|
> | heap and arena redesign with per-carrier allocation buffers | 6–10 | yes |
> | `Rc` → `Arc` and `RefCell` → locks or atomics (about 600 `Rc<` sites, 115 `RefCell<` types) | 8–14 | only the heap's own |
> | heap call-site migration | 4–6 | yes (stage 3) |
> | precise rooting to remove deferral | 4–8 | yes (2, 3, 4e) |
> | handshakes, safe regions, per-mutator GC state | 3–5 | partly (3, 9); handshakes remain |
> | shared code store and caches | 2–4 | yes (4e) |
> | concurrency testing and performance recovery | 8–16 | no |
>
> What remains after stage 9 is about 4–9 engineer-months. Isolates (one interpreter per OS thread) are about 3–5
> weeks after stage 5e.
>
> **Questions for the owner:**
> 1. Is shared-memory parallelism under SRFI 18 a goal (decision 7)? The default is "not now".
> 2. May invoking another thread's continuation be an error? The design assumes it is supported, as SRFI 18 and
>    Gambit require.
> 3. Is it acceptable that blocking I/O stalls every thread in the first SRFI 18 release?
>
> **Primary sources:**
> - SRFI 18 and SRFI 226 (parameter inheritance);
> - Go's per-P `mcache`; JEP 444 (Loom); OCaml 5 (ICFP 2020); Chez `c/thread.c` (`Sdeactivate_thread`);
> - Gambit `lib/_thread.scm`; chibi `lib/srfi/18/threads.c`;
> - Racket's parallel threads (2025); PEP 703.

---

## #D · Docs: replace `docs/GC_DESIGN.md` and `PRD/future/GC_STAGE5_PRD.md` with the GC redesign

**Labels:** documentation · **Depends on:** owner approval of the design (decision 22); #S0 and the stage issues filed

> Land the reviewed GC redesign as documentation only, rewriting existing files in place. No new markdown file is
> created (AGENTS.md).
>
> Scope:
> - Part I (the contract: encoding, object model, invariants, barrier, roots, polls, continuations, pluggability,
>   decisions) becomes the body of `docs/GC_DESIGN.md`.
> - Part II (one line per stage, and the kill-criterion table with `#kn` anchors) becomes the body of
>   `PRD/future/GC_STAGE5_PRD.md`.
> - Part III's work-item blocks are the bodies of the stage issues and are not committed. Part IV (the review
>   record) is this PR's description.
> - Renumber Part I consecutively. Turn each K*n* into a link to its anchor and each §15/§16 mention into a link
>   into the PRD. Replace each S-label with its issue link.
> - Update AGENTS.md's "Active planning docs" entries for both files.
> - **Policy reversals stated explicitly** in the PR description:
>   - the current PRD's non-goal "moving/compacting collection — ruled out permanently" is replaced by opportunistic
>     evacuation at stage 8, behind a measured gate;
>   - the trait names `Collector`/`GcRoots` change to `Collector<M>` and `RootProvider`;
>   - "concurrency" stays a non-goal, while parallel stop-the-world marking is budgeted (stage P).
>
> Acceptance:
> - [ ] Every link and path in both files resolves.
> - [ ] `git diff --check` is clean.
> - [ ] No new markdown file.
> - [ ] No Rust change, so no rebuild is needed (AGENTS.md, documentation-only changes).
>
> Effort: about 0.5 week [I].

---

# Group B: stage 0 work items

Each work item below is part of #S0. Performance numbers use the interleaved method: main/branch/main for at least 10
rounds, instructions retired and cycles from `/usr/bin/time -l` on macOS (`perf stat` on Linux), and bootstrap 95%
confidence intervals. A gate passes only if its interval clears the threshold. Until PR 0.2 lands, PR 0.1 uses the
existing `scripts/benchmarks.py compare` workloads with the same interleaving.

## #W0.1 · GC stage 0: phase timings, a GC log, pause keys in `(gc-stats)` and the deferral high-water counters

**Labels:** enhancement · **PR:** 0.1

> Nothing records where a pause goes, or how long a posted collection waits. `GcStats.last_pause_micros` is computed
> (`crates/patina-core/src/heap/gc.rs:1116`) and never read. Every later stage is judged by pause, MMU and K16
> numbers that do not exist yet.
>
> Scope (`crates/patina-core/src/heap/gc.rs`, `crates/patina-primitives/src/primitives/gc.rs`):
> - Each collection records:
>   - its reason: allocation threshold, `(gc)` or stress;
>   - phase times: roots, mark, weak fixpoint, sweep and `after_collection` (code release, provenance pruning);
>   - slots marked and swept per arena.
> - `PATINA_GC_LOG=<path>` writes one CSV line per collection: a monotonic start time, the reason, the phase times
>   and the counts. The format leaves room for the non-mutator intervals outside the pause that later stages add.
> - `(gc-stats)` gains `last-pause-us`, `pause-max-us` and `pause-total-us`.
> - **K16's two high-water marks**, each with its site (`#[track_caller]` on `GcDeferGuard::new`):
>   - work allocated between a posted collection and the safe point that runs it;
>   - work allocated inside one outermost `GcDeferGuard`.
>
>   They are measured in allocations until PR 1.1, then in bytes. Both are differences of the existing counter
>   taken at two points, so the allocation path gains no work.
>
> Acceptance:
> - [ ] No behaviour change. The lanes are byte-identical (release and debug-poison, both backends).
> - [ ] With logging off, the cost is ≤ 0.5% in instructions on the `compare` workloads, interleaved.
> - [ ] A test checks that the log has one line per collection, that the phase times add up to the pause within
>   rounding, and that the deferral counter reports a library load's site.
> - [ ] The CSV columns are documented in `docs/TEST_ORGANIZATION.md`.
>
> Effort: 0.5–1 week [I].

## #W0.2 · GC stage 0: a `gc` mode for the benchmark runner and the GC benchmark set

**Labels:** enhancement, performance · **PR:** 0.2 · **Depends on:** PR 0.1

> The repository's 41 benchmark cases barely exercise the collector. The longest runs 99 ms, and only 4 cases
> collect, at most 4 times each. No pause or MMU benchmark exists. The harness behind the research's measurements
> lived outside the repository.
>
> Scope:
> - A `gc` subcommand of `scripts/benchmarks.py` (reworked in #599), not a second runner, with a thin wrapper script
>   as for the two existing modes.
>   - ABA ordering over at least 10 rounds.
>   - Metrics per run: wall and user time, instructions retired, cycles, peak RSS, peak footprint and page reclaims
>     (`/usr/bin/time -l` on macOS; `perf stat` and `/usr/bin/time -v` on Linux); resident size and footprint after
>     a final `(gc)`; the PR 0.1 CSV.
>   - MMU at 1–100 ms windows from the pause timeline, and per-workload max pause. Pooled percentiles are never used.
>   - Bootstrap 95% CIs per workload ratio and on the geomean. Thresholds under 2% are judged in instructions or
>     cycles, never wall time.
>   - The mode's own tests join `scripts/tests/test_benchmarks.py`.
> - **The 20 measured workloads:** nboyer, deriv, gcbench, destruc, quicksort, gcold, mperm, queue3, fibfp, mbrot,
>   nucleic, ctak, fibc, generator, deeprec, libload, hashtable0, eqtable, dynamic and earley.
>   - The Larceny-derived ones are LGPL and are **not vendored**. They run from `~/Project/reference/larceny`, as
>     `run_larceny_tests.sh` does, and are skipped loudly when it is absent.
>   - Their scaled inputs and run-benchmark shims are Patina-authored and are vendored. Inputs are scaled so each run
>     takes at least 1 s.
>   - The fixnum twins of fibfp, mbrot and nucleic are derived from LGPL sources, so the PR vendors a script that
>     derives them from the checkout, not the programs themselves.
> - **Patina-authored probes**, vendored in `crates/patina-tests/bench_programs/gc/`:
>   - `samedepth1000`, `escape1000`, `pingpong1000`, `ctakdeep`, `abort100`;
>   - `frag-mix`, `ephem-chain-16k`, `port-churn`, `open-close-10k`, `retained-continuations`, `small-heap`;
>   - `many-heaps` (300 interpreters; a Rust harness in `patina-tests`);
>   - `deep-unwind`, `deep-descent`, `display-loop`, `parameterize-loop`;
>   - two loops that must poll every iteration under stress (an allocating `even?`/`odd?` tail recursion, and a
>     `call/cc` re-entry loop);
>   - `large-live` at 0.5 GiB and 1 GiB, sized in objects (≈ 40 M at 1 GiB in the headered layout). It is run on
>     demand, not by default.
>
>   `blocked-threads` waits for stage 9.
> - The six barrier programs (vecsort, hashtab, queue, tree, strport, letrec), the I/O workloads (Larceny cat, wc,
>   string, slatex, bibfreq and read1, from the checkout), and a tree-walker subset (fib 25, nboyer, deeprec at
>   200 K, libload, and a 200 K-deep recursion).
>
> Acceptance:
> - [ ] No behaviour change.
> - [ ] The mode reproduces the timing, RSS and pause rows of #S0's measurement table within their confidence
>   intervals. The results are posted on #S0.
> - [ ] A run without the Larceny checkout completes with the Larceny workloads reported as skipped.
> - [ ] The GBS list and the per-run metrics are documented in `docs/TEST_ORGANIZATION.md`.
>
> Effort: 1–1.5 weeks [I].
>
> *Drafting note (remove before filing): the research harness is `PRD/study/gc/probes/workload-demographics/instrumented/` (`REPRODUCE.sh`,
> `workloads/`); the barrier programs are in `PRD/study/gc/probes/barrier-remset/work/`.*

## #W0.3 · GC stage 0: vendor the memory-mapping and continuation probes under `scripts/gc_probes/`

**Labels:** enhancement · **PR:** 0.3

> The design's decommit mechanism, reservation rules and continuation targets rest on small probes run during the
> review. They belong in the repository, so the evidence stays reproducible.
>
> Scope: `scripts/gc_probes/`. Each file carries a header comment saying what it measures, how to build and run it,
> and the dated result on the development machine.
> - `madv.c`: 256 MiB touched in a 16 GiB `MAP_NORESERVE` mapping, then each way of giving it back.
> - `cycle.c`: an 8 MiB allocate-and-write cycle with and without decommit.
> - `decommit.c`: 464 MiB decommitted in 32 KiB pieces against 4 MiB runs.
> - `mapjit.c`: `MAP_JIT` regions with per-thread write protection.
> - `placement.c`: where anonymous maps land, and whether a 2⁴⁰ hint is honoured.
> - The continuation toy (`continuation_toy.rs`, a standalone `rustc` file, not a workspace member): today's
>   capture, design A and C′ at depths 100 and 1,000.
>
> Acceptance:
> - [ ] Each probe builds and runs with its documented command on macOS arm64. `madv.c`, `cycle.c` and `decommit.c`
>   also run on Linux.
> - [ ] The results are posted on #S0.
> - [ ] Nothing is added to the default build or to CI.
>
> Effort: about 0.25 week [I].
>
> *Drafting note: the sources are at `PRD/study/gc/probes/design-review/{madv,cycle,decommit,mapjit}.c` and
> `PRD/study/gc/probes/continuation-representation/`. The placement probe should be checked against
> `PRD/study/gc/probes/design-review/mm.c`.*

## #W0.4 · GC stage 0: a `gc-census` cargo feature for allocation, survival and store-mix measurements

**Labels:** enhancement · **PR:** 0.4

> Most of the design's measured claims come from an out-of-tree patch: 1,477 lines over 13 files in 5 crates. It
> covers the allocation mix and sizes, survival, the store mix, and live heaps per process. Later gates need the
> same tool in-tree: 4g's per-kind check, 5e's `Drop` census, survival for the bypass and K4, and the store mix for
> M2.
>
> Scope:
> - A `gc-census` feature in `patina-core`, with hooks in `patina-primitives` (records, parameters, promises,
>   equality), `patina-vm` (`control.rs`, `execution_state.rs`, `vm_state.rs`) and the tree-walker's continuation
>   code.
> - It records:
>   - the allocation census by kind and size, in today's slots and in the hypothetical headered layout;
>   - survival sampled at stress intervals from 16 K to 4 M allocations;
>   - store-mix counters: the immediate filter, the young-holder share, old→young edges and distinct targets per
>     interval;
>   - a heaps-per-process log.
> - Output is controlled by `PATINA_GC_CENSUS*` variables.
>
> Acceptance:
> - [ ] The default build is unchanged (`cfg`-gated; the lanes are byte-identical).
> - [ ] `cargo clippy --all-targets --all-features -- -D warnings` is clean with the feature on.
> - [ ] The census reproduces #S0's allocation, survival and store-mix rows within tolerance on the GBS. The results
>   are posted on #S0.
> - [ ] Usage is documented in `docs/TEST_ORGANIZATION.md`.
>
> Effort: 0.5–1 week [I].
>
> *Drafting note: the patch is `PRD/study/gc/probes/workload-demographics/instrumented/demographics.patch`.*

PR 0.5 is #A5 (the rebinding suite file and its rows). PR 0.6 is the #A6 comment on #597.

---

# Group B: stage 1 work items

PR 1.1 is #A7 (byte trigger) and PR 1.3 is #A4 (`EMFILE`).

## #W1.2 · GC stage 1: `(gc)` collects at its call, through `Step::Collect`

**Labels:** enhancement · **PR:** 1.2 · **Depends on:** PR 0.1

> Today `(gc)` only posts a request (`crates/patina-primitives/src/primitives/gc.rs:36-39`). The collection runs at
> the next per-instruction safe point. Stage 3 replaces that safe point with polls at frame entry and at `Transfer`
> returns. A `(gc)` that only posted would then let the next primitive in the same frame run first, and three #423
> tests would answer `#f` (`crates/patina-tests/tests/ephemerons.rs:94,109,127`). The same tests require one `(gc)`
> to break a dead key of any age (`:23,52,213,226`).
>
> Scope:
> - `Step` (`crates/patina-primitives/src/registry.rs:33`) gains `Collect(kind)`.
> - `gc` becomes a resumable primitive that answers `Step::Collect(Major)`. The machine delivers its result, then
>   collects before the caller's next instruction, with the caller suspended at the call's return pc.
>   - On the VM this goes through `resume_stub`. On the tree-walker it goes through `ResumePrimitive` at the
>     outermost trampoline.
>   - Inside a deferred region, the collection is posted and counted as a deferred poll by PR 0.1's counter.
> - The mechanism is what PR 1.3's `CollectAndRetry` reuses.
>
> Acceptance:
> - [ ] No behaviour change.
> - [ ] New tests, on both backends, break an ephemeron with a dead key immediately after `(gc)` in head position,
>   in tail position, as an argument, inside both `dynamic-wind` thunks and inside a `guard` handler.
> - [ ] All `ephemerons.rs` tests pass, as do the matrix and `escape_from_primitive.rs` on both backends.
> - [ ] The lanes are byte-identical.
> - [ ] The GBS geomean stays within ±1% in instructions.
>
> Effort: 1–1.5 weeks [I].

## #W1.4 · GC stage 1: delete the source-location stores nothing reads

**Labels:** performance · **PR:** 1.4 · **Depends on:** PR 0.2 (for the measurement)

> Loading 26 libraries peaks at 765 MiB of malloc, and 316 MiB of that is syntax provenance. Two of its stores have
> no production reader:
> - `SourceMap.locations` is written by the parser (`record`, `crates/patina-frontend/src/parser/mod.rs:289-294`).
>   It is read only by tests (`get`, `len`, `iter_locations`).
> - The heap's child spans (`Heap::child_source`, `crates/patina-core/src/heap/source.rs:42`) are read only by
>   tests.
>
> In addition, the desugarer builds a throwaway `SourceMap` for every macro expansion in a library body and drops it
> (`crates/patina-frontend/src/desugarer/mod.rs:2136-2149`), which is 355 MiB of churn. The freed-bits feed that
> keeps `locations` from misattributing a reused slot (`record_freed_bits`, `GcFreedBits`, `take_gc_freed_bits`)
> has no other consumer. It is also a sweep-time "visit the dead" hook, which the new heap cannot offer.
>
> Scope:
> - Delete the `locations` store and the parser's recording into it.
> - Delete the child-span store and the reader's recording into it (`parser/datum.rs:251,273`).
> - Stop creating the per-expansion `SourceMap`. The heap's `syntax_sources` stamping is kept, because the desugarer
>   reads it.
> - Delete the freed-bits feed.
> - Keep `SourceMap::get`, `len`, `iter_locations` and the public `prune_freed_locations` (re-exported at
>   `crates/patina-interpreter/src/lib.rs:52`) as deprecated no-ops that answer empty, under #601's convention.
>   Removal comes at stage 5e. The drivers stop calling `prune_freed_locations`.
> - Delete `source_map_entries_pruned_after_collection` (`crates/patina-tests/tests/interpreter_api.rs:270-292`),
>   which measures the deleted store.
> - Update `PRD/future/TREE_WALKER_HOOK_SYSTEM.md` §6 and `PRD/future/VISUAL_DEBUGGER_DESIGN.md`, which call
>   `prune_freed_locations` at form boundaries.
>
> Acceptance:
> - [ ] Error locations are unchanged: the `same.scm:2:3` caret test (`interpreter_api.rs:745-765`),
>   `crates/patina-repl/tests/diagnostics.rs` and the expansion-chain output.
> - [ ] The hygiene matrix is 139/139.
> - [ ] Both chibi scripts and the lanes pass.
>
> Measurement:
> - libload (26 libraries) malloc peak is at least 26 MiB lower, and churn at least 355 MiB lower (census or malloc
>   statistics).
> - libload load time is neutral or better in instructions, interleaved.
> - The GBS geomean is within ±1%.
>
> Effort: about 0.5 week [I].

---

# Group B: stage 2 work items

PR 2.2 is #A2 (teardown), PR 2.3 is #A1 (handles) and PR 2.4a is #A9 (`define-library` by binding).

## #W2.1 · GC stage 2: slot-based root visitor, open root registration, `CallFrame.closure` as a traced value

**Labels:** enhancement · **PR:** 2.1 · **Depends on:** PR 0.1

> Three things in the root contract block moving collection, handles and debugger roots:
> - The root visitor takes values by copy: `GcVisitor::visit(tv)` (`crates/patina-core/src/heap/gc.rs:485`).
> - Roots are a closed array built at each safe point, for example the tree-walker's
>   `collect(&[evaluator, &*registry, &gc_roots::EscapeRoots, &step_roots])`
>   (`crates/patina-tree-walker/src/eval/cps_eval/mod.rs:126-130`).
> - `CallFrame.closure` is a bare `Option<HeapIndex>` (`crates/patina-vm/src/types/mod.rs:50`) reached through
>   `visit_object_index` (`gc.rs:512`).
>
> Scope:
> - A `SlotVisitor` with `slot`, `slots`, `pinned`, `host` and `ephemeron`. Slots are `&Cell<TaggedValue>`, so
>   `trace_roots(&self)` keeps `&self`. The tree-walker's `Evaluator` is reached only through a shared reference.
> - `GcRoots` becomes `RootProvider`. `RootSet::register(Box<dyn RootProvider>)` returns a `RootToken` holding a
>   `Weak` to the set, and replaces the closed array.
> - Every implementation is ported: `VmState`, `LibraryRegistry`, `StepTracer`, and the tree-walker's `Evaluator`,
>   `EscapeRoots` and `StepRoots`. The tree-walker reports through `pinned`, which is sound because its heaps never
>   move.
> - `CallFrame.closure` becomes a `TaggedValue` visited as a slot, and `visit_object_index` is deleted.
> - The `PENDING_ESCAPE` thread-local (`crates/patina-tree-walker/src/eval/cps_eval/types.rs:21`) becomes an
>   evaluator field.
>
> Acceptance:
> - [ ] No behaviour change. The lanes are byte-identical (off, default and stress, release and debug-poison, both
>   backends).
> - [ ] A unit test shows that a registered root is traced, that dropping its token unregisters it, and that a test
>   visitor can rewrite a slot and restore it.
> - [ ] Both chibi scripts pass.
>
> Measurement:
> - Mark time per live pair and per live closure (PR 0.1's phase times on the GBS) stays within ±1%.
> - The GBS geomean stays within ±1%.
>
> Docs:
> - `docs/VM_DECISIONS.md` §4 and §5: the closure as a traced value, and the root inventory.
> - `PRD/future/TREE_WALKER_HOOK_SYSTEM.md` §5.2 and `PRD/future/VISUAL_DEBUGGER_DESIGN.md`:
>   `DebugHook: GcRoots` becomes `DebugHook: RootProvider`.
>
> Effort: about 1.5 weeks [I].

## #W2.4 · GC stage 2: process bare top-level imports outside the desugarer, recognized by the binding of `import`

**Labels:** enhancement · **PR:** 2.4 · **Depends on:** PR 2.4a (#A9), for the shared binding check

> Every program-level `(import …)` runs inside `desugar_with_imports`, under its `GcDeferGuard`
> (`crates/patina-frontend/src/desugarer/mod.rs:1841`). The bodies of the libraries it loads therefore cannot
> collect, whatever the loader does (#W2.5).
>
> Scope:
> - In `eval_datum` on both backends (`crates/patina-vm/src/backend.rs`, `crates/patina-tree-walker/src/backend.rs`),
>   a bare top-level `(import …)` is recognized **by the binding of `import`**: the core-syntax marker
>   (`CoreForm::Import`) that the environment binds to that name. It is never recognized by its spelling (AGENTS.md).
> - `is_define_library_form` matches spellings (#A9). This uses #A9's helper instead.
> - The import sets are processed outside the desugarer, with only the set list rooted.
> - Imports inside a `begin`, a REPL line, `eval` and `load` keep today's paths and policy
>   (`docs/TEST_ORGANIZATION.md`, "Import modifier policy and context coverage").
>
> Acceptance:
> - [ ] No behaviour change.
> - [ ] `import_modifiers.rs` passes in every context.
> - [ ] `vm_global_cache.rs` passes.
> - [ ] #435's `(begin (import …))` behaves the same on both backends.
> - [ ] A program that shadows `import` with its own binding is not hoisted and behaves as today.
> - [ ] The hygiene matrix is 139/139.
> - [ ] Both chibi scripts and the lanes pass.
>
> Measurement:
> - Startup and libload are neutral in instructions, interleaved.
>
> Effort: 0.5–1 week [I].

## #W2.5 · GC stage 2: library bodies loaded from top-level imports collect between and inside their forms

**Labels:** performance · **PR:** 2.5 · **Depends on:** PR 2.1, PR 2.4; PR 1.1 for byte-denominated K16 counters

> Library loading never collects while it runs:
> - `ParsedLibrary` carries a `GcDeferGuard` for its whole life (`crates/patina-runtime/src/library_loader.rs:208`).
> - `with_globals` defers too (`crates/patina-vm/src/runtime/vm_state.rs:343`).
> - So does the desugarer (#W2.4).
>
> `(import (nieper rbtree))` peaks at 200 MB RSS on the VM, against 11.7 MB for an empty program, with 1 collection
> in the whole run. This was measured on 2026-10-01 at `28a94f8`. An inlined copy of the same library went from a
> 211 MiB to a 118 MiB malloc peak when collection was allowed between forms. This replaces GC_STAGE5_PRD's
> "Priority 2 — nested-loop collection" for loading. Nested VM loops wait for stage 4e.
>
> Scope (VM):
> - **Point A** (between a library body's forms) and **point B** (between libraries) become collection points.
>   - The registry's `Loading` entry holds the parsed body as one heap list, plus the in-progress library
>     environment, reported through a registered root provider (PR 2.1).
>   - `with_globals`' saved environment moves onto a traced `globals_stack`.
>   - `ParsedLibrary` stops holding a `GcDeferGuard`.
> - **Point D.** A body loaded from a hoisted import (#W2.4) runs in the outermost VM loop, so it also collects
>   inside its forms.
> - **Still deferred, and renamed `NoGcScope` at each site:**
>   - nested VM loops (`across_reentry`, `run_apply_proc`, and loads requested by running code such as
>     `environment`), until stage 4e deletes the weak continuation tables whose soundness rests on "nested loops
>     defer" (`crates/patina-vm/src/runtime/vm_state/gc_roots.rs:21-28`);
>   - point C (an import met mid-form, and `include`);
>   - nested tree-walker trampolines.
> - **The tree-walker** keeps deferring during loads. That follows decision 1: its library bodies do not collect
>   between forms.
>
> Acceptance:
> - [ ] `(import (nieper rbtree))` peaks at or below 130 MB RSS on the VM (200 MB today), with collections during the
>   load. PR 0.1's log shows the reason and site.
> - [ ] K16's deferral high-water for libload is reported before and after.
> - [ ] The lanes are byte-identical in release and debug-poison, including `patina-compat check-smoke` run under
>   `PATINA_GC_STRESS` in the debug-poison build.
> - [ ] `library-bindings.scm`, `import_modifiers.rs`, the library-availability tests and
>   `finished_forms_release_code.rs` pass.
> - [ ] Both Larceny lanes are unchanged, and both chibi scripts pass.
>
> Measurement:
> - libload is neutral or better in instructions.
> - The GBS geomean stays within ±1%.
> - libload peak RSS is reported.
>
> Docs:
> - `PRD/future/GC_STAGE5_PRD.md` Priority 2 points here, if #D has not yet replaced the file.
> - `docs/TEST_ORGANIZATION.md` (the stress `check-smoke` run).
>
> Effort: 1–2 weeks [I].

---

# Later stages: issues filed at stage 0 as stubs

The design asks stage 0 to file one issue per stage. Each body is that stage's row of the design's Appendix D, with
the Appendix E and H blocks named for it. When the stage starts, its PRs get their own issues, as stages 0–2 do here.

| Stage | Title | Body from | Effort [I] |
|---|---|---|---|
| S3 | GC stage 3: `Mutator`, the collect capability, `Cx` and the store funnel; polls at frame entry; `InterruptHandle`; `Interpreter::call` and host primitives | Appendix D row 3; E.3; E.5 (capability, codemod order) | 10–13 |
| S4a | GC stage 4a: canonical identity and ports (closes #A3) | D 4a; F.3 port tests | 4–5 |
| S4b | GC stage 4b: global cells and binding records (variant R) | D 4b; §8.6; E.2 | 3–4 |
| S4c | GC stage 4c: identifiers as ids, inline provenance | D 4c | 3–5 |
| S4d | GC stage 4d: frames and stacks; the stack cap and its knob | D 4d; E.5 (code liveness, map memory) | 4–5 |
| S4e | GC stage 4e: continuations as heap objects; traced code liveness | D 4e; H.5 (nested loops) | 5–7 |
| S4f | GC stage 4f: tree-walker host payloads | D 4f | 2–3 |
| S4g | GC stage 4g: inline payloads | D 4g; E.5 (`Drop` census) | 2–3 |
| S5 | GC stage 5: the new heap, non-moving and whole-heap, kind by kind (closes #A8 if not fixed earlier) | D 5; E.4–E.6; H.2; H.3 | 16–21 |
| S6 | GC stage 6: JIT ABI spike and freeze | D 6; H.1 | 3–5 |
| S7 | GC stage 7: sticky generations behind a switch | D 7; H.2; H.3 | 7–10 |
| S8 | GC stage 8: opportunistic evacuation | D 8 | 6–9 |
| S9 | GC stage 9: threads readiness (the `GreenThread` split; SRFI 18 objects; deep-bound `parameterize`) | D 9; E.7 via #ST | 7–10 |
| SP | GC stage P: parallel stop-the-world marking (only if `large-live` exceeds 100 ms after stage 5) | D P; H.4 | 4–6 |

---

# Appendix: verification on 2026-10-01

Everything was run at `main` `28a94f8`, with a clean tree. The repository was not modified.

- **Release binary.** Built with `CARGO_TARGET_DIR=<a directory outside the repository> cargo build --release
  --bin patina`, run with `PATINA_LIBRARY_PATH=lib` from a working directory outside the repository.
- **Embedder probes.** A crate at `PRD/study/gc/probes/design-verify/repro`, with `patina-interpreter` as a path dependency and
  the repository's `rust-toolchain.toml` and `Cargo.lock`, built in debug and release. It has three binaries:
  `uaf` (#A1), `resilient` (#A1) and `teardown` (#A2).
- **Oracles.** chibi-scheme 0.12.0 "magnesium" and Gauche 0.9.15 (`gosh -r7`), always under `timeout -s KILL`.

| Issue | Command (abbreviated) | Result |
|---|---|---|
| #A1 | `repro/target/{debug,release}/uaf [tw]` | debug: panic `use-after-free: pair slot 15992` on both backends; release: `(45264 . 45264)` on both |
| #A1 | `repro/target/{debug,release}/resilient` | debug: panic `pair slot 16012`; release: `(45290 . 45290)` |
| #A2 | `repro/target/release/teardown <file> [tw]` | `heap alive = true, strong = 36` on both backends; file 0 bytes after exit |
| #A3 | `patina [--tree-walker] eq.scm`; `chibi-scheme eq.scm`; `gosh -r7 eq.scm` | `(#f #f #t #f)` ×2; `(#t #t #t #t)` ×2 |
| #A4 | `sh -c 'ulimit -n 1024; exec …' emfile.scm` | `(failed-at 1021 #t)` ×2; `(ok 100000)` ×2 |
| #A5 | `p1.scm`, `q2.scm` on all four | p1: `1 mine` ×2, `1 1` ×2; q2: `1 (2)` ×2, `1 1` ×2 (chibi warns) |
| #A6 | `grep`/`sed` over AGENTS.md, `docs/`, `PRD/`, `gc.rs`, `control_flow_matrix.rs`; `ls ~/Project/r7rs-benchmarks` | each item as quoted; the directory does not exist |
| #A7 | `/usr/bin/time -l` on `bigvec.scm` and `captures.scm` | 414 MB/0 collections ×2; VM 3.2 GB/1, tree-walker 46 MB/1; chibi 17 MB and 8.8 MB; Gauche 44 MB and 35 MB |
| #A8 | `patina [--tree-walker] ephem-chain.scm <n> forward\|reverse` | the table in #A8 |
| #A9 | `deflib2.scm` on all four | Patina exit 1 with a library parse error ×2; chibi and Gauche `(called (1 2))` `end` |
| #W2.5 | `/usr/bin/time -l patina [--tree-walker] rbtree.scm` | 200 MB (VM) and 202 MB (tree-walker) peak RSS, 1 collection; empty program 11.7 MB |

Source references cited in the drafts were checked at `28a94f8`:
- `heap/mod.rs:581-584` (`note_alloc`), `gc.rs:25`, `gc.rs:167` and `:1116` (`last_pause_micros`, written and never
  read outside `gc.rs`), `gc.rs:177-196` (`GcRoots`), `gc.rs:485` and `:512`.
- `types/mod.rs:50` (`CallFrame.closure`), `cps_eval/mod.rs:126-130`, `cps_eval/types.rs:21`.
- `ports.rs:444,463,482`, `registry.rs:33` (`Step`), `library_support.rs:61-70`, `vm backend.rs:238`,
  `tree-walker backend.rs:114`.
- `desugarer/mod.rs:1841` and `:2136-2149`, `library_loader.rs:208`, `vm_state.rs:343`, `interpreter lib.rs:52`
  and `:491-517`.
- `interpreter_api.rs:270-292`, `run_gc_differential.sh:38-40,67,139-175`, `common/mod.rs:644,706,844`.
- `environment.rs:489`, `compiled_macro.rs:483`, `exit_status.rs:63`.
- The six pinned rebinding tests, `control_flow_matrix.rs:309-328` (64 shapes) and `HeapObjectData` (28 variants).

One claim in the design was corrected while drafting. Appendix H.5 cites `is_define_library_form` as a
binding-keyed precedent for import hoisting, but it matches spellings. That mismatch led to #A9, and #W2.4 is written
to use #A9's binding check instead.
