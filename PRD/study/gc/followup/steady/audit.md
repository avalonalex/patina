# Steady-state audit: every source of unbounded growth, today and in the redesign

Date 2026-10-01. Repository `main` at `28a94f8`, untouched (release build in
a target directory outside the repository). Machine: Apple M4 Pro, 24 GiB, macOS 27.2 arm64, 16 KiB pages. Oracles: chibi-scheme
0.12.0, Gauche 0.9.15 (`gosh -r7`), Chez Scheme 10.3.0. Labels: **[P]** measured here, **[S]** read in source (path:line
at `28a94f8`, or the oracle's source under `~/Project/reference`), **[I]** judgement. "DESIGN" is
`PRD/study/gc/design/DESIGN.md`.

The probes are retained in `PRD/study/gc/probes/followup/steady/` (`probes/*.scm`, `chez/*.ss`, `run.py`); the raw results
(`results/*.jsonl`) were not. Appendix A says how to re-run them.

---

## 0. Summary

1. **Steady state is a property that can be tested, and Patina fails it today in eight measured ways.** The proposed
   definition (§1): a program whose live data and *name set* stay bounded must reach a footprint and a pause
   behaviour that stop growing, and no operation may get slower with history. The probes run each cycle N and 4N
   times and compare. Four classes of failure were found: true leaks (A), plateaus inflated by a blind trigger (B),
   memory and pause high-water marks that are never given back (C), and per-operation time that grows with history (E).
2. **True leaks today (class A), per cycle [P]:** `string->symbol` churn 215 B/symbol (chibi 165, Gauche 186, **Chez
   0**: it prunes its oblist); a library macro with a private helper (`guard` → `%guard-aux`) **1.8–2.3 KiB per top-level
   `guard` form**, in file, stdin and REPL alike, 3 interned alias symbols each; re-`eval` of a quoted datum that carries
   source provenance **95–118 KiB per eval for `case`, 52 KiB for `guard`**, with quadratic time (20 K evals: 2.2 GiB,
   168 s); top-level library redefinition **49 KiB per redefinition** (32 K: 1.59 GiB); every dropped embedded
   interpreter **3.6 MiB**; macro-introduced hidden top-level definitions 245 B per expansion (all three oracles leak
   here too: chibi 928, Gauche 523, Chez 222); unbound names in discarded code 457 B/name (oracles 288–356).
3. **Blind-trigger plateaus (class B) [P]:** a loop of `(environment '(scheme base))` plateaus at **~970 MiB** (VM) and
   ~930 MiB (tree-walker) where chibi stays at 8.7 MiB, Gauche at 31.5 and Chez at 49.4; continuation churn at depth 100
   plateaus at 563 MiB (VM) and passes 1.96 GiB on the tree-walker (chibi 8.7, Gauche 35, Chez 49). Bounded, but by
   *trigger interval × hidden bytes per object*, not by live data.
4. **High-water retention (class C) [P]:** after a 2 M-element peak is dropped, RSS stays at 210 MiB and **every later
   `(gc)` pause stays at 9.3 ms against 0.095 ms before the peak** (sweep over the arena high-water mark). After a
   1 M-frame recursion the VM keeps 143 MiB, the tree-walker 984 MiB. No oracle returns heap memory after a peak either
   (Gauche 143, Chez 151, chibi 574 MiB); Chez alone gives back stack (43 MiB after 1 M frames).
5. **What is already steady [P]:** redefining the same procedure through `eval` (18.5 MiB flat to 200 K), compiling and
   discarding lambdas (19.1 MiB at 800 K), record-type redefinition, string-port churn, file-port open/close, repeated
   `load`, repeated `import`, 100 K-form stdin and REPL streams of redefinitions, and 30 s of bounded-live allocation
   (45 MiB flat over 34.8 M iterations). The #338/#352 code-release work holds.
6. **The redesign fixes classes B and C and most of E, but as specified it turns several class-A leaks permanent and
   adds new ones.** Its immortal space is append-only and never swept (DESIGN §4), and it holds global cells, binding
   records and interned symbols. Under variant R every name in *every* namespace, environment specifiers included,
   placeholders and aliases included, gets an immortal record (DESIGN §8.6). So each `(environment '(scheme base))`
   would leak about 260 records (≈8 KiB) for good where today it is merely bloated; each unbound reference, each alias
   and each introduced definition leaks a record and a cell; and because cells are a root region scanned at every major
   (DESIGN §4, §6.1, §6.10 step 9), **major pauses grow with that history**, which breaks decision 6's "no pause term
   grows with heap size" in substance. Unaddressed new tables: the stage 4c scope-set intern table and per-document
   location tables, the descriptor space (append-only, hole reuse unspecified), JIT code memory (allocator
   unspecified), the `CoreExpr` literal pool, per-thread stack reservations (one VMA and ≥ 1 committed page per started
   green thread; Linux `vm.max_map_count` caps them near 65 K), and the in-memory MMU log.
7. **Recommendation:** adopt five steady-state rules (§4.1), the central one being that **the immortal space admits only
   objects bounded by the binary and the program text** (primitives, their descriptors, canonical flonum boxes, core
   syntax, a future boot image). Symbols, cells and binding records become non-moving but *mortal*, swept at majors
   and weakly interned or owned by their namespace. Add a steady-state lane (§5) with the probes of this audit, run from
   stage 0, and tie the owner's limits (`max_heap`, `--stack-max`) to it (§6): a program with bounded live data and
   names never reaches a limit, and one that leaks reaches a catchable `&heap-exhausted` that names what grew.

---

## 1. The concept

### 1.1 Definition

A program runs in **steady state** over an interval when (a) its live data is bounded, L(t) ≤ L_max, and (b) the set of
*names* it keeps is bounded: reachable interned symbols, top-level bindings it can still refer to, loaded libraries,
started and unterminated threads, open ports. Program text is a constant. A cycle is one iteration of whatever the
program repeats (a server request, a REPL command, an `eval` of generated code, a generator step).

Patina is **steady-state safe** when every such program satisfies four properties:

| # | Property | Measured as |
|---|---|---|
| SS1 | **Footprint plateaus.** Committed memory (resident size and `phys_footprint`) is at most F·L_max + C, with F the pacing factor (about 3 for whole-heap `2·L`, plus fragmentation) and C a constant (binary, boot image, the program's own code, reserves). After a peak passes, footprint returns to that bound within a stated number of majors. | RSS(4N) − RSS(N) per cycle; RSS over the last half of a timed run; RSS after a dropped peak |
| SS2 | **Pauses plateau.** Every pause is within the budget form of decision 6 in terms of L_max and stack depth. No term grows with run length, number of evals, number of collections, dead objects or past peaks. | per-workload max pause and MMU(10 ms) in the second half of a run equal to the first half; pause after a dropped peak equal to the pause before it |
| SS3 | **Per-operation cost plateaus.** `eval`, `define`, `string->symbol`, macro expansion, import, port open and thread start do not get slower with the number of earlier operations. | time(4N)/time(N) ≤ 4.4 |
| SS4 | **Every off-heap table has an owner.** A Rust-side table keyed by name, id or address is bounded by program text, or its entries die with a heap object (finalization or the weak epilogue), and its bytes are charged to the trigger as external bytes while they live. | code inspection; the `gc-census` feature; `(gc-stats)` keys per table |

### 1.2 Classes of failure

| Class | Name | Shape | Example today |
|---|---|---|---|
| A | true leak | grows by a fixed amount per cycle, for ever | `string->symbol` churn: 215 B per symbol |
| B | blind-trigger plateau | bounded, but the bound is trigger interval × bytes the trigger cannot see | `environment` churn: 970 MiB plateau |
| C | high-water retention | memory or pause cost reached at a peak is never given back | 9.3 ms pauses after a 2 M-element list is dropped |
| D | live growth | the program keeps a growing name set; every implementation grows; only the *cost per name* is Patina's business | fresh top-level names: 365 B each (oracles 400–484) |
| E | time growth | per-operation cost grows with history | hidden top-level definitions: 15× the time for 4× the expansions |

Class D is outside the contract by definition, but it belongs in the lane as a cost regression check, and the
redesign changes its constant (§3.1). The other four are defects.

### 1.3 Relation to the owner's limits

`max_heap` and `--stack-max` are only meaningful together with steady state. With SS1–SS4 a program with bounded live
data never reaches either limit, so the limits catch exactly the programs that really grow, and the condition they
raise can say *what* grew. Without SS4 the limits are bypassed: today's leaks are almost all in Rust tables
(namespaces, provenance, alias tables, library environments) that a heap limit over the reservation would never see,
so a leaking server would be killed by the OS instead of raising `&heap-exhausted`. §6 turns this into rules.

---

## 2. Measurements of today's code

Each probe runs a cycle N and 4N times (some also 16N) as a separate process under `/usr/bin/time -l`. Slope is
(max RSS at the larger N − max RSS at the smaller N) / ΔN. The Patina runs also print `(gc-stats)` (collections,
arena sizes, `symbols`). Same release build for both Patina backends.

### 2.1 Class A: true leaks

| Probe (cycle) | Patina VM | Patina TW | chibi | Gauche | Chez | Notes |
|---|---|---|---|---|---|---|
| `sym-churn`: `(string->symbol (string-append "sym-churn-" (number->string i)))`, discarded | **215 B/sym** (1 M: 223 MiB) | 405 | 165 | 186 | **0** (49.4 MiB flat at 1 M) | Patina's `symbols` count rises 1:1 [P]; Chez prunes symbols with no value or property list [S `ChezScheme/c/gc.c:1294-1297,1580-1615`]; Gauche's obtable is a strong string table [S `Gauche/src/symbol.c:422`]; chibi's symbol table is a strong bucket vector in the context globals [S `chibi-scheme/sexp.c:550`] |
| `unbound-ref`: `(eval '(lambda () unbound-i))`, never called, discarded | 457 B/name | 331 | 288 | 356 | 303 | Patina retains only the symbol, plus the pacing headroom that live symbols buy (`2 × live`); every oracle retains something too |
| `hidden-define`: re-expand `(def-counter counter)`, whose template defines a hidden `state` | 245 B/exp; **time ×15 for 4× N** (25 K: 5 s; 100 K: 76 s) | 274 B; ×10 | 928 B; ×16.5 | 523 B; ×3.9 | 222 B; ×13 | all keep each expansion's renamed binding; Patina's time is a linear scan per reference (§3.4) |
| `guard` at top level, 25 K vs 100 K forms, as a file / stdin stream / REPL `-i` | **1.8–2.3 KiB/form** (file 70 → 237 MiB: 2.3; stdin 63 → 222: 2.2; REPL 56 → 190: 1.8) | — | n/a | n/a | n/a | 3 interned alias symbols per expansion (`symbols` 3,006 after 1,000 evals, 9,006 after 3,000) [P]; `%guard-aux` is a library-private macro, so each expansion makes fresh `name.N` aliases (§3.5) |
| `macro-eval case`: `(eval '(case 3 ((1 2) 'a) ((3) 'b) (else 'c)) env)` | **95–118 KiB/eval**; 2.5 K: 237 MiB / 3 s; 10 K: 949 MiB / 42 s; 20 K: 2.2 GiB / 168 s | — | — | — | — | arenas flat (45 K pairs, 22.6 K objects): all off-heap. The same form read from a string at run time (no provenance) is flat [P]; the profile is in `stamp_expansion_source` copying `String`s (§3.7) |
| `macro-eval guard` (quoted constant, eval) | 52 KiB/eval (80 K: 3.27 GiB, 72 s) | — | — | — | — | provenance growth plus the alias leak |
| top-level `define-library (tmp steady)` + `import` + call, repeated | **49 KiB/redefinition** (2 K: 130 MiB; 8 K: 410; 32 K: 1.59 GiB; 19 collections) | — | n/a (no library definition in a script) | — | — | without the `import`, 8 K redefinitions reach 545 MiB with **one** collection: `define-library` forms never reach a safe point |
| embedding: `VmInterpreter::new_vm()`, one small program, `drop` | **3.6 MiB per interpreter** (400: 1.45 GiB, linear) | — | — | — | — | the known `Rc` cycle (heap → `VmClosure.globals` → `Environment.heap`) |
| `record-redefine`: re-`eval` the same `define-record-type` | 0 | — | **848 B/redef** | error in `gosh -r7` | 0 | Patina is steady here and chibi is not |

### 2.2 Class B: plateaus set by a blind trigger

| Probe | Patina VM | Patina TW | chibi | Gauche | Chez |
|---|---|---|---|---|---|
| `env-churn`: `(environment '(scheme base))` + one `eval`, discarded | 10 K: 451 MiB; **40 K and 160 K: 972 MiB** (7 collections in 160 K) | 10 K: 459; 40 K: 932 | 8.7 flat | 31.5 flat | 49.4 flat |
| `env-lambda`: the same plus `(eval '(lambda (x) (* x 2)) e)` called once | plateau **516 MiB** (40 K–160 K) | 10 K: 388 | 8.7 | 31.5 | — |
| `cont-churn`: `call/cc` at depth 100, discarded | plateau **563 MiB** (80 K–320 K, 10 collections) | 5 K: 512 MiB; 20 K: **1.96 GiB** (1 collection); depth 10: 393 → 421 MiB | 8.7 | 35.2 | 49.4 |
| `file-port-churn`: open, write, close; open, read, close | plateau 30 MiB (from 17.8) | — | 8.7 | 39.9 | — |

The trigger counts objects (`max(65,536, 2 × live slots)`); an environment specifier is one heap object that owns
about 260 bindings of Rust memory, and a continuation is one object that owns a copied register file.

### 2.3 Class C: high-water retention

| Probe | Patina VM | Patina TW | chibi | Gauche | Chez |
|---|---|---|---|---|---|
| `peak-then-drop`: build a 2 M-element list of 2-vectors, drop it, then loop with tiny live data | **210 MiB kept** for the rest of the run (peak 279) | 214 MiB kept | 574 MiB kept | 143 kept | 151 kept |
| the same, `(gc)` pause | **0.095 ms before the peak; 13.5 ms at the peak; 9.3 ms after the drop, for ever** (2,000,011 pair slots, 1,999,963 free) | — | — | — | — |
| `deep-then-steady`: 1 M-deep non-tail recursion, return, then loop | **143 MiB kept** | **984 MiB kept** | "out of stack space" at 1 M | 136 kept | **43 MiB: returned** |
| the same at 100 K frames | 26 MiB | — | 17 | 56 | 30 |

### 2.4 Steady today

| Probe | Patina VM | Patina TW | Oracles |
|---|---|---|---|
| `eval-redefine`: `(eval '(define (f x) …))` and call, same name | 18.3 → 18.5 MiB (50 K → 200 K) | 21.9 → 20.7 | flat in all three |
| `eval-lambda`: compile, call, discard | 17.6 → 19.1 MiB at 800 K (1,633 collections) | flat | flat |
| standard macros through `eval` (`let`, `let-values`, `do`, `case-lambda`, `parameterize`, `delay`, `define-record-type`) | flat (≤ 20 B/eval) | — | — |
| `define-values` of two names | flat | — | flat |
| `port-churn`: string ports | flat | flat | flat |
| `load-repeat`: `(load "loaded.scm")` of 6 definitions | 19.1 → 20.6 MiB (2 K → 32 K) | — | chibi flat |
| `reimport`: `(eval '(import (scheme char) (scheme list)))` | flat at about 31 MiB to 6,400 | — | — |
| 100 K-form streams of redefinitions on stdin and in the REPL (`-i`) | stdin 42.9 → 44.1; REPL 36.7 → 38.9 MiB | — | — |
| `steady-alloc`: a 20 K-slot ring of lists, strings and vectors, 30 s | **45 MiB flat** (34.8 M iterations) | 119 flat | chibi 35, Gauche 48, Chez 71, all flat |
| `thread-churn` (chibi only; Patina has no SRFI 18) | — | — | chibi flat at 8.7 MiB for 80 K threads |

`thread-blocked` (chibi): 32 KiB retained per thread blocked for ever, as decision 23 expects.

---

## 3. Source-by-source audit

Each entry gives what grows, the behaviour that grows it, today's code and measurement, what the oracles do, what the
redesign specifies, and the fix. The verdict column of the table in §3.0 uses the classes of §1.2.

### 3.0 Overview

| # | Source | Today | Redesign as specified | Fix and stage |
|---|---|---|---|---|
| 1 | Symbol table | A: 215 B/symbol | A: ≈48 B/symbol, immortal (decision 11) | owner decision: weak symbols (Chez) or strong (chibi, Gauche); 5c |
| 2 | Global bindings (fresh names) | D: 365 B/name | D: symbol + cell + record, immortal, about 100 B + map entry | cost check only |
| 3 | Placeholder records for unbound names | A: the symbol only | **A, worse**: a record and a cell per name, immortal | weak placeholders; 4b |
| 4 | Macro-introduced top-level definitions | A + **E (quadratic)** | A: records and cells immortal; E not addressed | index by scope set; reclaim with code; issue now, 4b |
| 5 | Per-expansion alias bindings (`name.N`) | **A: 1.8–2.3 KiB per `guard`** | A: an immortal record per alias | bind template references to records, no alias names; issue now, 4b |
| 6 | Environment specifiers | **B: 970 MiB** | **A**: ≈260 immortal records per `environment` call | namespaces are mortal host payloads; stage 1 charge; 4b |
| 7 | Provenance and expansion chains | **A: 95–118 KiB per eval**, quadratic | stage 1 deletes `locations`; 4c tables have no lifetime rule | fix chains now; 4c tables owned and compacted |
| 8 | Scope sets | none (inline) | **new A**: 4c intern table | weak or per-expansion-epoch table; 4c |
| 9 | Code store | steady | descriptor space hole reuse unspecified | mark-region descriptor space; 5 |
| 10 | JIT code memory | — | allocator unspecified, never moved | size-segregated allocator, fragmentation gate; 6 |
| 11 | `CoreExpr` literal pool | — | **new**: a "traced table" with no lifetime | per-compilation scope; 3 |
| 12 | Library registry and redefinition | **A: 49 KiB per redefinition**; no safe point | library is a host payload, but its cells are immortal | old namespace mortal; safe point between `define-library` forms; issue now |
| 13 | Ports | steady (30 MiB plateau) | steady (`PortTable`, finalization, R3) | process-wide weak registry must prune; 4a |
| 14 | Continuations | **B: 563 MiB VM, ≥ 2 GiB TW** | fixed (heap objects, byte-charged) | 1, 4e |
| 15 | Tree-walker environments | B | fixed by host-payload charging | 4f |
| 16 | Register stacks | **C: 143 MiB VM, 984 TW** | fixed by decommit (2 observations) | 4d; but see #26 |
| 17 | Heap arenas and sweep | **C: memory and a 100× pause** | fixed (blocks, lazy sweep, decommit) except when no major runs after the peak | post-peak trim at safe regions; 5e |
| 18 | Immortal space in general | — | append-only by definition | admit only program-text-bounded objects (rule R-SS1) |
| 19 | Descriptor space | — | append into blocks; holes unspecified | sweep and reuse holes; 5c |
| 20 | Global cells as a root region | — | **pause grows with cell history** | follows from #2–#6 and R-SS1 |
| 21 | Store buffers, mark stack, LOS cache | — | bounded (soft limit, segment cap, 32 MiB cache) | state segment-pool trimming; 5a, 7 |
| 22 | Pacing ratchet and decommit | object trigger | `×1.5` livelock boost has no decay | decay the boost; 5e |
| 23 | GC observability | small | MMU "from every non-mutator interval" | streaming MMU in fixed memory; 0 |
| 24 | Interpreter teardown | **A: 3.6 MiB per interpreter** | fixed at stage 2 | process-wide registries prune dead heaps; 2 |
| 25 | Threads (SRFI 18) | — | entries freed at termination; blocked-forever rooted | as designed (decision 23) |
| 26 | Green-thread stacks | — | **one reservation (VMA) and ≥ 1 committed page per started thread; 4 MiB decommit runs** | sub-allocate stacks in one per-heap arena; page-granular trim; 9 |
| 27 | REPL and stdin streaming | steady | unchanged | keep the compaction; extend it to 4c location tables |
| 28 | Counters and ids | no exhaustion | 48-bit scope-set and source ids | none |
| 29 | Small per-heap tables (`code_store`, free lists, `shadowed_primitives`, `OUTPUT_FILES`, `gc_freed_bits`) | bounded by peak | slot tables with free lists | accept 8–16 B per peak slot, or shrink |

### 3.1 Symbol table (#1) and fresh global names (#2)

- **What grows.** `Heap.symbol_table: HashMap<String, HeapIndex>` (`crates/patina-core/src/heap/mod.rs:319` [S]);
  `intern_symbol` inserts and nothing removes (`:981-990` [S]); collection re-marks every entry
  (`crates/patina-core/src/heap/gc.rs:449-465` [S]). Each symbol costs a 72 B `objects` slot, an `Rc<str>`, a `String`
  key and a hash-table entry: **215 B** for a 16-character name [P], 405 B on the tree-walker.
- **Behaviour.** `string->symbol` on data (parsers, JSON keys, `read` of untrusted input), and every alias of §3.5.
  Source identifiers are `Identifier` objects, not symbols, so program text adds almost nothing (`symbols` is 1–35 in
  every probe that does not churn [P]).
- **Oracles.** chibi and Gauche keep every symbol (165 and 186 B/symbol [P]; strong tables [S] as cited in §2.1). Chez
  reclaims a symbol that has no top-level value and no property list, during collection (`ChezScheme/c/gc.c:1294-1297`
  keeps only symbols with a value or property list; `:1580-1615` prunes the rest from the oblist [S]); 1 M churned
  symbols leave its RSS at 49.4 MiB [P]. Racket and Guile also intern weakly [I].
- **Redesign.** Symbols go to the immortal space (`16+len`, DESIGN §3 table and §4) behind "an interner over immortal
  symbols" (DESIGN E.1), and decision 11 keeps the table strong ("a weak symbol table adds one epilogue step"). The
  leak survives at about 32–48 B per symbol: smaller, still class A.
- **Fix.** An owner decision, now with numbers:
  - *Strong* (decision 11 as written): matches chibi and Gauche; a parser server leaks about 48 B per distinct key for
    ever; JIT code may embed any symbol address.
  - *Weak* (recommended): symbols live in a non-moving **symbol space** swept at majors; the interner is pruned in
    epilogue step 6 of symbols that are unmarked. A symbol stays alive while anything references it, including code
    constants, binding records (their name field) and identifiers, so Chez's "value or property list" rule falls out
    of ordinary tracing. Two consequences need rules: (1) the JIT embedding rule of DESIGN §8.2 gains a fourth case,
    "an object referenced from the body's own unit (constants or link table)", which is already sound because the
    unit outlives the body; (2) ephemeron keys: DESIGN §6.6 makes symbol keys always live. Keep that observable answer
    by treating a symbol key as strong (the ephemeron keeps its symbol alive), so a symbol-keyed weak table never loses
    an entry, as today and as in chibi and Gauche, while symbols that nothing holds are reclaimed. Identity hashes of
    symbols are stored in the symbol, so reclaiming one cannot be observed.
- **Fresh names (#2)** are class D: every oracle keeps them (chibi 400, Gauche 484, Chez 446 B/name; Patina 365 [P]).
  In the redesign each costs a symbol, a cell (32 B), a binding record (32 B, variant R) and a namespace map entry;
  the lane checks that the cost per name does not regress.

### 3.2 Placeholder records for unbound names (#3)

- **Today** a reference to an unbound global in compiled code retains only the symbol: 457 B/name including the pacing
  headroom the live symbols buy (`unbound-ref` [P]; no placeholder slot exists in `crates/patina-core/src/environment.rs`
  or `crates/patina-vm/src/compiler/` [S]).
- **Redesign.** "A name with no binding yet gets a placeholder record whose cell holds `UNBOUND`" (DESIGN §8.6), and
  records and cells are immortal. A server that `eval`s generated code with typos, or a REPL session, leaks a symbol, a
  record and a cell per distinct unbound name, and the cell joins the root region scanned at every major (#20). The
  global-binding-cells gap report already listed "placeholder and introduced-cell growth in long REPL sessions or code
  that runs `eval` in a loop" as risk 6; DESIGN dropped it.
- **Oracles** retain 288–356 B/name, so the leak itself is common; the pause growth would be Patina's alone.
- **Fix.** Placeholder records are **weak entries** of their namespace: an entry whose cell is still `UNBOUND` and whose
  record no live link table references is removed in epilogue step 6. A later `define` creates the record again;
  code compiled earlier and still alive holds its record, so it sees the definition, which is variant R's whole point.

### 3.3 Variant R binding records and variant C (#2–#6, #20)

The records of variant R are what make most of §3.2–§3.6 permanent. Variant C (decision 2, after stage 5) removes the
records but keeps cells in the immortal space, so it fixes nothing here by itself. Both variants need the same rule:
**cells and records are mortal and owned by a namespace** (R-SS1, §4.1). The global namespace and every library
namespace are roots, so their own definitions live as long as today; what becomes reclaimable is exactly what the
probes leak: placeholders, aliases, introduced definitions nothing references any more, environment-specifier
namespaces, and the namespaces of redefined libraries. JIT code that embeds a cell address holds that cell through its
unit's link table (the fourth embedding case of §3.1), and `WATCHED` keeps working because it is a flag on a live cell.

### 3.4 Macro-introduced top-level definitions (#4)

- **What grows.** `RareTables.introduced_global_names` maps spelling → scope set → renamed global
  (`crates/patina-core/src/environment.rs:384-403` [S]), and each renamed global is an append-only slot ("a binding's
  slot never moves or disappears", `:186-197`; "nothing removes a binding from either table", `:528-530` [S]).
- **Behaviour.** A macro whose template defines a hidden top-level helper (`(begin (define state 0) (define (name) …))`,
  or `(define-values () …)`'s `dummy`, `binding.scm:93-96` [S]) expanded again and again: REPL redefinition, or `eval` of
  generated code. Semantically each expansion needs its own binding, but once the visible name is redefined nothing can
  reach the old hidden one.
- **Measured** 245 B per expansion and **quadratic time**: 25 K expansions 5 s, 100 K 76 s [P]. The `sample` profile
  puts the time in `alpha_rename::RenameEnv::resolve` → `Environment::for_each_introduced_global` →
  `ScopeSet::is_subset_of` (`crates/patina-vm/src/compiler/alpha_rename.rs:164-169`,
  `crates/patina-core/src/environment.rs:1220` [S, P]): every reference to `state` scans every introduced definition
  of that spelling ever made.
- **Oracles** all retain the bindings (chibi 928, Gauche 523, Chez 222 B/expansion) and chibi and Chez are also
  superlinear (×16.5 and ×13 for 4× N) [P]. Patina is cheapest per expansion and should not stay quadratic.
- **Redesign.** Introduced definitions become records and cells in the immortal space (DESIGN §8.6); the scan is not
  mentioned.
- **Fix.** (1) Now: index the candidates so a reference finds its definition without scanning all of them (the
  candidates whose scope set is a subset of the reference's; a map keyed by the expansion scope that introduced each
  definition answers it in one probe per scope of the reference). (2) Stage 4b: the introduced entry is weak like a
  placeholder (§3.2): it lives while code that references its record lives. That is better than every oracle.

### 3.5 Per-expansion alias bindings (#5)

- **What grows.** When a template refers to a binding the use site cannot see by name (a library-private helper, or a
  definition a generator introduced), the desugarer makes a fresh name `name.N` from a process-wide counter
  (`crates/patina-frontend/src/desugarer/mod.rs:196-205` [S]), interns it as a symbol, and installs an alias in the use
  site's environment (`:1270-1302` [S]); `alias_bindings` is never pruned (`environment.rs:502-513,528-530` [S]).
- **Behaviour.** Any top-level use of such a macro: `guard` (its `%guard-aux`, `lib/scheme/base/exceptions.scm:82-102`
  [S]) makes 3 per expansion [P]. This is not only `eval`: **a long REPL session or a long stdin stream leaks 1.8–2.3 KiB per
  top-level `guard` form** [P], about three times what the three symbols alone cost (3 × 215 B).
- **Redesign.** Aliases are binding records in the immortal space (DESIGN §8.6: "own definitions, imports, aliases and
  placeholders alike").
- **Fix.** An alias names a *location*, so it needs no fresh name: under link tables the template's reference resolves
  at expansion time to the definition site's record, and the code's link table holds that record directly. No name is
  added to the use site's namespace and nothing is interned. Before 4b, dedupe aliases by target (as `import_alias`
  already does: "one name for one location has one alias however many expansions ask", `environment.rs:419-424` [S]).
  File the issue now (§7, I1).

### 3.6 Environment specifiers (#6)

- **Today.** `environment` builds a fresh `Rc<Environment>` and imports every export into it
  (`crates/patina-primitives/src/primitives/eval.rs:54-80` [S]); `null-environment` and `scheme-report-environment`
  do the same (`:481,527`). The specifier is one heap object; its ~260 bindings, index and import links are Rust memory
  the object-count trigger never sees. Result: a **970 MiB plateau** (VM) and 930 MiB (TW) against 8.7 (chibi), 31.5
  (Gauche) and 49.4 (Chez) [P].
- **Redesign.** Every name of an environment specifier maps to an immortal binding record (DESIGN §8.6). Each call would
  leak about 260 × 32 B ≈ 8 KiB of records for good, plus the root-region cost at every major: **a class-B plateau
  becomes a class-A leak.** The gap report's own model said "an `environment` specifier creates **no** cells, only
  imports" (gaps/global-binding-cells.md C10); records reintroduce the cost.
- **Fix.** (1) Stage 1: the byte trigger charges an environment's Rust bytes (bindings, index, links) to the specifier
  that owns it; PR-2's list (vector and string buffers, bignums, continuation snapshots) omits environments. (2) Stage
  4b: a namespace is a host payload with `FinalKind::HostPayload`; its records are allocated in its own chunks (or as
  ordinary old objects) and released when the specifier dies and no code compiled in it is alive. Imports in a
  specifier point at the exporter's records or cells, so the specifier owns nothing else.

### 3.7 Provenance, expansion chains and source documents (#7, #27)

- **What grows today.**
  - `Heap.syntax_sources: HashMap<u64, Rc<SyntaxSource>>` keyed by raw bits, pruned at sweep
    (`crates/patina-core/src/heap/mod.rs:305` [S]); `SourceMap.locations` keyed by raw bits, pruned only between
    top-level forms (`crates/patina-core/src/source_map.rs:61,213-246` [S]), so a server loop inside one form never
    prunes it (it stays bounded by the arena slot count, because keys repeat as slots are reused).
  - **Expansion chains.** `stamp_expansion_source` walks every node of an expansion and, for a node that already has a
    location, stores a new chain = its old chain + the macro's name (`crates/patina-frontend/src/desugarer/mod.rs:110-163`
    [S]). A node that *persists* across expansions (a quoted datum in the program text passed to `eval` again and again;
    the clause data that `case` re-inserts) gets one more entry per expansion, and the chain is copied in full each time:
    **95–118 KiB per eval and quadratic time** for `case` [P]. The same datum read from a string at run time has no
    provenance and stays flat [P], which isolates the cause.
  - `SourceDocument.expansions` appends a name per `(line, column)` when a location has no span
    (`crates/patina-core/src/source_document.rs:110-118` [S]); rare.
  - Streaming text is compacted: `push_line`/`forget_old_lines` (`source_document.rs:69-108`), driven from
    `crates/patina-repl/src/program_stream.rs:135` [S]; stdin's `Unread` buffer reclaims its consumed front
    (`crates/patina-core/src/port.rs:122-133` [S]); rustyline history is capped at 1,000 (`crates/patina-repl/src/repl/mod.rs:242` [S]).
    Measured flat [P].
- **Oracles.** chibi and Gauche keep source information inside the syntax objects of the expansion, so it dies with
  them [I]; their `eval` loops are flat [P].
- **Redesign.** Stage 1 deletes `locations` and child spans (DESIGN E.1); stage 4c gives identifiers a source id into a
  **per-document location table**, with form spans on head identifiers. Nothing states who owns a document's table, when
  it is freed, how a streaming document (stdin, the REPL) compacts it, or where expansion chains live.
- **Fix.** (1) Now (issue I2): a chain belongs to an *occurrence*; stamping must not extend the chain of a node that an
  earlier expansion stamped, or chains must be interned as `(parent chain, macro)` nodes so a re-stamp is one lookup.
  (2) Stage 4c rules: a document's location table is owned by the document, which code units reference (through their
  source ids), so it dies with the last unit; a streaming document compacts its table with its text, as
  `forget_old_lines` does; expansion chains are interned per document and pruned with it. (3) The lane runs
  `macro-eval case` and `guard` at 4N.

### 3.8 Scope sets (#8)

- **Today** a scope set is an inline `SmallVec` in each identifier; the counter is a `usize`
  (`crates/patina-core/src/scope.rs:36` [S]) and `SCOPE_ORIGINS` grows only through `fresh_with_origin`, which nothing
  calls (`scope.rs:39-71` [S]). No table grows.
- **Redesign.** Stage 4c interns scope sets and stores a "scope-set id in the length bits" of each identifier
  (DESIGN §3, D 4c). Every expansion makes fresh scopes and therefore fresh sets (libload: 64,782 distinct sets for
  2.19 M identifiers, DIGEST §3.14), so an intern table that only grows is a **new class-A leak** for every `eval` and
  REPL form.
- **Fix.** The intern table is weak: an entry dies when no identifier carries its id (pruned in epilogue step 6, with
  the ids recycled through a free list), or the table is scoped to one top-level form or one library body, whose
  identifiers that survive (macro templates, literals) are re-interned into a long-lived table at the end. The lane's
  `eval` probes catch it.

### 3.9 Code store (#9), descriptor space (#19), JIT code memory (#10), literal pool (#11)

- **Today: steady.** Units are released when nothing can run them (`load_unit`, `release_unit_if_unused`,
  `after_collection`: `crates/patina-vm/src/runtime/vm_state.rs:430-575` [S]); `eval-redefine`, `eval-lambda`,
  `load-repeat` and the redefinition streams are flat [P]. The slot vector keeps its peak length (8 B per slot, reused
  through a free list, `:464-472` [S]).
- **Descriptor space.** "Append into dedicated blocks; marked at majors; units released by the registry; never moves"
  (DESIGN §4). If freed descriptors leave holes that are never reused, a REPL that keeps one escaped unit (a procedure
  stored in a global) per block's worth of short-lived units keeps every block: one live 48 B descriptor pins a 32 KiB
  block. Record types live there too, and `define-record-type` through `eval` makes one per call. **Fix:** the descriptor
  space is a mark-region space like the SOS (lazy sweep, holes reused, never evacuated), and K3's fragmentation report
  covers it.
- **JIT code memory.** Decision 20: own `MAP_JIT` reservation, per-unit freeing after a complete major, bodies never
  moved or patched. Without moving, fragmentation in the code reservation is permanent, so the allocator decides
  whether a REPL or `eval` server plateaus. **Fix:** size-segregated slabs for small bodies and a coalescing free list
  for large ones, a fragmentation figure in `GcStats`, the `eval-lambda` and `eval-redefine` probes in the JIT lane, and
  a cap on the reservation with a policy at the cap (stop tiering up, keep interpreting) instead of a failure.
- **Inline caches** hold traced descriptor references (DESIGN §6.8). Strong entries keep a redefined callee's unit alive
  until the site is rewritten; this is bounded by the number of sites, so weak entries are only an optimisation.
- **`CoreExpr` literal pool.** DESIGN §8.3 adds "a traced table shared by both backends" for literals held by `CoreExpr`
  and `CpsExpr`. If it is append-only, every literal of every `eval`'d form is rooted for ever. **Fix:** the pool is
  scoped to one compilation (an epoch, cleared when the unit's constants exist), or its entries belong to the unit.

### 3.10 Library registry and redefinition (#12)

- **Today.** `register_or_replace` replaces the registry entry (`crates/patina-runtime/src/library_registry.rs:466-468`
  [S]), yet each top-level redefinition plus `import` retains **49 KiB** (32 K redefinitions: 1.59 GiB, 19 collections
  [P]). The retaining path was not isolated; candidates are the importer's import links and `import_alias` entries
  keyed by the old library's environment id. Separately, `define-library` forms alone never reach a safe point: 8 K of
  them reach 545 MiB with one collection [P]. (`define-library` through `eval` is refused: "import is only allowed at
  top level" [P].)
- **Oracles.** Libraries are never unloaded in chibi, Gauche or Chez; redefinition at the REPL is rare in all of them.
- **Redesign.** A library is a host payload (DESIGN §3) and the loader collects between forms at points A and B
  (DESIGN H.5), which fixes the missing safe point; but the old library's cells and records are immortal, so the leak
  stays.
- **Fix.** The redefined library's namespace is mortal (§3.3) and its code units are released by the ordinary rules;
  file the present leak as I4.

### 3.11 Ports (#13)

- **Today: steady.** `OUTPUT_FILES` holds weak references pruned at amortised doubling thresholds
  (`crates/patina-core/src/port.rs:170-199` [S]); string-port churn is flat and file-port churn plateaus at 30 MiB [P].
  Unclosed file ports still exhaust descriptors at the 1,021st open (known, DESIGN Appendix B).
- **Redesign:** `PortTable` slots freed by finalization, descriptor pressure posting majors, `EMFILE` collect-and-retry
  (DESIGN §6.7). Steady. One rule to add: the process-wide weak registry of `PortTable`s (R1) prunes entries of dropped
  heaps (as `OUTPUT_FILES` does), or an embedder that creates interpreters in a loop grows it.

### 3.12 Continuations (#14) and tree-walker environments (#15)

- **Today** class B: weak side tables of `Rc<VmContinuation>` (`crates/patina-vm/src/runtime/vm_state.rs:196-199`, per
  DIGEST §1.5) under an object-count trigger: 563 MiB at depth 100 on the VM; on the tree-walker 1.96 GiB after 20 K
  captures at depth 100 with one collection [P]. Tree-walker environments and continuations are `Rc` graphs freed only
  when sweep drops the procedure that owns them.
- **Redesign** fixes both: continuations are heap objects charged by bytes (stages 1 and 4e), tree-walker payloads are
  charged as external bytes (4f). The lane keeps `cont-churn` at depths 10, 100 and 1,000 and `env-churn` on both
  backends.

### 3.13 Register stacks (#16) and green-thread stacks (#26)

- **Today** class C: `ExecutionState` truncates its register and frame `Vec`s but never releases capacity
  (`crates/patina-vm/src/runtime/execution_state.rs:80,275-282` [S]); 143 MiB stay resident after a 1 M-frame recursion,
  984 MiB on the tree-walker [P].
- **Redesign** fixes the main thread: a reserved stack whose unused pages are decommitted after two observations (a
  major or a return to the top level), in 4 MiB runs (DESIGN §4, H.3).
- **Green threads are not steady-state safe as specified.** Each started thread gets its own reservation (256 MiB by
  default, DESIGN §4 and §8.1). Consequences: one VMA (two with a guard page) per thread, so Linux's default
  `vm.max_map_count` of 65,530 caps a process near 32–65 K live threads, a limit nobody chose; at least one committed
  page (16 KiB here) per thread; and decommit in 4 MiB runs never trims a stack shorter than 4 MiB, so a blocked thread
  that once went deep keeps up to 4 MiB resident. chibi keeps 32 KiB per blocked thread and nothing per terminated
  thread [P]; Gambit's threads cost a heap continuation [I].
- **Fix (stage 9).** Thread stacks are sub-allocated from one per-heap stack arena (one VMA), small at first; frames are
  position-independent (byte offsets, DESIGN §8.1), so a stack grows by moving to a larger slot at a `Transfer`
  boundary, or by C′'s segments once they exist; decommit for stacks is page-granular. The `blocked-threads` probe
  reports bytes per thread, and a `thread-churn` probe joins the lane.

### 3.14 Heap arenas, sweep and decommit (#17, #22)

- **Today** class C in memory and in time: four `Vec` arenas never shrink, and every collection sweeps the whole arena
  high-water mark after pre-marking the free lists (DIGEST §1.4). After a 2 M-element peak is dropped, RSS stays at
  210 MiB and each `(gc)` takes 9.3 ms instead of 0.095 ms [P].
- **Redesign** removes both: lazy sweep and block classification run after the pause, so no pause term depends on dead
  objects; decommit with two-major hysteresis gives memory back (DESIGN §4, §6.1).
- **Two gaps.** (1) Decommit and the catch-up sweep run only after a major, and a major runs only when allocation
  reaches the target. A server that handles one huge request and then idles in `read` never collects again and keeps
  its peak. G1 had the same problem and fixed it with periodic collections that return memory (JEP 346,
  https://openjdk.org/jeps/346). Patina needs a deterministic trigger: on entering a blocking safe region (reads,
  `thread-sleep!`, waits) or returning to the top level, if committed bytes exceed `3·L_last + 64 MiB`, run a major and
  queue decommit. Counts, not time, so the lanes stay deterministic. (2) "Two consecutive majors that each free under 1%
  raise the target ×1.5" (DESIGN §13) has no way back down; a transient phase would raise the target, and with it the
  committed reserve `(target − L) + free_reserve`, for the rest of the run. The boost must decay (for example, recomputed
  from `2·L` at the next major that frees more than 10%).

### 3.15 Immortal space (#18) and the root-region pause (#20)

- **As specified** the immortal space holds global cells, binding records, interned symbols, canonical flonum boxes and
  primitives with their descriptors, in append-only blocks that are never swept (DESIGN §4). §3.1–§3.10 show that four of
  those five kinds are created by run-time operations whose count is unbounded under bounded live data.
- **Pauses.** Cells are a root region scanned at every major, and in generational mode every pointerful root-region
  object is re-armed at every major (DESIGN §6.1, §6.10 step 9). Every leaked cell therefore adds to every later major
  pause, which defeats SS2 and decision 6's intent without breaking its letter (an immortal cell counts as "live").
- **Fix:** rule R-SS1 (§4.1).

### 3.16 Store buffers, mark stack, LOS cache, finalization registry, handles (#21)

Bounded as specified: the store buffer has a soft limit derived from the minor budget and decommits its overflow pages
after draining (DESIGN §7, H.3); the LOS recycle cache is capped at 32 MiB; the finalization registry holds one entry
per live registered object and queues the dead at every complete collection, with backstops bounding how long old
entries wait (DESIGN §6.2, §6.7); `Owned` handles are generation-checked and freed on drop. One unstated rule: the mark
stack's recycled segment pool (DESIGN H.2, §6.3) gives segments back above a small reserve after a major, or a single
deep mark (for example a 2 M-element list of vectors, +55 MiB today) keeps its peak.

### 3.17 Observability (#23)

`GcStats` per-site high-water counters (K16) are bounded by the number of sites. The MMU is computed "from every
non-mutator interval at 1–100 ms windows" (DESIGN §13); stored as a list of intervals it grows with run length. Use a
streaming form: for each window size keep only the intervals that end within the last window (a ring buffer) and
update a running minimum, so memory is fixed by the largest window. `PATINA_GC_LOG` writes to a file the user chose.

### 3.18 Embedding (#24)

Today each dropped `VmInterpreter` leaks **3.6 MiB** (400 interpreters: 1.45 GiB, linear [P]); the cause is the known
`Rc` cycle (DIGEST §1.11, item 2). Stage 2's teardown fixes it (DESIGN D 2). Add `interp-churn` (this audit's
`interp-churn` crate) to the lane, with the process-wide port-table registry, `InterruptHandle` cells and any other
process-wide table checked for pruning in the same probe.

### 3.19 Threads (#25)

Thread objects, mutexes and condition variables are heap objects; the thread table frees an entry at termination and
the finalizer releases the stack at the next poll (DESIGN §12). A thread blocked for ever on objects nothing else reaches
stays rooted (decision 23), as in chibi (32 KiB per thread here [P]) and Gambit; that is class D by the owner's choice.

### 3.20 Counters and ids (#28) and small tables (#29)

No counter can be exhausted in practice: scope ids are `usize`, environment ids and alias ids `u64`
(`environment.rs:443`, `desugarer/mod.rs:196` [S]), `CodeObjectId::label` wraps harmlessly by design
(`crates/patina-vm/src/types/code_object.rs:46-55` [S]); the redesign's 48-bit scope-set and source ids are out of reach.
Slot tables with free lists (`code_store`, the arenas' free lists, `PortTable`, `HostPayloadTable`, the thread table,
the handle table) keep their peak length at 4–16 B per slot; acceptable if stated, or shrink when the top slots are free.
`gc_freed_bits` is capped at 65,536 entries and `gc_freed_closure_code_ids` is drained after each collection
(`heap/mod.rs:396-415` [S]).

---

## 4. What the redesign should add

### 4.1 Five rules for `docs/GC_DESIGN.md`

| Rule | Text | Sources it governs |
|---|---|---|
| **R-SS1** | The immortal space admits only objects whose number is bounded by the binary and the program text: primitives and their descriptors, canonical flonum boxes, core-syntax markers, a future boot image. Everything a run-time operation creates is mortal, and objects that must not move go to a non-moving space that majors sweep. | symbols, cells, binding records (#1–#6, #18, #20) |
| **R-SS2** | Every per-heap table keyed by name, id or address is either bounded by program text or has an owner whose death removes the entry, through finalization or epilogue step 6. Each such table is listed with its owner in the off-heap holder inventory (DESIGN E.1). | namespaces, alias and introduced tables, scope-set and location tables, chains, literal pool, registries (#3–#12, #23) |
| **R-SS3** | Every Rust byte a heap object keeps alive is charged as external bytes while it lives, and `max_heap` bounds managed bytes (reservation commit plus external bytes), not the reservation alone. | environment specifiers, library namespaces, code bodies, string-port buffers, tree-walker payloads (#6, #12, #14, #15) |
| **R-SS4** | No operation scans a table whose size grows with history; lookups are by key. | introduced definitions, chain copying (#4, #7) |
| **R-SS5** | Memory reached at a peak is given back within two majors after the peak passes, and a major is guaranteed at the next blocking safe region or top-level return when committed bytes exceed `3·L_last + 64 MiB`. Boosts to the pacing target decay. | heap blocks, LOS cache, register and thread stacks, store buffers, mark-stack segments (#16, #17, #21, #22, #26) |

### 4.2 Changes by stage

| Stage | Change | Probe that proves it |
|---|---|---|
| 0 | Bring in the steady-state probes and runner (§5); record today's slopes on S0; file I1–I6 (§7) | the lane runs, red where §2 says |
| 1 | Byte trigger charges environment and library namespaces' Rust bytes (R-SS3); fix chain growth (I2) | `env-churn` ≤ 64 MiB; `macro-eval case` flat |
| 2 | Teardown (already planned); process-wide registries prune | `interp-churn` flat |
| 3 | `CoreExpr` literal pool scoped per compilation (§3.9) | `eval-lambda` flat on both backends |
| 4a | `PortTable` registry pruning | `file-port-churn`, `interp-churn` |
| 4b | Namespaces as owners of mortal records and cells; weak placeholders, introduced definitions and aliases; aliases as direct record references (§3.2–§3.6); indexed introduced-definition lookup if not done earlier | `unbound-ref`, `hidden-define` (bytes and time), `guard` stream, `env-churn`, `library redefinition` |
| 4c | Weak or epoch-scoped scope-set table; document-owned, compacted location tables; interned chains | `eval` probes and the stdin and REPL streams |
| 4d | Register stack decommit (already planned) | `deep-then-steady` returns to baseline |
| 5 | Symbol space (weak or strong per the owner's decision); cell and record space swept at majors; descriptor space reuses holes; post-peak trim at safe regions; decaying pacing boost; mark-segment trimming | `sym-churn`; `peak-then-drop` (memory and pause); a `frag-desc` probe |
| 6 | JIT code allocator and cap | `eval-lambda`, `eval-redefine` under the JIT lane |
| 9 | Stack arena for green threads, page-granular trim | `thread-churn`, `blocked-threads` bytes per thread, a 100 K-thread probe on Linux CI |
| 0 on | Streaming MMU | memory of `GcStats` constant over a 1-hour soak |

### 4.3 A kill criterion

**K17 — steady state.** A stage that turns a steady-state row red (a slope above its threshold, or a time ratio above
4.4) is blocked until it is green again, as a red tree-walker lane blocks its stage today (DESIGN §8.4). Stage 5's exit
(K9) also requires every class A, B and C row of §2 to be green on both backends, except the rows the owner classes as
D.

---

## 5. The steady-state lane

- **Programs:** the probes of this audit (`PRD/study/gc/probes/followup/steady/probes/*.scm`; Chez variants in `chez/`), vendored
  as Patina-authored programs under `crates/patina-tests/bench_programs/gc/steady/` with the GBS: `sym-churn`,
  `eval-fresh-names` (D), `unbound-ref`, `hidden-define`, `macro-eval` for `case`, `guard` and the other standard
  macros, `form-eval-fresh`, `eval-redefine`, `eval-lambda`, `env-churn`, `env-lambda`, `record-redefine`,
  `define-values-null`, `port-churn`, `file-port-churn`, `cont-churn` (depths 10, 100, 1,000), `load-repeat`, `reimport`,
  the library-redefinition and `guard` streams as file, stdin and REPL `-i`, `steady-alloc` (timed), `peak-then-drop`
  and `pause-after-peak`, `deep-then-steady`, `interp-churn` (Rust), and from stage 9 `thread-churn` and
  `blocked-threads`.
- **Rule:** run at N and 4N, both backends, release build. Pass when RSS(4N) − RSS(N) ≤ max(2 MiB, 5% of RSS(N)) and
  time(4N)/time(N) ≤ 4.4; for timed probes, RSS over the second half varies by at most 5%; `pause-after-peak` passes
  when the pause after the drop is within 2× the pause before the peak. Class D probes record bytes per name and fail
  on a 20% regression.
- **Oracles:** chibi, Gauche and Chez numbers are recorded beside each row as context, not as pass criteria; Patina's
  target is "no worse than the best oracle that is steady on the row".
- **Cost:** about 3 minutes per backend at the N used here; it can run nightly rather than per PR, with the four
  cheapest probes per PR.
- **Diagnosis:** `(gc-stats)` gains keys per owner so a red row says what grew: `symbols`, `namespaces`, `cells`,
  `binding-records`, `code-units`, `descriptor-bytes`, `jit-code-bytes`, `external-bytes` by kind, `scope-sets`,
  `documents`, `ports`, `threads`.

---

## 6. Limits organised around steady state

The owner asked for limits as an explicit part of the effort. With the rules above each limit has one meaning: a
steady-state program never reaches it; a growing program reaches it and gets a catchable condition that names the
resource.

| Limit | Default (DESIGN decision 15 unless noted) | Covers | Condition at the limit | Steady-state requirement |
|---|---|---|---|---|
| `max_heap` | min(16 GiB, 75% of RAM) | **managed bytes**: committed reservation plus external bytes (R-SS3) | `&heap-exhausted` after one collect-and-retry | without R-SS3, Rust-table leaks (today's largest) bypass it |
| `--stack-max` | 8 GiB main; 256 MiB per green thread | register stack per thread | `&stack-exhausted` | decommit after the peak (R-SS5) |
| code memory | new: a cap on the JIT reservation | JIT bodies | stop tiering up, keep interpreting; no error | per-unit freeing plus an allocator that does not fragment without bound |
| file descriptors | `RLIMIT_NOFILE` | open ports | `EMFILE` collect-and-retry, then a file error | port finalization (R2, R3) |
| threads | new: stated as a number, not left to the VMA limit | started, unterminated threads | an error from `thread-start!` | stack arena (§3.13); blocked-for-ever threads count, by decision 23 |
| store buffer hard end | 256 MiB VA per mutator | logged granules | never reached: soft limit forces a collection | — |

A limit should be reported in `(gc-stats)` beside the current use, so a program can watch its own margin.

---

## 7. Observed problems to file as GitHub issues

Per AGENTS.md (search issues and `PRD/ARCHIVE` first; symptom and minimal repro only):

| # | Title | Repro | Measured |
|---|---|---|---|
| I1 | A library macro with a private helper leaks interned aliases per expansion | 100 K top-level `(guard (e (#t 0)) (raise 'x))` forms, as a file, on stdin, or at the REPL | 1.8–2.3 KiB per form; `symbols` +3 per expansion |
| I2 | Re-evaluating a datum with source locations grows memory and time without bound | `(define form '(case 3 ((1 2) 'a) ((3) 'b) (else 'c)))` then `(eval form (interaction-environment))` N times | 95–118 KiB per eval; 10 K evals 949 MiB / 42 s; 20 K 2.2 GiB / 168 s |
| I3 | References to macro-introduced top-level definitions get slower with each expansion | `def-counter` expanded N times through `eval` | 25 K: 5 s; 100 K: 76 s; 245 B retained per expansion |
| I4 | Redefining a library at top level retains the old one, and `define-library` forms never reach a safe point | repeated `(define-library (tmp steady) …)` + `(import (tmp steady))` | 49 KiB per redefinition; 8 K bare redefinitions: 545 MiB with one collection |
| I5 | `environment` churn plateaus near 1 GiB | `(environment '(scheme base))` in a loop | 972 MiB plateau (chibi 8.7, Gauche 31.5, Chez 49.4) |
| I6 | Pause after a dropped peak stays at the peak's sweep cost | build and drop a 2 M-element list, then `(gc)` | 9.3 ms against 0.095 ms before the peak; RSS 210 MiB kept |

Already known and planned (file only if no issue exists): the interpreter teardown leak (3.6 MiB per interpreter here),
continuation trigger blindness, the 1,021st-open `EMFILE`. Observed but not a steady-state problem: `define-library`
through `eval` is refused with "import is only allowed at top level"; check R7RS and the oracles before filing.

---

## Appendix A. Method and reproduction

- Build: `CARGO_TARGET_DIR=<target dir> cargo build --release -p patina-repl` from `main` at `28a94f8`.
- Runner: `PRD/study/gc/probes/followup/steady/run.py IMPL PROBE ARGS…` runs `timeout -s KILL 180 /usr/bin/time -l` over the
  implementation (`patina`, `patina-tw`, `patina-stdin`, `chibi`, `gosh -r7`, `chez --script`) and prints JSON with max
  RSS, wall time, output and an optional `ps`-sampled RSS series (`SERIES=1`); `PATINA_STATS=1` appends `(gc-stats)`.
  `batch.sh` reads `impl probe args` lines; `scaled.sh IMPL` runs the main set at N and 4N; `series.sh` and `series2.sh`
  run the timed probes; `slopes.py` and `sersum.py` print the tables of §2.
- Results (not retained): `results/scaled.jsonl` (Patina and the oracles), `scaled16.jsonl`, `cmp.jsonl`, `macro.jsonl`,
  `macro-case.jsonl`, `rerun.jsonl`, `tw.jsonl`, `stream.jsonl`, `lib.jsonl`, `threads.jsonl`, `formexp.txt`,
  `series.jsonl`, `series2.jsonl`, `interp-churn.txt`; CPU profiles `hidden-define.sample.txt` and `case.sample.txt`.
- The embedding probe is the crate `PRD/study/gc/probes/followup/steady/interp-churn` (path dependency on
  `crates/patina-interpreter`).
- Caveats: one machine; max RSS from `time -l`, which includes the allocator's retained pages; several oracle runs
  shared the CPU with others, which changes times but not RSS plateaus; slopes from two points (three or four where
  the table shows them); the Chez probes use Chez-native forms (`chez/*.ss`), and Chez's `environment` is slow enough
  that its churn ran at 300 and 1,200.
