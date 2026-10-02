# Patina garbage collector: the recommended design

Date 2026-10-01. Repository `main` at `28a94f8`. Status: proposal for owner review.

**Where each part goes.** AGENTS.md forbids new markdown files without owner approval, keeps PRDs high level, puts
work items in GitHub issues, and cites the 2,500-line Track L record as the shape to avoid. So this text is written in
four parts, each marked at its heading. Only Parts I and II are committed as blocks, each by rewriting an existing
file in place (decision 22):

| Part | Sections | Destination |
|---|---|---|
| I. The design | conventions, executive summary, §0–§14, §17 | the body of `docs/GC_DESIGN.md`: the contract (encoding, object model, invariants, barrier, roots, polls, continuations, pluggability) and the decisions |
| II. The plan | §15 (one line per stage) and §16 (kill criteria, one anchor per row) | the body of `PRD/future/GC_STAGE5_PRD.md` |
| III. Work items and inventories | Appendices B, D, E, H (each block names its issue) and F | B, D, E, H: the bodies of the issues filed at stage 0; F: `docs/TEST_ORGANIZATION.md`, each entry added by the stage that creates the lane, test or program |
| IV. Review record | Appendices A, C and G | the description of the PR that lands Parts I and II |

Section and appendix numbers are kept from the reviewed draft, so the review dispositions still resolve. At the
split, Part I is renumbered and its references to §15, §16 and the kill criteria become links into
`PRD/future/GC_STAGE5_PRD.md` (decision 22).

---

# Part I. The design (→ `docs/GC_DESIGN.md`)

**Conventions.** **[P]** marks a Patina measurement on one machine (Apple M4 Pro, macOS 27.2 arm64, 16 KiB pages,
release build at `28a94f8`), with the workload named; each is listed with its workload or probe in the measurement
table of the S0 tracking issue, which stage 0 re-measures after bringing in the harness (a `gc` mode of
`scripts/benchmarks.py`), the census (the `gc-census` feature) and the probes (`scripts/gc_probes/`). **[S]** marks a
fact read in source, at `28a94f8` or an upstream path; **[I]** marks judgement. "S*n*" is the issue for stage *n*
and "K*n*" a kill criterion, both in `PRD/future/GC_STAGE5_PRD.md`. "GBS" is the GC benchmark set (§14). "The
matrix" is `crates/patina-tests/tests/control_flow_matrix.rs` (64 rows [S]); "the hygiene matrix" is
`crates/patina-tests/tests/hygiene_matrix.rs` (139 shapes).

## Executive summary

1. **Representation first, collector second.** The measured costs are the representation (72 B enum slots, `Rc`
   payloads, relocating `Vec` arenas, `Rc<RefCell<Heap>>`, a sweep over the arena high-water mark), not mark-sweep:
   2.05× the bytes of a headered layout [P, 348 M-object census], and a 178 ms worst pause [P, first collection after
   loading 25 R7RS-large libraries], about 18 ms of it sweep and the rest releasing 2.9 M Rust `Drop` payloads and
   pruning provenance.
2. **Value word:** fixnum tag `000` (61 bits); an exact heap bit (bit 2); 4-bit heap tags from 16 B alignment;
   self-tagged flonums if they pay with inline flonum operations in both arms (K6); raw tagged addresses.
3. **Objects:** headerless pairs, procedures (word 0 = code descriptor) and records (word 0 = record type); one
   immutable header elsewhere; no GC bits and no Rust `Drop` payload in any object; one layout specification
   generates size, trace, copy, verify and the JIT offsets; code references are ordinary references.
4. **Heap:** one VA reservation per heap; 16 B granules with a side metadata byte; 32 KiB blocks; mark-region
   allocation (bump into holes, lazy metadata-only sweep); a large-object space with young and old lists; a
   non-moving descriptor space; an immortal space for cells, symbols and primitives; decommit with hysteresis.
5. **Allocation never collects** (Chez runs a moving generational GC under the same contract). The fast path is
   `add; cmp; b.hi`. A user-sized request that finds the heap full collects and retries once, at its call.
6. **One production collector, `MarkRegion`:** non-moving and whole-heap first; sticky generations and Immix-style
   evacuation are per-heap switches that measured gates turn on; a copying nursery is the pre-planned fallback (K4).
7. **Barrier:** field logging on a per-granule armed bit; 4 aarch64 instructions for heap values, 1 for immediates; a
   call-free slow path into a per-mutator store buffer whose soft limit follows from the minor-pause budget.
8. **Roots are precise.** In every JIT tier, VM register frames hold every Scheme value, as a tagged value, at every
   non-`Leaf` call; no Cranelift stack maps, no raw slots. Rust holds raw values freely inside a `Cx`, which cannot
   collect; only the driver's `&mut Heap` can. Tree-walker heaps stay whole-heap and non-moving.
9. **One poll word:** posting an event zeroes the stack limit that every frame entry checks; every closure call, tail
   call and `Transfer` return polls; `(gc)` collects at its call; signals arrive through an `InterruptHandle`.
10. **Weakness:** ephemerons resolved by key in one fixpoint; a fixed epilogue; a finalization registry; GC-time port
    flush and `EMFILE` collect-and-retry, as chibi and Gauche do.
11. **Continuations:** immutable value-only heap objects (design A) over a non-relocating stack; C′ optional, later.
12. **Pacing and pauses:** bytes plus external resources, deterministic; whole-heap interval `max(8 MiB, 2·L)`.
    Minors and majors have budgets of one form, a constant plus terms in frames, logged granules and marked MiB, and
    no pause term grows with dead objects or heap size (decision 6). Parallel stop-the-world marking (stage P) is
    budgeted, because a single-threaded major at 1 GiB live is estimated at 120–320 ms.
13. **Pluggability is a contract** (§11): HotSpot's GC interface without its catalogue of collectors. `ObjectModel`,
    `Collector<M>` with the mutators lent to `collect`, a slot-only visitor with an ephemeron hook, a weak registry,
    the epilogue order, a `#[repr(C)] Mutator`, `GcAttrs` read as data, two helper classes. One production
    collector; `NullGc` only in the conformance suite; every runtime mode, `PATINA_GC=null` included, a mode of
    `MarkRegion`. No load barriers, no `dyn` on fast paths, no code patching, no MMTk dependency.
14. **Threads:** SRFI 18 as M:1 green threads; the `Mutator` (carrier) split from the `GreenThread`; thread, mutex and
    condition-variable objects with a root rule and a stack lifetime; protocols written for N mutators; isolates
    optional.

**For the JIT.** One context, the `Mutator`, in pinned `x21` (`r15` on x64), saved by a Cranelift-generated entry
trampoline; offsets freeze after a throwaway spike (stage 6). S1 fragments (`CallConv::Tail`,
`return_call_indirect`) are the only baseline. Allocation is `add; cmp; b.hi`, the barrier 4 instructions, a call a
tag test plus 2 dependent loads, `car` a tag test plus 1 load, a constant 1 load, a global 2 loads (variant R) or 1
(variant C, behind `WATCHED`). Polls are free at calls and 2 instructions at back-edges. Every non-`Leaf` helper is a
`Transfer`: the caller publishes, the helper answers "continue" or a new target, and `(code, pc)` is the resume truth.
Attached debugger hooks pin the interpreter tier. Code lives in Patina's own `MAP_JIT` reservation, never patched.

**The plan** (`PRD/future/GC_STAGE5_PRD.md`): stage 0 brings in the benchmark mode, the census and the probes; stage
1 fixes today's measured blow-ups on the current collector; stage 2 makes roots slot-based and embedding sound. The
first PRs are drafted in the stage issues, starting with S0, the tracking issue.

**Owner decisions** (§17, defaults in bold): shared-memory parallelism (**not now**; isolates optional); what
"bounded pauses" means (**one budget form for minors and majors**); rebinding (**R now, C after stage 5**, case by
case); port finalization at GC (**yes**); continuation end state (**design A**); MMTk spike (**not now**); footprint
(**2·L interval, decommit with hysteresis, 16 GiB default `max_heap`, 8 GiB main stack, both settable**); threads
blocked for ever (**rooted until they terminate**, as in Gambit and chibi); and approval of the split that makes
Parts I and II the new `docs/GC_DESIGN.md` and `PRD/future/GC_STAGE5_PRD.md`.

---

## 0. Prior art: what we take from whom

The alternatives each row rejected, and why, are recorded in the description of the PR that lands this design (here,
Part IV, Appendix G).

| Decision | Taken from | Primary sources |
|---|---|---|
| Allocation never collects; collection only at polls; Rust holds raw values between polls | Chez Scheme; gc-arena's "mutation XOR collection" | Chez `c/alloc.c`, `s/library.ss:1194-1240`; https://github.com/kyren/gc-arena |
| Raw tagged addresses, tag folded into the displacement; 16 B alignment; headerless pair, procedure (code first) and record (type first) | Chez (`TYPE(x,t)`, closure code word) | Chez `s/cmacros.ss:478-481,822-829`, `c/types.h` |
| Self-tagged flonums, judged with inline flonum operations in both arms | Melançon, Serrano, Feeley, OOPSLA 2025 (Gambit) | https://arxiv.org/abs/2411.16544 |
| 16 B granules with a side metadata byte; mark-region allocation; lazy sweep that reads only metadata | Whippet nofl/mmc; Immix; RC Immix | https://github.com/wingo/whippet (`src/nofl-space.h`, `doc/collector-mmc.md`); https://www.steveblackburn.org/pubs/papers/immix-pldi-2008.pdf; https://www.steveblackburn.org/pubs/papers/rcix-oopsla-2013.pdf |
| Opportunistic evacuation with a free-block reserve | Immix; Whippet `mmc.c`; Chez/Racket CS mark-in-place; Wingo's heap-growth livelock | Whippet `src/mmc.c:656-715`; Chez `c/gc.c:35-110,1010-1036`; https://wingolog.org/archives/2025/05/22/whippet-lab-notebook-guile-heuristics-and-heap-growth |
| Sticky mark bits for generations, behind a measured switch | Demers et al., POPL 1990; MMTk StickyImmix (the Ruby and Julia defaults); Whippet mmc. Weighed: Wingo's losses on nboyer and splay and Go's rejection of generational barriers, against HotSpot, where every collector family went generational (ZGC, JEP 439, its non-generational mode removed by JEP 490; Shenandoah, JEP 521) | https://wingolog.org/archives/2025/02/09/baffled-by-generational-garbage-collection; https://go.dev/blog/ismmkeynote; https://openjdk.org/jeps/439; https://openjdk.org/jeps/490; https://openjdk.org/jeps/521; https://github.com/mmtk/mmtk-core (`src/plan/sticky/immix`) |
| Field-logging barrier with armed-bit polarity, an inline slow path, a per-mutator store buffer | LXR; Whippet; MMTk `FieldBarrier` | https://arxiv.org/abs/2210.17175; Whippet `api/gc-barrier.h:61-91`; https://wingolog.org/archives/2024/10/03/preliminary-notes-on-a-nofl-field-logging-barrier; https://www.steveblackburn.org/pubs/papers/barrier-ismm-2012.pdf; https://openjdk.org/jeps/522 |
| One GC interface consumed by every execution tier; barriers expanded late from one definition; initializing stores unbarriered | HotSpot's GC interface (JEP 304: `CollectedHeap`, and a `BarrierSet` realized per tier as `BarrierSetAssembler`, `BarrierSetC1`, `BarrierSetC2`); JEP 475; G1's `on_slowpath_allocation_exit` | https://openjdk.org/jeps/304; https://openjdk.org/jeps/475 |
| Ephemerons resolved by key; never older than key or value; one fixpoint | Whippet `gc-ephemeron.c`; SRFI 124, SRFI 254 | https://wingolog.org/archives/2025/01/09/ephemerons-vs-generations-in-whippet; https://srfi.schemers.org/srfi-254/srfi-254.html |
| A finalization registry split by generation; Rust-only finalizers; guardians fill queues | OCaml custom blocks; Chez guardians | https://openjdk.org/jeps/421; https://wingolog.org/archives/2024/07/22/finalizers-guardians-phantom-references-et-cetera |
| Young and old lists in the large-object space | MMTk's treadmill (`alloc_nursery`, `collect_nursery`) | https://github.com/mmtk/mmtk-core (`src/util/treadmill.rs`) |
| Identity-hash state in side metadata, extension word on move | Bacon, Fink, Grove (ECOOP 2002); JDK compact headers | https://openjdk.org/jeps/450 (experimental); https://openjdk.org/jeps/519 (product); https://wiki.openjdk.org/display/lilliput/Main |
| JIT values published as tagged values to VM frames before every non-`Leaf` call; no native stack maps | V8 Sparkplug; SpiderMonkey Baseline; the Guile 3 JIT | https://v8.dev/blog/sparkplug; Cranelift `cranelift/codegen/src/ir/user_stack_maps.rs:6-13`; https://fitzgen.com/2024/09/10/new-stack-maps-for-wasmtime.html |
| Two helper classes, `Leaf` and `Transfer` | V8 Sparkplug, SpiderMonkey Baseline (runtime calls resume at bytecode resume points) | https://v8.dev/blog/sparkplug |
| A minor-GC stack watermark as a strided return barrier | JEP 376; Loom return barriers; OCaml `Already_scanned` | https://openjdk.org/jeps/376; https://openjdk.org/jeps/444 |
| Poll folded into the stack limit; owner-only reset with a re-check | V8 `StackGuard`; OCaml `young_limit`; Lin et al. on yieldpoint costs | V8 `src/execution/stack-guard.cc:38,169`; OCaml `runtime/domain.c:387-396,2022-2058`; https://doi.org/10.1145/2754169.2754187 |
| A deterministic tick quantum for preemption | Chez `%trap` | Chez `s/cmacros.ss:2119`, `s/library.ss:1194-1240` |
| Triggers in bytes plus external bytes; whole-heap interval 2·L; √L majors only beside a nursery; MemBalancer opt-in | Chez phantom bytes; OCaml custom memory; Racket CS; Kirisame, Shenoy, Panchekha | Racket `racket/src/cs/rumble/memory.ss:35-56,141-160`; https://arxiv.org/abs/2204.10455 |
| Decommit with hysteresis, in coalesced runs, after the pause | Chez `heap-reserve-ratio` | Chez `c/segment.c:460-484` |
| Continuations as immutable traced snapshot objects (design A) | Larceny's stack-cache flush; Gambit; Chez (for C′) | Hieb, Dybvig, Bruggeman, PLDI 1990; Chez `c/schsig.c:58-150` |
| Fast paths as data; static selection; spaces composed into plans; a null collector as lower bound | Whippet `api/gc-attrs.h`; MMTk plans; JEP 318 | https://openjdk.org/jeps/318; https://openjdk.org/jeps/474 |
| One declarative layout specification generates every traversal | Chez `s/mkgc.ss` (fused collection and accounting made "a GC with accounting about twice as fast" in Racket CS) | Chez `s/mkgc.ss:1-118`; https://github.com/cisco/ChezScheme/commit/37a515ca274e5f16510c1608ff37a9dae58ebd27; https://github.com/racket/racket/commit/282ec8125afe4ef05135e440ec72ee7d0f9d6f8d |
| A context that cannot collect; collection only through the driver's `&mut Heap`; a `'gc` brand; root scopes and owned handles only at boundaries | gc-arena; Wasmtime `RootScope`/`OwnedRooted`; Nova's `NoGcScope` | https://docs.wasmtime.dev; Wasmtime `crates/wasmtime/src/runtime/gc/enabled/rooting.rs:255-299,1379-1409`; https://trynova.dev/blog/garbage-collection-is-contrarian |
| The mutator is the carrier, not the green thread; protocols written for N mutators; blocking calls leave the mutator | Go's per-P `mcache`; Loom carrier threads; OCaml 5 domains; Chez thread contexts | https://go.dev/src/runtime/mcache.go; https://openjdk.org/jeps/444; https://arxiv.org/abs/2004.11663; Chez `c/thread.c` (`Sdeactivate_thread`) |
| SRFI 18 objects on one shared heap; threads blocked for ever rooted until they terminate | SRFI 18; Gambit (thread groups hold every non-terminated thread); chibi (a global list of blocked threads) | https://srfi.schemers.org/srfi-18/srfi-18.html; Gambit `lib/_thread#.scm:1346`, `lib/_thread.scm:1649`; chibi `lib/srfi/18/threads.c:165-206` |
| A bespoke in-tree engine with MMTk-shaped seams | MMTk's binding decomposition | https://github.com/mmtk/mmtk-core |
| Patina's own code reservation with `MAP_JIT`; W^X toggled in one install function | — | Cranelift `cranelift/jit/src/memory/system.rs`, `cranelift/jit/src/backend.rs:164,206` |
| No incremental or concurrent marking now; an SATB arm reserved for slow paths | Racket CS; LXR | https://arxiv.org/abs/2112.07880 |
| Parallel stop-the-world marking on per-heap GC workers; evacuation destinations chosen sequentially | HotSpot Parallel and G1; MMTk work packets; Whippet's work-stealing tracer | Whippet `doc/collector-mmc.md`; https://wingolog.org/archives/2025/07/08/guile-lab-notebook-on-the-move |

---

## 1. Thesis and key bets

Build first the common core every candidate design needs: a stable block heap; headers and inline payloads with no
Rust `Drop`; byte pacing; a slot-based root contract; a context type in place of `Rc<RefCell<Heap>>`, with
collection reachable only from the driver; global cells; continuations as heap objects; traced code liveness. Every
stage ships measured value or stays neutral within its gate, and no stage removes a safety property (an `Rc` count, a
deferral guard, a cleared dead slot) before its replacement exists. The first production collector is the plainest
that removes the measured costs: **non-moving, whole-heap mark-region** with bump allocation and a metadata-only lazy
sweep. Generations and evacuation come later on the same space, metadata byte, barrier and JIT ABI, each a per-heap
switch turned on by a pre-registered measurement. The JIT-facing contract is designed now and frozen after a
throwaway Cranelift spike.

| # | Bet | If wrong |
|---|---|---|
| B1 | **Representation dominates**; a copying nursery is second-order on Patina's bimodal survival | K9 stops after stage 5 and re-plans; K4 builds the copying nursery |
| B2 | **Allocation never collects**, through the JIT era | K16 bounds the windows that cannot collect and adds a collection point at the offending site; making allocation collect is not the fallback (230–890 Rust functions hold values across allocation) |
| B3 | **A granule field-logging barrier costs under 1% in the interpreter and at most 2% in JIT code** | K2: the card plus young-filter runner-up |
| B4 | **Fragmentation is containable without moving** until evacuation lands | K3 pulls evacuation forward |
| B5 | **No tier needs Cranelift stack maps or raw slots in published frames** | K12: more `Leaf` helpers and inlining; native maps only through a separately approved design |
| B6 | **Self-tagged flonums pay** (≥ 70% immediate on float code; fewer bytes and cycles with inline flonum operations in both arms; ≤ 1% elsewhere) | K6 reverts to 16 B boxes before the ABI freeze |

---

## 2. Value encoding

`Word` (today `TaggedValue`) stays `#[repr(transparent)] u64` and `Copy`.

```
 low bits      meaning                         decode / layout
 ...xxxx 000   fixnum, 61-bit signed           n = w >> 3 (arithmetic)
 ...xxxx 001   immediate (specials, chars,     low byte = sub << 3 | 001; payload in bits 8–63
               object headers)
 ...sxxx 010   flonum, self-tagged, band A     see below; bit 3 is the sign
 ...sxxx 011   flonum, self-tagged, band B
 ...a    1xx   HEAP REFERENCE (bit 2 set)      4-bit tag k = w & 15, address = w − k (16 B aligned)
```

| `w & 15` | Kind | Word 0 of the object | Field *i* load |
|---|---|---|---|
| `0100` | pair (headerless) | car | car `[w−4]`, cdr `[w+4]` |
| `0101` | procedure (headerless) | code-descriptor reference (a `1111` value) | free variable *i* at `[w+3+8i]` |
| `0110` | vector | header | element *i* at `[w+2+8i]` |
| `0111` | record (headerless) | record-type reference | field *i* at `[w+1+8i]` |
| `1100` | string (pointer-free) | header | UTF-32 unit *i* at `[w−4+4i]` |
| `1101` | symbol (immortal) | header | — |
| `1110` | bytevector (pointer-free) | header | byte *i* at `[w−6+i]` |
| `1111` | other headered object | header | word *k* at `[w−15+8k]` |

- **Fixnum `000`** lets a JIT add tagged words with `adds`/`b.vs`; a tagged index is a byte offset for 8-byte
  elements. **`#f` is `0x01`**, so `if` is one compare; today's specials keep their bit patterns
  (`crates/patina-core/src/tagged_value.rs:91-110` [S]).
- **One exact heap bit:** the barrier's value filter is one `tbz w, #2`, removing 64% of heap stores before any
  metadata load [P, store-mix census]; visitors test the same bit. Seven kinds get primary tags, so their type tests
  need no header load (calls are 10–31% of dispatches [P]). 16 B alignment frees the fourth tag bit (Chez's rule).
- **Code references are values** (`1111` references to code descriptors): code liveness is ordinary marking.
- The aarch64 sequences these choices give (4 instructions for a checked `car`, 10 for `vector-ref`, a call in 3
  plus 2 dependent loads) are in S6's issue, which the spike confirms.

**Flonums.** With `raw = f64::to_bits(d)` and `K = 1 << 60`: `encode(d) = rotate_left(raw − K, 4)`, immediate iff
`(w & 7) ∈ {0b010, 0b011}`; `decode(w) = rotate_right(w, 4) + K`. Exponent tops `011` and `100` land on tags `010` and
`011`, so every double with |d| in [2⁻²⁵⁵, 2²⁵⁷), of either sign, is immediate (Melançon, Serrano and Feeley's
three-band variant minus the ±0/subnormal band, which keeps the heap bit exact). Other doubles are 16 B boxes: ±0.0,
±∞ and the canonical NaN use **immortal canonical boxes**, so producing them never allocates. The encoding is
canonical, so `eqv?` is a bit compare for immediates. Observable changes (`eq?` on equal in-band flonums; SRFI 69 `eq?`
tables finding equal flonum keys; flonum-keyed ephemerons never breaking) are checked against the oracles in stage
5f and recorded in `DIVERGENCES.tsv` where they differ. The encoding is judged only with type-specialised flonum
arithmetic in both arms, because generic numeric dispatch is about 51% of fibfp's samples [P] (K6).

**Immediates (`001`).**

| Low byte | Value | Notes |
|---|---|---|
| `0x01`, `0x09`, `0x11`, `0x19`, `0x21` | `#f`, `#t`, `()`, eof, unspecified | unchanged |
| `0x29` | default-object | |
| `0x31` | `BWP`, a broken ephemeron's key and value | never reaches Scheme: `ephemeron-key` and `ephemeron-datum` answer `#f` for it (`crates/patina-tests/tests/ephemerons.rs:29-30` [S]) |
| `0x39` | `UNBOUND`, a global-cell placeholder | never reaches Scheme |
| `0x41` | char | `(cp << 8) \| 0x41`; moves here from today's tag `010` |
| `0x81` | object header | `len << 16 \| type << 8 \| 0x81`; never a value |
| `0xD9` | `DEAD_SLOT` | fill for dead register slots in the debug-poison and zeal lanes (§8.1) |
| `0xE9` | `FWD_CHECK` | debug check word written into evacuated objects |
| `0xF1` | today's `FORWARDED` import marker | deleted with global cells (stage 4b) |
| `0xF9` | `GC_POISON` | debug and torture poison (exists today) |

**Forwarding** state lives in the side metadata byte (`FORWARDED`, §4), not in object words. An evacuated object's
word 0 holds the new untagged address, and a visitor rewrites a slot as `new | (old & 15)`, identically for
headerless and headered objects. Stages 5b–5e decode a transitional tag map while objects leave the arenas (S5).

---

## 3. Object model

**Header** (headered kinds only): `length / payload (48 bits) | type (8) | 0x81`.
- A header reads as an immediate, so a stray header is never taken for a reference.
- Headers are written once, by the constructor. **No GC state and no mutable flags live in a header**: mark, end,
  log, hash and forwarding live in side metadata, so the mutator never read-modify-writes a word the collector also
  writes. Immutability variants use distinct type codes (`T_VECTOR` = 1, `T_VECTOR_IMM` = 2, both below 32).

**Invariant W (value-only words).** Every word of every object the barrier can log, and every word of a captured
continuation, is a value or has low bits `000` (reads as a fixnum): headers are immediates, descriptor and record-type
references are values, constructors write padding as fixnum 0. A minor re-reads both words of a logged granule as
values (§7), so W makes granule logging sound; capture clears dead slots and no tier keeps raw slots in a published
frame (§8.2), so the core traces a continuation word by word without parsing frames (§10). A `CodeDesc`'s raw fields
are declared raw in the layout specification, never scanned, and never a barrier target.

**Layouts.** Sizes round up to 16 B; "pf" means pointer-free.

| Kind | Tag | Words | Bytes; space |
|---|---|---|---|
| pair | `0100` | car, cdr | 16 |
| procedure: VM closure | `0101` | descriptor ref, fv₀…fvₙ₋₁ | 8+8n |
| procedure: primitive | `0101` | descriptor ref (its primitive descriptor), pad | 16; immortal, with its descriptor |
| procedure: parameter | `0101` | descriptor ref (PARAM), converter, value vector (transitional shallow binding); from stage 9: descriptor ref, converter, global-value cell | 32 |
| procedure: continuation | `0101` | descriptor ref (CONT_INVOKE), `CONT` object | 16 |
| procedure: tree-walker lambda | `0101` | descriptor ref (TW_LAMBDA), host id (fixnum) | 16 |
| vector | `0110` | header(len), e₀… | 8+8n |
| record | `0111` | record-type reference, f₀…fₙ₋₁ | 8+8n |
| string | `1100` | header(len), UTF-32 units | 8+4n, pf |
| symbol | `1101` | header(len), hash (fixnum), UTF-8 bytes | 16+len, pf; immortal |
| bytevector | `1110` | header(len), bytes | 8+n, pf |
| flonum box | `1111` | header, f64 | 16, pf |
| bignum | `1111` | header(limbs; sign by type code), u64 limbs | 8+8n, pf |
| ratnum / complex | `1111` | header, num, den / re, im | 32 |
| cell (box, `MutableCell`) | `1111` | header, value | 16 |
| record type | `1111` | header, name, parent, uid, field names, nfields (fixnum), flags (fixnum), pointer mask | 64; descriptor space |
| identifier | `1111` | header(scope-set id in the length bits), symbol, source id (fixnum) | 32 |
| ephemeron | `1111` | header, key, value (reported through `SlotVisitor::ephemeron`, never as two strong slots), GC link (collector-private: fixnum 0 outside a collection, never traced) | 32 |
| promise | `1111` | header, box → `[hdr PBOX][done (fixnum)][value-or-thunk]` (SRFI 45 sharing) | 16 + 32 |
| values | `1111` | header(n), v₀… | 8+8n |
| condition | `1111` | header(kind), message, irritants, extra | 32 |
| port | `1111` | header, port id (fixnum) | 16 |
| host handle | `1111` | header(kind), host id (fixnum), optional literal vector | 16–32 |
| environment specifier | `1111` | header, namespace id, flags | 32 |
| thread (SRFI 18) | `1111` (`T_THREAD`) | header, thread id and state (fixnum: new, runnable, blocked, terminated; id −1 when no `GreenThread` exists), name, specific, thunk (until started) or result, end exception, joiners (a waiter queue) | 64 |
| mutex | `1111` (`T_MUTEX`) | header, name, specific, owner (a thread, or a state immediate: not owned, abandoned, not abandoned), waiters | 48 |
| condition variable | `1111` (`T_CONDVAR`) | header, name, specific, waiters | 32 |
| time (SRFI 18) | `1111` | header, f64 seconds | 16, pf |
| continuation (design A) | `1111` | header(words), meta (fixnums), dynamic-state references, frame words | variable; LOS above 8 KiB |
| code descriptor | `1111` (`T_CODE`) | see below | 48 + 8·nconst; descriptor space (primitive descriptors: immortal) |
| global cell (never a value) | — | header, value, name symbol, namespace id + flags (fixnum; includes `WATCHED`) | 32; immortal |
| binding record (variant R only; never a value) | — | header, cell reference, name symbol, flags (fixnum; includes `WATCHED`) | 32; immortal |

Waiter queues are heap lists, written through the store funnel; a thread object's `GreenThread` (register stack,
frames) is Rust-owned in the per-heap thread table (§12).

**Code descriptor** (`T_CODE`, a heap object referenced by ordinary values): `w0` header (nconst); `w1` entry (raw:
JIT body or interpreter trampoline; the tier-up target); `w2` resume (raw: `*const ResumeTable`, return pc → resume
entry); `w3` body (raw: `*const CodeBody` with bytecode and safepoint maps, owned by the per-heap `CodeStore`); `w4`
meta (fixnum: nfree, nregs, arity, kind); `w5` unit (fixnum: unit id, generation); `w6…` constants (values, traced
and updatable; nested descriptors appear here as ordinary references). JIT code loads constant *k* with one load
through the descriptor reference it holds. Descriptors are born old, written only by initializing stores, and pushed
on the remember-whole list (§7).

**No heap object owns a Rust value.** Today 19 of 28 variants carry `Drop` and 47% of allocations carry it [P].
Rust-owned resources live in per-heap side tables reached by a `u32` id: `PortTable`, `CodeStore`, the thread table
and `HostPayloadTable` (tree-walker `CpsLambda`/`CpsContinuation`, macro bodies, libraries, future FFI objects), each
entry registered in the finalization registry (§6.7). Bignums keep inline limbs and build `num-bigint` temporaries for
arithmetic. A generated `const` assertion fails the build if any type in the layout specification needs `Drop`.

**One layout specification.** `declare_layouts!` (defined in `patina-gc`, invoked in `patina-core`) generates the
`ObjectModel` implementation (`size`, `trace` over writable slots, `copy`, `verify`), the debug printer, the datum
writer's kind view and the JIT offset table, as Chez's `mkgc.ss` does. It removes today's hazard where misfiling a
value-bearing variant as a leaf is a use-after-free rather than a compile error
(`crates/patina-core/src/heap/mod.rs:137-141` [S]).

---

## 4. Heap organisation

**One virtual-address reservation per heap**, sized from `max_heap` (data plus metadata and block table, rounded to a 4
MiB run). `max_heap` defaults to the smaller of 16 GiB and 75% of physical memory (decision 15), set by
`PATINA_HEAP_MAX`, `--heap-max` or `HeapConfig::max_heap`; reaching it raises `&heap-exhausted` (§5). It is mapped
read-write with lazy commit (`MAP_NORESERVE` on Linux), one VMA per heap, so `vm.max_map_count` is never at risk; 4,096
such reservations of 16 GiB succeed on the development machine, and one test process holds up to 318 live heaps [P].
`Drop for Heap` unmaps it, which needs stage 2's teardown fix. Memory returned is measured as resident size and
`phys_footprint`.

```
reservation:  [ metadata: 1 byte per 16 B granule ][ block table: 16 B per block ][ data: blocks + LOS page runs ]
meta_bias = meta_base − (data_base >> 4);   meta(a) = *(meta_bias + (a >> 4))        // one shift, one load
separate:     per-mutator store buffer (256 MiB VA, lazily committed, guard page at the hard end);
              per-green-thread register stack (defaults: 8 GiB main thread, 256 MiB others; §8.1);
              per-heap code reservation (MAP_JIT on macOS arm64)
```

**Spaces**, composed in the style of MMTk plans:

| Space | Holds | Allocation | Collection | Moves? |
|---|---|---|---|---|
| Small-object space (SOS) | objects up to 8 KiB | bump into holes; objects over 256 B through an overflow allocator that uses empty blocks only | mark-region; lazy sweep | no (stages 5–7); opportunistic evacuation from stage 8 |
| Large-object space (LOS) | objects over 8 KiB | 16 KiB page runs, first fit by size class; a new run joins the **young list** | young runs classified after every collection, old runs after majors; dead runs go to a recycle cache after the pause | never; buffers lent to FFI live here |
| Descriptor space | code descriptors, record types | append into dedicated blocks | marked at majors; units released by the registry | never |
| Immortal space | global cells, binding records, interned symbols, canonical flonum boxes, primitive procedures and their descriptors | append-only blocks with state `IMMORTAL` | never swept; cells are a root region at majors, re-armed then (§6.1); primitives and their descriptors reference only immortal objects, so they need no scan | never; JIT code may embed these addresses |

Granules are 16 B (56% of objects are exactly 16 B [P]); blocks 32 KiB; the metadata byte is the line table, at
granule size; the medium threshold is 256 B and the LOS threshold 8 KiB. The other parameters (recyclable threshold,
free and evacuation reserves, initial commit, decommit unit) and the 16 B block-table entry are in S5's issue.

**The metadata byte**, one per granule (6.25% of the data region):

| Bits | Field | Where it is valid | Writer |
|---|---|---|---|
| 0–2 | `STATE`: 0 = free or young-unmarked; 1, 2, 3 = rotating mark epochs; 4 = `FORWARDED`; 5–7 reserved (5 = `BUSY` for future parallel evacuation) | start granule | collector |
| 3 | `END`: last granule of a marked object | last granule | collector (marker) |
| 4 | `KEYHINT`: an ephemeron is pending on this key | start granule | collector |
| 5 | `HASHED`: an identity hash was taken | start granule | mutator, through `MetaByte` |
| 6 | `LOG`: armed; the first store into this granule must be logged | every granule | the collector arms it; the mutator barrier disarms it through `MetaByte` |
| 7 | `HASH_MOVED`: the copy carries a trailing extension granule holding the original hash | start granule | collector |

**Pinning is per block**, as in Chez's `must_mark`: a block with a non-zero pin count, or pinned this cycle, is never
an evacuation candidate. Identity hashing never pins (§6.9), so pins are rare (FFI buffers, mostly in the LOS, and
debugger event payloads).

**Decommit** has hysteresis, works on coalesced runs and never runs inside the pause. A block becomes a candidate after
2 consecutive majors empty; the heap keeps at least `(target − L) + free_reserve` of committed empty capacity; only
4 MiB-aligned runs are decommitted, data and metadata together (one metadata page covers several blocks); the work is
queued by the epilogue and done from the poll slow path after the world is released, rate-limited and logged as a
non-mutator interval. The mechanism is per OS (Linux `MADV_DONTNEED`; macOS `mmap(MAP_FIXED)` over the run, because
`MADV_FREE` alone leaves resident size unchanged there [P]). LOS runs pass through a recycle cache of up to 32 MiB.
Register stacks and store buffers follow the same rule, observed at majors and at returns to the top level, so a
program that stops allocating after a deep recursion still gives the memory back.

---

## 5. Allocation

**The contract stays: allocation never collects.** Today `note_alloc` only raises a flag
(`crates/patina-core/src/heap/mod.rs:581-584` [S]), and 230–890 Rust functions hold unrooted values across allocation
and are correct only because of this. Allocation sites are never safepoints: JIT code publishes and reloads nothing
around them, SSA values holding half-built structures survive, and initializing stores need no barrier (JEP 475's
condition). The slow path refills, overdrafts, posts events and fails only at hard limits; it never collects and never
runs Scheme. Collect-and-retry happens at the *call* of a user-sized primitive (step 6), never inside the allocator.

**Rust fast path** (interpreter, primitives, frontend):

```rust
#[inline(always)]
pub fn alloc_small(&mut self, bytes: usize /* multiple of 16, ≤ 256 */) -> NonNull<u64> {
    let m = self.mutator();
    let p = m.ap.get();
    let np = p.wrapping_add(bytes);
    if np <= m.alloc_limit.get() { m.ap.set(np); unsafe { NonNull::new_unchecked(p as *mut u64) } }
    else { self.alloc_slow(bytes, AllocKind::Small) }            // #[cold]; never collects
}
```

- The receiver is `&mut Cx` while the `Vec` arenas can relocate, `&Cx` once they are gone (stage 5e).
- Constructors take their contents and write **every word**, including padding (fixnum 0). Hole data is never
  zeroed, only a hole's metadata bytes; debug builds fill holes with `GC_POISON`.

**JIT fast path.** `ap` and `limit` sit at `Mutator` offsets 0 and 8 (MMTk's `BumpPointer` offsets) and are kept in
SSA; one `add; cmp; b.hi` covers a basic block's summed allocation (OCaml's Comballoc); tags are applied with `add`,
never `orr` (`ap` is only 16-byte aligned); the refill is a `Leaf` helper returning the new `(ap, limit)`. `NoAlloc`
helpers are the only calls across which `ap`/`limit` stay in SSA; every other call writes them back and reloads them,
a call-graph test enforces the attribute, and debug builds assert `Mutator.ap` unchanged across `NoAlloc` calls. A
conformance test checks that the emitter writes every word of every inline-allocated kind. The listing is in S6.

**Slow path** `alloc_slow(m, bytes, kind)`:
1. **Over 8 KiB:** the LOS; the run joins the young list. User-sized requests go through `try_alloc` (step 6).
2. **Over 256 B:** the overflow allocator, empty blocks only.
3. **Otherwise, lazy-sweep the current block** from its cursor: a granule whose `STATE` is the current epoch starts a
   live object (skip to its `END`); any other run is a hole, whose metadata is zeroed and which becomes `(ap, limit)`.
4. **Then** the next recyclable block, a free block, or newly committed blocks; a block whose `counted_epoch` is stale
   holds nothing live, and its metadata is cleared as it is taken, not in the pause.
5. **Accounting at refill:** `bytes_since_gc += hole_bytes`. Crossing the nursery budget or the major target posts
   `GC_MINOR` or `GC_MAJOR` as an owner post (§9); the mutator keeps allocating, and K16 bounds the windows in which
   a posted collection cannot run.
6. **User-sized requests are fallible, and retried after a collection.** `make-vector`, `make-string`,
   `make-bytevector`, string-port growth and `read-string` call `try_alloc(bytes) -> Result<_, Oom>` before they make
   any visible change, so a retry is idempotent. A primitive that gets `Oom` fails with it, and the machine treats
   that failure as `Step::CollectAndRetry`, the mechanism `EMFILE` already uses (§6.7): it runs a full major at the
   call's return pc, where the arguments are still in the suspended frame, and calls the primitive once more; only a
   second `Oom` raises `&heap-exhausted`. The primitives stay heap-only (`Leaf` in JIT code, where the fragment's
   `Transfer` path for the status does the same). Where collection is deferred (`NoGcScope`, a nested tree-walker
   trampoline) the first `Oom` raises at once. **Capture** (§10), a `Transfer` helper, does the same through
   `try_alloc` and never draws on the emergency reserve.
7. **Hard ceiling** `max_heap`: a small infallible allocation dips into a 4 MiB emergency reserve and posts
   `HEAP_EXHAUSTED`; the next poll runs a full major and raises a catchable `&heap-exhausted` if the heap is still
   over (uncaught, the CLI exits non-zero); if the reserve itself runs out before a poll, the process aborts with a
   diagnostic and a non-zero exit after flushing ports (R1).

---

## 6. Collection

All collection is stop-the-world and deterministic: single-threaded in a fixed trace order, or, under stage P, with GC
workers whose marks, broken ephemerons and addresses do not depend on the schedule (§11). It runs only at a poll (§9),
with every Scheme value in VM-managed memory, and only from code holding the driver's `&mut Heap` (§8.3). **Per-heap
policy** `{ generational, evacuation }` is a runtime field read only by slow paths: VM heaps start `{false, false}`
and measured gates flip the bits; tree-walker heaps stay `{false, false}`. The step-by-step algorithm is in S5's and
S7's issues; these are its rules.

### 6.1 Major collection

Retire every mutator's allocation buffer; **flip** the mark epoch (1 → 2 → 3 → 1, so no clearing pass); **drop** every
mutator's store buffer, range entries and remember-whole list; trace roots (pinning roots first, then every rooted
thread's frames, ignoring watermarks, and dynamic-state slots, root scopes, handles and the cells); mark (set `STATE`
to the epoch, keeping `KEYHINT | HASHED | HASH_MOVED`; set `END`; in generational mode arm `LOG` on every granule of
a pointerful object; count live granules per block under a `counted_epoch` stamp, so no pass resets the counts); run
the weak fixpoint and the epilogue (§6.6, §6.10). In generational mode a major re-derives every `LOG` bit: marking
arms what it marks, and the epilogue arms the root-region cells, which are never marked, so a cell disarmed before the
major cannot hide a later store. The **catch-up sweep** of blocks whose `swept_epoch` lags (so a stale epoch cannot
alias) and **block classification** (empty, recyclable, full; decommit candidates; dead LOS runs) run after the world
is released, in slices from the poll slow path.

### 6.2 Minor collection (sticky marks; generational heaps only)

"Old" means the start byte holds the current epoch, "young" `STATE` 0; minors do not flip the epoch. Roots: the frames
above the watermark of each thread with `ran_since_gc` (§8.1; every write into a non-running thread's frames goes
through `return_into`, which sets it); every rooted thread's dynamic-state slots, whether or not it ran; root scopes and
handles; the remember-whole lists and range entries; young host-payload registrations; machine roots. Each
store-buffer entry is a granule address whose two words are re-read as values (invariant W), their young referents
marked, and the granule re-armed; range entries are scanned whole and re-armed. Marking touches young objects only,
and **marking is promotion**: a survivor becomes old in place, so promotion cannot fail. Weak processing covers young
entries only. **Young LOS runs:** a marked run moves to the LOS's old list; an unmarked one is dead and is released into
the recycle cache after the pause, as at majors, its bytes counted in `bytes-reclaimed` (MMTk's treadmill keeps the
same nursery list). Without that, every dead young object over 8 KiB (a deep continuation capture, a large vector, a
string-port buffer) would wait for a major, up to 256 minors away. Lazy sweep then re-sweeps only blocks allocated into
since the last GC, and each thread's watermark is reset.

Sticky loses where a copying nursery pays nothing for dead young objects and where survivors stay in place (K3, K4);
the pre-planned fix is a `NurserySpace` (§11). An **adaptive bypass** with hysteresis runs majors only while minors keep
more than 30% of the budget alive, and **backstops** force a major after 256 consecutive minors or max(1 GiB, 64·L) of
nursery allocation, which bounds how long ports, code and dead-key ephemerons wait.

### 6.3 Marking order and a bounded mark stack

A pair pushes its cdr and continues with its car, so the stack is bounded by car-nesting depth rather than list length;
large objects push range entries processed 256 words per pop; past a cap the marker stops pushing, sets `RESCAN` on the
blocks of unscanned objects and later rescans them by start bytes, so no heap parsability is needed.

### 6.4 Lazy sweep

Per block, on demand (§5 step 3), at most once per cycle. It reads only metadata, writes only the holes it returns,
never reads a dead object and never runs a destructor. The debug-poison and zeal lanes add an eager poisoning sweep
and a quarantine (§14), since lazy sweep no longer poisons freed slots.

### 6.5 Evacuation and pinning (stage 8)

Evacuation starts at a major when the previous cycle's **fragmentation** (free granules not in holes of at least
256 B, over committed SOS granules, measured after marking; also K3's metric) exceeds 10%, and stops below 5% (Whippet
`mmc.c:656-715`). Candidates are SOS blocks under ¾ live with no pin (Chez/Racket CS's `use_marks` rule); pinning roots
are traced first. A reachable object in a candidate is copied once: its start byte becomes `FORWARDED`, its word 0 the
new address, and a `HASHED` object gets its extension granule (§6.9); when the reserve runs out, the rest are marked in
place. The LOS, the descriptor and immortal spaces, pinned blocks and tree-walker heaps never move. The `move-all`
torture mode makes every unpinned block a candidate and poisons from-space.

### 6.6 Weak references and ephemerons, with no O(n²) rescans

- **Key liveness.** `is_live(K)` holds when K is an immediate (fixnum, char, in-band flonum, special), an object of the
  immortal space (symbol, canonical flonum box, cell, binding record, primitive or primitive descriptor), already
  marked, or, in a minor, old (descriptor-space objects included). Symbol-keyed ephemerons therefore never break, as
  today, where symbols are re-marked at every collection (`crates/patina-core/src/heap/gc.rs:449-465` [S]).
- **Key-indexed resolution** (Whippet `gc-ephemeron.c`). The generated trace reports an ephemeron only through
  `SlotVisitor::ephemeron` (§11). An ephemeron whose key is not live is chained onto `pending[K]` through its link word
  and `KEYHINT` is set on K; whenever the tracer marks (or copies) an object with `KEYHINT`, it pops the waiters and
  traces their values. The 16 K chain that costs 289 ms in the bad order today becomes linear [P].
- **Generational rule: an ephemeron is never older than its key or value.** Ephemerons are allocated only young, never
  in the LOS or immortal space, and promotion is age-monotone, so a minor examines only young ephemerons and treats
  old and immediate keys as live; no remembered-set edge exists for ephemerons.
- **`(gc)` is always a full major and collects at its call.** It is a `Transfer` primitive returning
  `Step::Collect(Major)`: the machine delivers its result, then collects before the caller's next instruction, with the
  caller suspended at a mapped return pc (§9). A `(gc)` that only posted would let the next primitive in the same frame
  run first once the per-instruction poll goes, and three #423 tests
  (`crates/patina-tests/tests/ephemerons.rs:94,109,127` [S]) would answer `#f`; the same tests require a dead key of any
  age to break after one `(gc)` (`ephemerons.rs:23,52,213,226` [S]). Inside `NoGcScope`, `(gc)` posts and counts a
  deferred poll.
- **Breaking** writes `BWP` into key and value (§2). **One fixpoint** covers host payloads, ephemerons and, later,
  SRFI 254 guardians: sequencing weak kinds separately was a use-after-free (commit `1d18c49`). `reference-barrier`
  stays an opaque use in JIT code (a `Leaf`, `NoAlloc` identity helper).

### 6.7 Finalization and ports (R1–R6)

**Registry** (its interface is part of the contract, §11): `FinalRegistry { young, old }` of entries
`{ obj, kind: Port | CodeUnit | HostPayload | Thread | Foreign, id }`, created only by slow-path constructors. After a
minor, a young entry whose object was marked or forwarded moves to `old`, and one **neither forwarded nor marked** is
queued, which is correct under sticky marking, in-place promotion and copying alike; after a major, unmarked old
entries are queued too. A runtime may also queue an entry whose object it knows is finished (a terminated thread,
§12). Queued entries run in id order **after the pause and before Scheme resumes**, in the poll slow path, as
non-mutator intervals, Rust-only (no allocation, no Scheme): ports flush, close and free their `PortTable` slot,
`MemoryFs` writers commit, threads release their register stacks.

- **R1.** `end_process` and `emergency-exit` flush every open file port of every heap in the process, through a
  process-wide weak registry of `PortTable`s (today's `OUTPUT_FILES` covers every heap on the thread,
  `crates/patina-core/src/port.rs:173,212-233` [S]).
- **R2.** A collection that proves a file port dead flushes and closes it before Scheme resumes.
- **R3.** Each open, unclosed file port charges 8 KiB of external bytes; opens minus closes since the last major
  reaching min(128, `RLIMIT_NOFILE`/4) posts a major; on `EMFILE`/`ENFILE` the open primitive returns
  `Step::CollectAndRetry` and the machine collects at the call's return pc and resumes it once, as chibi does
  (`eval.c:1300-1323`). The loader's opens at points A and B retry too; opens where collection is deferred still fail,
  a documented limit.
- **R4.** Finalization runs no Scheme and allocates nothing. **R5.** Dropping the heap finalizes every entry.
  **R6.** A port is one canonical object, and the `current-*-port` parameters hold it.

GC-time flushing is observable, so the tests that pin R1–R6 live outside the byte-identical differential lane.

### 6.8 Code liveness, including the eager-release contract

Code is released per unit (a top-level form and the lambdas compiled with it). Edges into a unit are ordinary
references to its descriptors (procedure word 0, frame `code` words in live stacks and captured continuations, nested
descriptors in constants), so a unit is live if any of its descriptors is marked; `live_closures`,
`gc_freed_closure_code_ids` and `RETIRED_VM_CLOSURE_CODE` go. Until stage 4e, `Rc` counts keep units loaded. The
`CodeStore` references descriptors weakly.

**The eager-release contract (#338/#352).** `crates/patina-tests/tests/finished_forms_release_code.rs:72-99` [S] runs
2,000 forms **with no collection** and requires each form's code gone, its slots reused, as soon as the form finishes.
So each unit carries an **`escaped` bit**, set whenever a reference to one of its descriptors can outlive the form:
`MakeClosure` of a member; a capture over a frame of the unit; a `Step::Eval` closure; and any other holder (debugger
breakpoint or hook, profiler sample, tracer snapshot, JIT inline cache), which obtains descriptor references only
through `CodeStore::escape`. A finished form whose unit never escaped is released at once; debug builds poison its
descriptors' raw fields and assert that nothing references it. An escaped unit waits for a complete major. Inline
caches hold traced descriptor references (strong, or weak and cleared at epilogue step 5), so a reused address never
matches a stale entry.

**JIT bodies** go with their unit and are freed by epoch: tier-up and invalidation are `Transfer` helpers, so no
native activation resumes a replaced body; a replaced body is freed only when the driver sees no JIT activation on the
native stack, after the fix-up walk (§10) has re-derived the cached return addresses in every green thread's stack;
freeing is batched with W^X toggled once. The zeal mode `jit-invalidate` invalidates every body at every poll.

### 6.9 Identity hash

```
identity-hash(v):
  immediate                → mix64(bits)
  symbol                   → stored hash
  HASHED clear             → set HASHED (MetaByte::fetch_or); return mix64((addr − heap_base) ^ seed)
  HASHED, not HASH_MOVED   → mix64((addr − heap_base) ^ seed)
  HASH_MOVED               → the u64 in the trailing extension granule
```

Hashing relative to the heap base makes it identical across runs despite ASLR. **No pinning:** evacuation copies a
`HASHED` object into `size + 16`, stores the original hash in the extension granule and sets `HASH_MOVED`
(Bacon–Fink–Grove, as in JDK compact headers), which works for headerless pairs and procedures and costs nothing for
objects never hashed. SRFI 69's stored hashes (`lib/srfi/69/srfi-69-impl.scm:118-120` [S]) stay valid; `equal-hash`'s
heap-index fallback becomes `identity-hash`.

### 6.10 The epilogue, in a fixed order

This order is a contract obligation of every collector (§11); minors restrict every step to young entries.

1. Trace to completion.
2. **One fixpoint:** newly reached host payloads → ephemerons resolved by key → (later) guardians, which resurrect
   *before any breaking*.
3. Break the remaining pending ephemerons: key and value become `BWP`.
4. (Later) SRFI 254 transport cells.
5. Weak-key tables and weak inline-cache entries: rekey forwarded keys, drop dead ones.
6. Prune the weak-id and host-payload tables.
7. Classify the finalization registry (§6.7) and queue the dead.
8. Code release (majors only).
9. In generational mode, re-arm every pointerful root-region object (majors). Queue the post-pause work: dead LOS runs
   (after every collection: young runs after minors, young and old after majors) and, after majors, the catch-up
   sweep, block classification and decommit candidates.
10. Statistics and pacing: L is the marked bytes plus LOS and external bytes; `bytes-reclaimed` includes released LOS
    runs.
11. Release the world. Queued finalizers run at the poll before Scheme resumes; post-pause work runs in rate-limited
    slices at later polls. All of it is logged as non-mutator intervals.

### 6.11 Pause budgets

Both kinds of pause have a budget of one form: a constant plus terms in work the program controls, and **no term in
dead objects, committed heap size or the arena high-water mark**. Decision 6 fixes the constants. Stage 5a's mark
microbenchmark (records, closures, vectors, deep stacks) measures a, b and c; stage 7b extends it with store-buffer
entries to measure c_min and d.

| Pause | Budget | Constants |
|---|---|---|
| Major | c + a·(frames in all rooted stacks) + b·(live MiB) | c ≤ 1 ms; a ≤ 15 ms per million frames (≈13 ns per frame with its registers [P, deeprec]); b ≤ 0.35 ms per live MiB single-threaded (3–8 ns per object at the 26.9 B mean), divided by the worker count under stage P |
| Minor | c_min + a·(frames above the watermarks) + d·(logged granules) + b·(survivor MiB) | c_min ≤ 0.25 ms; a and b as for majors, each rooted thread's dynamic-state slots counting as one frame and young LOS survivors as survivor bytes; d ≤ 10 ns per logged granule, the logged granules being the store buffers' exact entries plus the granules covered by range entries and remember-whole objects |

- **The frames terms have no limit.** A recursion that descends a million frames without allocating and then allocates
  leaves them all above the watermark: about 13 ms for one minor. Forcing a minor whenever the stack grows is rejected
  (it would add 10³–10⁴ minors to deeprec); the budget states the cost, and the `deep-descent` and `deep-unwind`
  probes measure it.
- **The logged-granules term is bounded by the store buffer's soft limit, derived from d:** about 1.25 ms / d, rounded
  to a power of two, so 128 K granules (1 MiB of exact entries) at d = 10 ns; stage 7b sets it from the measured d. A
  range entry or remember-whole object whose granules would pass the remaining capacity is not logged, and the next
  collection is forced major instead, which is sound because a major re-derives every bit.
- **The survivor term** is bounded by the nursery budget plus overdraft (4 MiB, about 1.4 ms), except that a large young
  LOS object that survives is charged like any survivor.
- **Expected values** [I]: a per-workload max minor pause of at most 2 ms on the GBS; majors of about 5–15 ms at the
  measured maximum of 50 MB live; at 1 GiB live, about 40 M objects (2³⁰ / 26.9 B), so 120–320 ms single-threaded,
  inside b's budget but long, which is why stage P is budgeted. Today's worst pauses (queue3's 41 ms, mostly sweep;
  178 ms after a library load, mostly `Drop` and provenance [P]) are made of terms this design removes from the pause.
  Per-workload estimates are in S5 and S7.

---

## 7. Write barrier

**Kind.** Pre-write **field logging** at granule granularity, with armed-bit polarity: fresh memory is unarmed, so
stores into young holders fall through; the first store into an armed granule of an old object logs that granule's
exact address into a per-mutator sequential store buffer (SSB) and disarms it. It needs no heap parsability (pairs
have no header) and deduplicates by construction (slow paths per run in simulation: nboyer 5, hashtab 131 K [P]).

**aarch64 fast path.** Holder `x0` (tagged), value `x1`, field at byte offset `OFF`, tag `T`; `x20` = `meta_bias`,
loaded once per fragment from `[x21, #0x10]` as a hoistable `readonly` load.

```
      add   x2, x0, #(OFF − T)        ; slot address (the store needs it anyway)
      tbz   x1, #2, 1f                ; [1] immediate value → no barrier
      lsr   x3, x2, #4                ; [2] granule index
      ldrb  w4, [x20, x3]             ; [3] metadata byte
      tbnz  w4, #6, log_cold          ; [4] LOG armed → cold block
1:    str   x1, [x2]
```

Four instructions for heap values, one for immediates; `vector-set!` with a variable index costs the same, and two
fields in one granule share one entry. The cold block disarms the granule (`ldclrb` under `threaded`), appends the
granule address at `remset_cur`, and at the soft limit makes an owner post of `GC_MINOR` and zeroes `reg_limit`. It is
call-free for register pressure (no tier declares stack maps, §8.2); the spike measures a `PreserveAll` stub against
it (S6).

**Store buffer.** 256 MiB of reserved VA per mutator, lazily committed, with a guard page at the hard end (the Rust twin
checks the end explicitly). Its **soft limit is derived from the minor-pause budget** (§6.11): about 1.25 ms / d logged
granules, 128 K at d = 10 ns, set by stage 7b from the measured d; reaching it posts a minor. **Bulk stores**
(`store_range`, `fill_range`: `vector-fill!`, `vector-copy!`, string-port growth into an old vector) log one range
entry and disarm its granules with one metadata `memset`. Range entries and remember-whole objects count their granules
against the soft limit; one that would pass the remaining capacity is not logged, and the next collection is forced
major. If the poll slow path finds the buffer past its soft limit while collection is deferred (`NoGcScope`, the
`HEAP_EXHAUSTED` overdraft), it **discards** the buffer and forces the next collection to be a major, which is sound
because a major re-derives every bit (§6.1).

**Filters, in order:** static elision (literal immediates; results of boolean-producing operations, never arithmetic,
which can overflow to bignums; initializing stores into objects allocated with no safepoint since); the dynamic value
filter (bit 2); a young holder (unarmed: 99.4% of the median workload's stores [P]); a granule already logged this
cycle (disarmed).

**Arming.** Marking arms every granule of every pointerful object it marks, in generational mode only; a generational
major arms the root-region cells, which marking never reaches; minors re-arm what they process; sweep clears freed
granules. Objects born old are exactly those made by `alloc_old` (descriptors, cells, binding records, record types):
created unarmed and pushed on the **remember-whole list**, which the next minor scans whole before arming them (G1's
compensation). Immortal objects that reference nothing mortal (symbols, canonical boxes, primitives and their
descriptors) need neither. Every other allocation, LOS and overflow-allocator objects included, is born young.

**No barrier on** strings, bytevectors and bignums (pointer-free); register-stack and frame stores (roots, covered by
the watermark); continuation objects (immutable after capture); descriptors after creation. A heap whose policy is not
generational reports `BarrierKind::None`: JIT code emits nothing, and the Rust funnel's branch is never taken.

**`WATCHED` cells.** A cell's flags word carries `WATCHED` once JIT code depends on its value. Every store into a cell
tests that bit after the barrier; when set, the cold path is a `Transfer` helper that invalidates the dependent bodies
and deoptimizes the executing fragment before stale code can observe the new value (§10). Until a JIT exists no cell is
`WATCHED`, and the interpreter guards its inline primitive sites per binding (decision 2).

**The Rust store funnel** is the only way to write a value into a heap object:

```rust
impl<'gc> Cx<'gc> {
    #[inline(always)]
    pub fn store<S: SlotOf<'gc>>(&mut self, slot: S, v: Value<'gc>) {
        let a = slot.addr();                               // PairCar, PairCdr, VectorElem (bounds-checked),
        if v.raw() & 0b100 != 0 {                          // CellValue, RecordField, ClosureFree (fix-up only),
            let m = self.mutator().meta(a);                // GlobalCellValue, PromiseBox, ParamValue
            if m.load() & LOG != 0 { self.log_granule_cold(a, m) }  // #[cold]: the twin of the JIT slow path
        }
        unsafe { HeapSlot::at(self.base(), a).store(v.raw()) }      // plain; relaxed AtomicU64 under `threaded`
        if S::IS_CELL { self.check_watched(slot) }         // #[cold] Transfer path when WATCHED (JIT era)
    }
    pub fn store_range(&mut self, v: Vector<'gc>, start: usize, src: impl ExactSizeIterator<Item = Value<'gc>>);
    pub fn fill_range(&mut self, v: Vector<'gc>, start: usize, len: usize, x: Value<'gc>);  // one range entry
}
/// Initializing stores: only through a token that holds the mutator borrow, so no safepoint can intervene,
/// and only with values of the same heap.
pub struct Fresh<'m, 'gc> { obj: Value<'gc>, _m: PhantomData<&'m mut Mutator> }
impl<'m, 'gc> Fresh<'m, 'gc> { pub fn init(&mut self, word: u32, v: Value<'gc>); pub fn finish(self) -> Value<'gc>; }
```

`HeapSlot::at` derives its pointer from the per-heap base (`base.with_addr(a)`), preserving provenance. Debug builds
assert in `Fresh::init` that the holder is young or pre-logged. `Heap::vector_slice_mut` (one non-test caller, the VM
`VectorSet` arm [S]), `get_string_chars_mut` and `get_bytevector_mut` are deleted, and records, parameters and
promises, stored today through cloned `Rc` handles that bypass the heap
(`crates/patina-primitives/src/primitives/records.rs:262`, `parameters.rs:168,267`, `lazy.rs:134` [S]), are written
through the funnel.

---

## 8. Roots and rooting

### 8.1 VM frames and the stack watermark

**The register stack never relocates.** Each green thread owns one reservation, committed on demand: 8 GiB for the
main thread (about 60 M frames, so today's 10 M-frame recursion still runs [P]) and 256 MiB for others, set by
`PATINA_STACK_MAX`, `--stack-max` and `HeapConfig` (decision 15). Within 1 MiB of the cap the frame-entry check raises
a catchable `&stack-exhausted`, the last MiB being the handler's room; uncaught, it exits non-zero.

Frames are **interleaved and position-independent**, with one header for the interpreter, JIT code and captured
continuations:

```
fp+0   ret      raw     cached resume address in the caller (JIT); 0 in interpreter frames; 0 in captured copies
fp+8   link     fixnum  caller distance in bytes (fp − caller_fp) [3–34] | nregs [35–50] | flags [51–62]
                        (stub kind; WM = "the return out of this frame crosses the watermark")
fp+16  code     value   reference to the code descriptor
fp+24  meta     fixnum  pc of the last suspension point [3–34] | return_reg (result register in the caller) [35–50]
fp+32  closure  value
fp+40  r0 … rₙ₋₁ tagged values only, in every tier, at every suspension point (§8.2)
```

The header is 40 B, today's `CallFrame` size; `closure` becomes a traced value. The interpreter keeps its calling
protocol (`return_reg` in the callee's frame); prompts, handlers and winds record byte offsets from the stack base.
The JIT delivers a result as the first `CallConv::Tail` argument of the caller's resume entry (`x2` on aarch64, `rdi`
on x64 [S]).

**Frame invariants.**
1. **Window initialization.** Every frame push writes an immediate into every non-parameter slot of its window, stub
   frames included, so a window never holds a stale word from a popped frame.
2. **Maps at every suspension point:** the return pc of every call (including `CallPrimitive` and every inline
   primitive opcode, whose slow path or shadow deoptimization calls from that pc,
   `crates/patina-vm/src/runtime/control.rs:3467-3489` [S]); the pc after every instruction that can raise
   (`raise_step_stub` stays over the erroring frame while handler code runs, collects and captures,
   `control.rs:909-978` [S]); pc 0, where frame-entry polls are serviced; every `Transfer` helper site. The table is
   therefore **dense**: a `pc → u16` map index per body (`0xFFFF` = never a suspension point) into deduplicated
   bitsets, with O(1) lookup.
3. **Clearing, not skipping.** The VM's root provider visits each live slot through the map, plus `code` and
   `closure`, and **overwrites each dead slot** with `UNSPECIFIED` (`DEAD_SLOT` in the debug-poison and zeal lanes):
   today's `retire_registers` folded into the scan. A wrong map can then retain garbage, which the #423 tests catch,
   but never expose a freed referent. Stub frames carry an all-live map; a frame at a `0xFFFF` pc panics in debug
   builds and has every slot visited in release builds; debug builds assert in `reg_at` that no instruction reads a
   `DEAD_SLOT`; capture clears dead slots in the copy too (§10).
4. **Other readers of raw registers** (the `StepTracer`, which reports registers as roots,
   `crates/patina-vm/src/tracer.rs:269-286` [S]; watchpoints; debugger hooks; `--dump`) read them only at suspension
   points, where 1 and 3 make every word a value or an immediate (the datum writer renders `DEAD_SLOT` as `#<dead>`).
   Per instruction that holds only in the interpreter,
   which is why attached hooks pin the interpreter tier (§8.2).
5. **Verifier:** every frame pc reached during a scan or a capture has a map, and every visited register decodes to
   an immediate or a start granule.

**The stack watermark (generational heaps)** is a return barrier. After a minor every frame below the executing frame
is clean, so its caller becomes the watermark: the collector sets the `WM` flag in that frame's `link` word
(interpreter) or swaps its `ret` for a trampoline, saving the real target in the thread **as `(code, pc)`**, never as
a raw address (JIT). Every `Return` reads both words anyway, so normal returns pay nothing; the cold path lowers the
watermark by a stride of k frames (k = 64–256, tuned in stage 7), so deep allocating unwinds do not take it on every
return. A tail call copies `link` and `ret`. **Every write into, or activation of, a suspended frame** (value delivery,
`ResumeWindJump`, raise stubs, abort landing, delimited append, scheduler deliveries, debugger writes) goes through
`return_into(frame)`, which lowers the watermark first and sets the owning thread's `ran_since_gc`; reinstating a
continuation resets the watermark to the stack base. A minor scans only frames above the watermark of threads that
ran, which removes deeprec's per-minor scan of 11.5 M registers [P]. The verifier asserts that no frame below a
watermark holds a young reference; K8 can switch the watermark off, leaving minors correct and slower.

### 8.2 JIT frames, by tier: the tier contract

**In every tier, VM register-stack frames are the only home of Scheme values at every suspension point (every
non-`Leaf` call), and there they hold only tagged values.** No tier uses Cranelift user stack maps, a native frame
walker, an unwinder or `stack_switch`, and derived pointers never live across a suspension point. S1 (fragments with
`CallConv::Tail` and `return_call_indirect`) is the only baseline; native call/ret (S2) is outside this contract (K11).
Tier 1 caches VM registers in SSA between suspension points and stores dirty values before each; a later tier 2 may
unbox and derive freely within a block but publishes **tagged values only** (re-tagging or boxing doubles through a
`Leaf` allocation, shifting untagged integers, materializing inlined frames), so the same `(code, pc)` means the same
thing whichever tier wrote the frame, capture stays a `memcpy`, and invariant W holds for continuations.

**Helper classes**, from a machine-checked `#[helper(class = …, noalloc)]` attribute and a table the JIT reads:
- **`Leaf`** never collects, runs Scheme, transfers control, blocks or enters a safe region; values stay in SSA across
  the call; an error comes back as a status, after which the fragment takes a `Transfer` path to raise it (or, for
  `Oom`, to collect and retry the call, §5). `NoAlloc` marks those that do not touch the allocation buffer. Examples:
  the refill, bignum promotion, `eqv?`/`equal?`, flonum boxing, the user-sized constructors, non-blocking heap-only
  primitives.
- **`Transfer`** does anything else: the poll slow path, calls of closures from helpers, resumable and higher-order
  primitives, `apply`, the deoptimization call of a shadowed inline primitive, `call/cc` and capture, continuation
  invocation, raise, the wind operations, abort, `(gc)`, blocking I/O, tier-up and invalidation, `WATCHED` stores.
  Before the call the fragment publishes dirty values, `pc` and `ap`; the helper services pending events and answers
  `(Continue | Target(entry), value)`, and the fragment reloads and continues or tail-calls the target (the
  interpreter's helpers answer the next `(code, pc)`). So a thread switch, a signal, a debugger stop or a
  deoptimization resumes elsewhere without any native frame resuming stale state.

**Embedded addresses.** Machine code embeds an address as an immediate only if it is (1) in the immortal space (§4);
(2) a descriptor of the body's own unit, freed with the body; or (3) guarded by a `WATCHED` dependency that invalidates
the body before the referent can die. Anything else is one load from the descriptor's traced constants: descriptor-space
objects never move but die at majors, so a reused address could satisfy a stale type test. Inline-cache words are
traced. **The `Mutator` is never embedded** (it is always in `x21`/`r15`), and code is never patched at GC; tier-up
writes `desc.entry` (data).

**The entry trampoline.** With `enable_pinned_reg`, Cranelift never saves the pinned register, yet `x21`/`r15` are
callee-saved for the Rust caller (`aarch64/abi.rs:1411-1417`, `x64/abi.rs:1140-1143` [S]). Rust enters JIT code through
a generated trampoline in the platform convention that saves the old pinned value (`get_pinned_reg`), sets the `Mutator`
(`set_pinned_reg`), calls the `Tail` body and restores the old value, so nested drivers and many heaps per process never
corrupt a caller's `x21`.

**Debugging pins the interpreter tier** (`PRD/future/TREE_WALKER_HOOK_SYSTEM.md` §10.1: "hooks attached ⇒ don't tier
up"). Attaching a `StepTracer`, a breakpoint, a watchpoint or a `DebugHook` happens only at a top-level-form boundary
and sets a per-heap **tier-policy flag**. While it is set nothing tiers up and no JIT body is entered (`desc.entry` is
bypassed for the interpreter trampoline), and fragments already running leave their bodies through the `Transfer`
invalidation path (§10), as on a `WATCHED` store; detaching clears the flag at the next form boundary. A paused hook
that evaluates Scheme (`p <expr>`, conditional breakpoints) re-enters the machine beneath a `Cx`, so its evaluation runs
under `NoGcScope` and its allocation is counted by K16.

### 8.3 Rust code: the soundness boundary

**Capabilities.** The type system, not a runtime counter, decides who may collect.
- **`&mut Heap`** is the only capability that can collect: the VM's and tree-walker's outermost driver loops, the
  loader at points A and B (below), `eval_datum`, `run_forms`, and the embedding API's `eval_*` and `call` hold it.
- **`Cx<'gc>`** is the mutation and allocation context, created by `heap.mutate(|cx| …)` (HRTB-branded, gc-arena's
  shape) or by a driver for one primitive call. **No method on `Cx` can collect.** A nested driver entry reachable
  from a `Cx` (point C, the residual `apply_proc` fallbacks, a paused debugger's evaluation, `across_reentry` until
  stage 4e) runs its polls deferred: `NoGcScope` by type.
- So **a `Value<'gc>` cannot be alive while a collection runs**: collection needs `&mut Heap`, which the borrow checker
  refuses while any `Cx<'gc>` borrows the heap. Driver code alternates `mutate` windows and collection points, and
  roots carry values across.
- **The VM and tree-walker cores are a trusted island**: their loops hold `&mut Heap` and raw `Word`s in VM-managed
  memory, and rest on the root-provider contract, the verifier, the poison lanes and zeal. JIT entry passes the heap as
  a raw pointer derived from the driver's `&mut Heap`, reborrowed by each helper for its own duration (Stacked
  Borrows, checked by Miri on the Rust twins).
- Until stage 5e the capability is a `GcDriver` token over the `RefCell`-wrapped arena heap, borrowed per operation
  (S3).

```rust
pub type Prim = for<'gc> fn(&mut Cx<'gc>, &[Value<'gc>]) -> Result<Value<'gc>, EvalError>;
// Value<'gc> = #[repr(transparent)] word + PhantomData<fn(&'gc ()) -> &'gc ()>  (invariant brand)
// Resumable primitives return Step<'gc>: Done | Call | Eval | Collect(kind) | CollectAndRetry
```

The brand is a compile-time guarantee in primitives, frontend, macros, the embedding API and `patina-compat`, pinned by
trybuild compile-fail tests (a value used after a may-collect call, a slice held across `load_library`, a value in a
`static`, a value escaping `interp.with`). Values stored long-term outside the core crates (`CoreExpr`/`CpsExpr`
literals, macro literals, `Step` state) use an explicit `pub unsafe` raw-word API under a `NoGcScope` or a traced root,
or the **`CoreExpr` literal pool**, a traced table shared by both backends.

**The unsafe boundary.** `#![forbid(unsafe_code)]` in `patina-primitives`, `patina-frontend`, `patina-macros`,
`patina-runtime`, `patina-ir`, `patina-pipeline`, `patina-interpreter` and `patina-compat` (none uses `unsafe` for heap
access today [S]), and `deny` with an audited allow for `patina-repl`'s two `std::env::set_var` calls; unsafe heap
access is confined to `patina-core`, `patina-gc`, `patina-vm`, `patina-tree-walker` and
the JIT crate. `Word → Value` and raw slot access are `pub unsafe fn`. Release-mode reservation range checks guard the
trust boundaries, and `PENDING_ESCAPE` leaves `thread_local!`, so no provider can report a value to another heap.

**Borrowing.** Reads take `&Cx`; mutation and allocation take `&mut Cx` while the arenas can relocate, `&Cx` after
5e. Bulk reads return `&[HeapSlot<'gc>]` tied to a `&Cx` borrow, so a funnel store cannot coexist with them; no public
API returns `&BigInt`, `&mut Vec<char>` or `&mut [Word]` into the heap.

**Across a collection**, only these hold values: resumable-primitive state (in the VM `resume_stub` frame or the
tree-walker's `ResumePrimitive` continuation, both traced); a **`RootScope<'h>`**, which wraps the `&'h mut Heap`
capability so `mem::swap` cannot break LIFO, and whose `Rooted` values carry `{heap_id, generation}`, checked on
`get`; an **`Owned` handle** (`{heap_id, index, generation}` plus a `Weak<HandleTable>`, so a `Drop` after teardown is
a no-op); `RootToken` and `PinToken`, which hold a `Weak` to their table.

**Loading and nested loops.** A bare top-level `(import …)` is recognized **by the binding of `import`**, never its
spelling, and processed outside the desugarer, so library bodies collect between forms (points A and B) and inside
them (point D, in the outermost loop). Nested VM loops stay deferred until stage 4e deletes the weak continuation
tables, whose soundness rests on "nested loops defer" (`crates/patina-vm/src/runtime/vm_state/gc_roots.rs:21-28` [S]);
then loops the VM enters from its own driver level may collect, and loops beneath a `Cx` stay deferred for good. Point
C (an import met mid-form) and nested tree-walker trampolines keep `NoGcScope`. `%parameterize-swap!` stops calling
back at stage 4a (standard ports become parameter objects; other parameter-like procedures go through a Scheme loop
with a `guard`-based undo). S2 and S4e hold the detail.

### 8.4 The tree-walker: it stays correct and may lag

- Its heaps are `HeapPolicy { generational: false, evacuation: false }` **for good**: whole-heap and non-moving,
  sharing the object model, primitives, allocator and funnel. It collects only at its outermost trampoline safepoint;
  nested trampolines keep `NoGcScope`.
- **Roots.** Its `Rc<Environment>` frames, `ContValue` chains, `StepResult` fields, `CpsExpr` literals and the pending
  escape are reported by value through `SlotVisitor::pinned`, sound because nothing in its heaps moves.
- **Payloads.** `CpsLambda`/`CpsContinuation` are host-payload ids; globals go through cells. An `Environment` is
  charged as external bytes only while a registered host payload holds it, so `(fib 25)`'s 1.09 M environments [P]
  never drive a collection.
- **Threads.** A green thread is one suspended `StepResult` in the thread table (§12); the tree-walker is never one of N
  carriers.
- **Rule: a red tree-walker lane blocks the stage that turned it red.**

### 8.5 Embedding API

- `Interpreter::eval_*` returns `Owned`, which fixes the measured use-after-free of an `eval_str` result read after a
  later collecting `eval_program` [P].
- `interp.with(|cx: &mut Cx<'gc>| …)` is branded and takes `&mut self`, so nothing that may collect runs inside it.
- **`Interpreter::call(&mut self, f: &Owned, args: &[Owned]) -> Result<Owned, InterpreterError<_>>`** is a driver entry
  like `eval_*`: it may collect while the callee runs, with the arguments rooted by their handles, and a continuation
  captured inside it behaves as one captured inside `eval_*`. `Cx` has no `call`; a primitive needing a callback
  returns `Step::Call`.
- **Host primitives:** `register_primitive(lib, name, PrimSpec { f: Prim, arity, class })` and
  `register_resumable(lib, name, arity, f)`. `class` declares the helper class the JIT reads (`Leaf`, optionally
  `NoAlloc`, or `Transfer` for one that blocks; resumable means `Transfer`); a procedure argument is called only through
  `Step::Call`; registration exports the primitive from the library `lib`, so a program reaches it through `import`,
  which installs a binding, and fast paths key on that binding, never on the spelling.
- `Interpreter::interrupt_handle()` returns the `InterruptHandle` of §9. The public `Backend` trait migrates one step
  per stage under #601's deprecation convention, with `scripts/check_embedding_features.sh` green at each (S2, S3).
- Dropping the interpreter tears its heap down (finalizers run, the reservation is unmapped). `patina-compat`'s reader
  uses `Heap::new_standalone()`, with collection off and no driver. Plugins never see `HeapIndex` or the encoding.

### 8.6 Global bindings under variant R

Every off-heap holder of a value today has a stated fate (the inventory is on the S0 tracking issue); the global
environment's fate is a design. Variant R (stage 4b) keeps today's "follow the name" semantics (decision 2): code
compiled before a later `define` over an import, or before a later import, must see the new binding, so R needs an
indirection that variant C does not.
- **Binding records.** Every name in a namespace (the global environment, a library's environment, an environment
  specifier) maps to an immortal `BindingRecord { cell }`: own definitions, imports, aliases and placeholders alike.
  `Library.exports` maps names to cells, resolved one hop to the owning cell.
- **Link tables.** Each code unit keeps a table of record references indexed by its global operands, filled at load; a
  name with no binding yet gets a placeholder record whose cell holds `UNBOUND`. `LoadGlobal`, `StoreGlobal` and
  `Define` take a link index; a read is `link[k] → record → cell → value`.
- **Re-pointing.** A `define` over an import allocates a new cell and re-points `record.cell`; a `define` over the
  namespace's own binding stores into its cell (R7RS's "acts like `set!`"); an import re-points `record.cell` to the
  exporter's cell. Earlier code holds the record, so it sees the change, when the `define` runs.
- **What goes:** `GlobalCacheEntry`, the `env_id` cache, `frame_globals`, `FORWARDED`, `Owner` links and
  `VmClosure.globals` (one link of today's teardown cycle). **What stays:** the shadow bitsets and `mark_if_*`, which
  latch when a record is re-pointed or a cell store replaces a primitive, and the per-site deoptimization they drive
  (#442). Stage 4b measures the path against `frame_globals` rather than assuming a gain.
- **The JIT under R** emits the two dependent loads, or embeds the cell with `WATCHED` on both record and cell. Under C
  the records disappear and a JIT global is one load. The tree-walker reads root bindings through the same records.

---

## 9. Safepoints and polling

**Mutator words** (offsets in §11): `reg_limit` (`AtomicUsize`; non-zero values written only by the owner through
`set_limit`, zero by any poster), `event` (`AtomicU32`; remote posters `fetch_or`, the owner swaps), `pending`
(`Cell<u32>`, owner posts only) and `ticks` (`Cell<i32>`, owner only, Tick mode only). Relaxed atomic loads and stores
are plain `ldr`/`str`, so the atomics cost nothing and a signal handler writing them is sound.

**Event bits:** `GC_MINOR`, `GC_MAJOR`, `PREEMPT`, `SIGNAL`, `DEBUGGER`, `TERMINATE`, `HANDSHAKE` (reserved),
`FINALIZERS_PENDING`, `DECOMMIT_PENDING`, `HEAP_EXHAUSTED`, `STACK_GROW`.

**Protocol.** Owner posts (allocation slow path, barrier soft limit, finalizers, decommit, stack growth):
`pending |= bit; reg_limit.store(0, Relaxed)`. Remote posts (signal handlers, timers, profilers; later, other
carriers): `event.fetch_or(bit, SeqCst); reg_limit.store(0, SeqCst)`. `set_limit(new)`, used by the poll slow path and
by a green-thread switch: store `new`, `fence(SeqCst)`, and store 0 again if `event` or `pending` is non-zero. The poll
slow path takes `pending.take() | event.swap(0, SeqCst)`, services it, then calls `set_limit(real_limit)`. This is
OCaml's protocol (`runtime/domain.c:387-396,2022-2058`): the fence makes a Dekker pair, so **no request is ever
lost**, and latency is bounded by the next frame entry, back-edge or `Transfer` return.

**`InterruptHandle`.** A signal handler, timer thread or profiler cannot borrow an `Interpreter`, there is no
process-wide GC singleton, and the `Mutator` is freed at teardown, so each heap hands out handles:

```rust
#[derive(Clone)]
pub struct InterruptHandle(Arc<InterruptCell>);            // Send + Sync + Clone
struct InterruptCell { mutator: AtomicPtr<Mutator>, posting: AtomicUsize }

impl InterruptHandle {
    /// Async-signal-safe: lock-free atomics only; no lock, no allocation, no panic path.
    pub fn post(&self, ev: u32) -> bool {
        let c = &*self.0;
        c.posting.fetch_add(1, SeqCst);
        let m = c.mutator.load(SeqCst);
        if !m.is_null() {
            unsafe { (*m).event.fetch_or(ev, SeqCst); (*m).reg_limit.store(0, SeqCst); }
        }
        c.posting.fetch_sub(1, SeqCst);
        !m.is_null()
    }
}
```

Teardown stores null into `mutator`, waits until `posting` reads 0, then frees the `Mutator`; with every operation
`SeqCst`, a poster either raised `posting` before the null store (and is waited for) or loads null and does nothing.
The REPL's SIGINT handler uses its interpreter's handle through a `sigaction` wrapper in `patina-core`. Servicing
`SIGNAL` raises a non-continuable `&interrupt` at the poll, also under `NoGcScope`, where GC events stay deferred.

**Where polls are.** Codegen emits forward jumps only, so every loop passes through a closure call or tail call (which
enters its callee through the check) or a `Transfer` operation (which polls on return). `Leaf` primitive sites cannot
loop and need no poll.

| Site | Interpreter | JIT | Extra cost |
|---|---|---|---|
| **frame entry**: every non-tail call, every tail call (self or not, whatever the window), `Apply`, `TailApply` | after the header and arguments are stored and the window initialized: `top > reg_limit`; an event is serviced at the callee's pc 0, whose map lists the parameters, `code` and `closure` | callee prologue: `ldr x9,[x21,#0x30]; add x10,fp,#FRAME; cmp x10,x9; b.hi cold` | **0**: the stack-overflow check is needed anyway (V8's `StackGuard` fold) |
| self tail call (every loop) | the frame is re-entered at pc 0 through the same check | `ldr x9,[x21,#0x30]; cbz x9, cold` at the back-edge | 2 instructions |
| **every `Transfer` return**: generic calls of non-closures, continuation reinstatement, abort landing, raise-stub push, `Step` results, `(gc)` | the helper services pending events before answering the next `(code, pc)` | the same, inside the helper | — |
| return to the driver | services pending work | the same | — |

**No collection sees a half-built frame**: the frame-entry check may detect an event early, but servicing waits until
the frame is complete (today `TailCall` stages arguments in a Rust buffer before storing them, `vm_state.rs:1601-1630`
[S]). The cold path tells a genuine stack limit from a posted event (`reg_limit == 0`). The interpreter's
per-instruction `gc_pending` load (+1.1–1.4%, as today's `docs/GC_DESIGN.md` §6.1 reports) goes in stage 3. Allocation
and barrier slow paths only post. Zeal `entry` services an event at every poll site.

**Deterministic mode.** GC requests come only from byte thresholds and counts (§13), so the differential lanes run the
poll code production ships. `PollKind::Tick` exists only for SRFI 18's deterministic scheduler: poll sites also
decrement `ticks` (Chez's `%trap`) and expiry posts `PREEMPT`. No timer runs under `PATINA_DETERMINISTIC=1`, the default
in test lanes (K7 measures Tick's cost).

**Nested Rust loops** (what replaces today's `GcDeferGuard` sites). The poll slow path collects only with the
`&mut Heap` capability and `no_gc_depth == 0`; entries beneath a `Cx` run deferred. A deferred event stays posted while
allocation overdrafts, a store buffer past its soft limit is discarded with the next collection forced major (§7), and
`GcStats` keeps the high-water bytes allocated since the last serviced poll and under one `NoGcScope`, with the opening
site, which K16 bounds; so the GCLocker failure mode (JEP 423) stays visible, confined and measured. Green-thread
preemption is deferred while `reentry_depth > 0`.

**Safe regions.** `enter_safe_region`/`leave_safe_region` wrap blocking I/O and the future FFI; with one mutator they
are no-ops (Chez `Sdeactivate_thread`). One may be entered only when no unrooted heap value is on the Rust stack or in
JIT SSA: a `Leaf` helper never enters one, blocking primitives are `Transfer`, and the blocking part works on Rust
buffers (output formatted first; input read before anything is allocated, unlike today's `read`, which lexes while the
parser holds a half-built datum). The restructuring is required only before N > 1.

---

## 10. Continuations and stacks

**Representation: design A.** A continuation is **one immutable, variable-size heap object** (`T_CONT`): meta words as
fixnums (kind, `deliver_reg`, the depth-at-capture byte offsets, `exit_status`, re-entry boundary ids), references to
the captured winds, handlers, prompts and parameterization, and the captured frame words. The continuation procedure
is a `0101` object whose descriptor is `CONT_INVOKE`, so invoking one is an ordinary call (a `Transfer`).

**Capture** is a `memcpy` of `[base, top]` into an object allocated through `try_alloc` (collect-and-retry on `Oom`,
§5), then a pass over the copy that clears dead slots through the maps plus the `call/cc` `dst` hole (clearing that hole
fixed a 296 MB leak, `crates/patina-vm/src/runtime/control.rs:641-654` [S]), zeroes every `ret` and clears `WM` flags.
**The copy is value-only**, so invariant W holds, frames need no fix-ups, and **the core traces a `T_CONT` word by word,
like a vector**, without parsing frames; the frame and map formats stay private to the VM, and the code references in
captured frames keep their units alive by ordinary marking. Captures over 8 KiB go to the LOS, where minors release the
dead ones (§6.2).

**Cost.** Today a capture at depth 1000 costs **24 µs and about 171 KB** [P, `samedepth1000`]. Design A's estimate comes
from a continuation toy whose model of today costs 5.0 µs and retains 64–80 KB per capture at that depth; the toy put
design A about 5.8× below its model, but with a bare `memcpy` and 24 B frames. Design A keeps the dead-slot pass (O(1)
per frame) and uses 40 B headers, so about 88 KB per capture at depth 1000 [I]; stage 4e re-runs the toy with the real
pass, headers and lookup before fixing per-probe targets.

**What this deletes:** `VmContinuationRef`, `VmDelimitedContinuationRef`, both weak side tables, the VM's
`trace_weak_ids`/`sweep_weak`, and the soundness rule "store touched within one dispatch", with it the reason nested VM
loops must defer. **What it fixes:** captures become byte-accounted (today 80 K captures at depth 1000 reach 5.7 GB
unseen by the trigger [P]).

**GC interaction.** Continuations are born young and written only by initializing stores, so they need no barrier;
reinstatement copies frames out and resets the watermark to the base; a future incremental mode must darken a
continuation on reinstatement (OCaml's fibers).

**JIT interaction.** **`(code, pc)` is the authoritative resume point**; `ret` is a cache, re-derived on reinstatement
from `desc.resume[pc]` (or the interpreter trampoline). Tier-up writes `desc.entry` and answers the new entry.
Invalidation (a `WATCHED` store, a tier-down, an attached debugger hook) marks dependent bodies invalid, deoptimizes the
executing fragment and re-derives the `ret` of every frame of the affected descriptors in every green thread's stack;
saved watermark targets are already `(code, pc)`. Replaced bodies are freed by epoch (§6.8). Captured copies hold no
`ret`, so a multi-shot continuation never jumps into discarded code. Capture is the same `memcpy` for interpreted and
compiled frames, and escapes are "the helper answers a new target": nothing unwinds native frames, which holds because
S1 is the only baseline.

**Control semantics are unchanged:** multi-shot `call/cc` copies out on every invoke; `dynamic-wind` travels one thunk
per step through stub frames; delimited capture covers `[prompt_offset .. top]` and relocates by byte-offset arithmetic.
The matrix and `escape_from_primitive.rs` gate stage 4e on both backends.

**Later and optional: C′** freezes frames above a frozen watermark into immutable chunks and thaws them lazily through
an underflow stub frame (toy: 57 ns per capture at depth 1000, against the toy's 5.0 µs model of today and the measured
24 µs [P]). It reuses this frame format and the watermark machinery and would give green threads segmented stacks; it
comes only after the baseline JIT and only if the capture-at-depth probes show a need (K15).

---

## 11. Pluggability contract

**Principle.** The contract is pluggable; the collectors are not a catalogue. Fast paths are data that the JIT inlines
and that `#[inline(always)]` Rust twins share (Whippet's `gc-attrs.h`; JEP 475's one definition, expanded late). The
collector is selected statically, per-heap policy is a runtime field read only by slow paths, and nothing on a fast
path is `dyn`.

**Against HotSpot's GC interface.** JEP 304 made HotSpot's collectors pluggable behind `CollectedHeap` and a
`BarrierSet` realized per execution tier (`BarrierSetAssembler` for the interpreter and stubs, `BarrierSetC1`,
`BarrierSetC2`), with the collector chosen at startup (`-XX:+UseSerialGC`, `-XX:+UseG1GC`, `-XX:+UseZGC`, …). Here
`Collector<M>` plays `CollectedHeap`, `GcAttrs.barrier` plays the `BarrierSet` (consumed by the interpreter's funnel and
the JIT emitter alike), and `emit_alloc`, `emit_store` and `emit_poll` are the per-tier halves. Patina **adopts** a
per-collector barrier contract consumed by every execution tier, and late barrier expansion from one definition (JEP
475). It **rejects** several production collectors chosen at startup behind virtual calls (HotSpot itself retires
collector modes for their maintenance cost, JEPs 474 and 490), and per-collector code-generation hooks inside each
compiler: HotSpot needs those for two JIT compilers and several collectors, while Patina has one JIT and one production
collector, so a data description the emitter switches on is enough.

```rust
// crate patina-gc: depends on libc only; no VM or core dependency; Miri-testable.

/// Implemented by `declare_layouts!` in patina-core (`CoreModel`) and by a test model in the conformance suite.
/// Every method requires `obj` to be the start of a live object of this heap, which is why each is `unsafe`.
pub unsafe trait ObjectModel: 'static {
    const LAYOUT: &'static LayoutTable;                                    // tags, header types, JIT offsets
    unsafe fn size(obj: Address, tag: Tag, meta: MetaByte) -> usize;       // + 16 if HASH_MOVED
    unsafe fn trace<V: SlotVisitor>(obj: Address, tag: Tag, v: &mut V);   // writable slots; T_CONT word by word;
                                                                           // T_EPHEMERON only through `ephemeron`
    unsafe fn copy_to(obj: Address, tag: Tag, to: Address, extra: usize);
    unsafe fn verify(obj: Address, tag: Tag) -> Result<(), LayoutError>;
}

/// Slot-only visitor: frames are walked by the VM's RootProvider, not here. Slots are interior-mutable, so
/// providers keep `&self`.
pub trait SlotVisitor {
    fn slot(&mut self, s: &HeapSlot);               // precise and updatable
    fn slots(&mut self, s: &[HeapSlot]);
    fn pinned(&mut self, v: Word);                  // precise, never updated; pins the block for this cycle
    fn host(&mut self, id: HostId);                 // edge to a host payload, traced in the fixpoint
    /// Emitted for T_EPHEMERON in place of two strong slots. A collector traces `value` only once `key` is live and
    /// may break both (§6.6); a visitor that is not a collector may visit both.
    fn ephemeron(&mut self, e: Address, key: &HeapSlot, value: &HeapSlot);
}
pub enum RootPass { Minor, Major }
pub trait RootProvider { fn trace(&self, pass: RootPass, v: &mut dyn SlotVisitor); }
// Registration is open: RootSet::register(Box<dyn RootProvider>) → RootToken, which holds a Weak to the set.

/// The heap's carriers and green threads, owned by the heap and lent to `collect`: one `Mutator` under M:1, two in
/// the two-mutator lane, N later. The VM's root provider walks frames, reading each thread's watermark here and
/// re-installing it as it finishes that thread.
pub struct MutatorSet { /* mutators: Vec<Box<Mutator>>, threads: ThreadTable */ }
impl MutatorSet {
    pub fn mutators(&mut self) -> impl Iterator<Item = &mut Mutator>;  // retire ap/limit; drain (minor) or drop
                                                                       // (major) the store buffer, range entries and
                                                                       // remember-whole list; K16 counters
    pub fn threads(&mut self) -> impl Iterator<Item = &mut ThreadGcState>;      // every rooted green thread
}
pub struct ThreadGcState { pub ran_since_gc: bool, pub watermark: usize /* byte offset */, pub stack: Range<Address> }

pub unsafe trait Collector<M: ObjectModel>: Sized + 'static {
    fn new(cfg: &HeapConfig, vm: Box<dyn VirtualMemory>) -> Result<Self, HeapError>;
    fn attrs(&self) -> GcAttrs;                                             // per heap; the JIT reads it
    fn bind_mutator(&self, m: &mut Mutator);
    fn alloc_slow(&self, m: &Mutator, bytes: usize, k: AllocKind) -> NonNull<u8>;            // never collects
    fn try_alloc(&self, m: &Mutator, bytes: usize, k: AllocKind) -> Result<NonNull<u8>, Oom>; // user-sized; §5
    fn alloc_old(&self, m: &Mutator, bytes: usize, k: AllocKind) -> NonNull<u8>;             // remember-whole
    unsafe fn log_slow(&self, m: &Mutator, granule: Address);            // Rust twin of the inline slow path
    unsafe fn log_range(&self, m: &Mutator, obj: Address, start: usize, len: usize);  // bulk stores
    unsafe fn identity_hash(&self, m: &Mutator, obj: Address) -> u64;    // core's safe wrapper takes Value<'gc>
    unsafe fn pin(&self, obj: Address) -> PinToken;
    unsafe fn is_live(&self, obj: Address) -> bool;   // the immortal space (cells, binding records, symbols,
                                                      // canonical boxes, primitives and their descriptors): true
    unsafe fn forwarded(&self, obj: Address) -> Option<Address>;
    fn requested(&self, m: &Mutator) -> Option<CollectionKind>;
    fn collect(&mut self, kind: CollectionKind, mutators: &mut MutatorSet, roots: &RootSet,
               weak: &mut WeakRegistry) -> GcStats;
}
```

**Linkage.** `patina-gc` defines the traits, `MarkRegion<M: ObjectModel>` and, inside its conformance suite only,
`NullGc<M>`. `patina-core` generates `CoreModel` with `declare_layouts!` and defines
`pub type ActiveGc = MarkRegion<CoreModel>;`, so core depends on gc and the collector reaches the model through its type
parameter; downstream crates name `Heap`, never `Heap<C>`. `collect` takes `&mut self`, which only the `&mut Heap`
capability reaches (§8.3), and is lent everything per carrier and per thread that it must retire, drain or reset, so
the protocol is written for N mutators. `VirtualMemory` is a trait so that Miri can run on a `Vec`.

**Weak processing and finalization are part of the contract.** A second collector (`NullGc`, a future
`NurserySpace`, an MMTk adapter) implements §6.6–§6.10 from these types and obligations alone:

```rust
pub struct WeakRegistry {           // per heap; lent to `collect`; no collector keeps a copy
    pub ephemerons: EphemeronTable, // per collection: pending[K] chains, KEYHINT bookkeeping, the to-break list
    pub host: HostPayloadTable,
    pub finals: FinalRegistry,
    // stage 7 or later (decision 11): pub guardians: GuardianTable
}
pub trait HostPayload {                        // tree-walker lambdas and continuations, macro bodies, libraries, FFI
    fn trace(&self, v: &mut dyn SlotVisitor);  // `slot`s, or `pinned` in tree-walker heaps; collecting thread only
    fn external_bytes(&self) -> usize;         // charged at registration, credited at finalization (§13)
}
impl HostPayloadTable {
    pub fn register(&mut self, p: Box<dyn HostPayload>) -> HostId; // joins `young`
    pub fn dirty(&mut self, id: HostId);       // a mutated old payload rejoins `young` for the next minor
    pub fn trace_payload(&self, id: HostId, v: &mut dyn SlotVisitor);
    pub fn take(&mut self, id: HostId) -> Box<dyn HostPayload>;     // by its finalizer only
}
pub enum FinalKind { Port, CodeUnit, HostPayload, Thread, Foreign }
impl FinalRegistry {
    pub fn register(&mut self, obj: Address, kind: FinalKind, id: u32);   // slow-path constructors only; `young`
    pub fn queue(&mut self, id: u32);          // the runtime knows the object is finished (a terminated thread)
    pub fn drain(&mut self) -> impl Iterator<Item = (FinalKind, u32)> + '_;     // queued entries, in id order
    pub fn drain_all(&mut self) -> impl Iterator<Item = (FinalKind, u32)> + '_; // teardown (R5)
}
```

The obligations of every `Collector`:
- **Order.** §6.10's eleven steps, in that order, in every collection; a step may be empty (`NullGc` leaves all but
  statistics and release empty), never reordered.
- **Tracing.** Host edges reported through `host(id)` are traced through `trace_payload`, on the collecting thread,
  inside the one fixpoint of step 2, with ephemerons resolved by key and, later, guardians resurrecting before breaking.
- **Safety, always:** never break an ephemeron whose key is live; never queue a reachable registered object or payload;
  never finalize or decommit inside `collect`; never call a `HostPayload` off the collecting thread; leave every
  ephemeron link word at fixnum 0; retire every mutator's buffer and drain or drop every mutator's logs.
- **Completeness, in collections reported `complete`** (every `MarkRegion` major, `(gc)` included; never `NullGc`):
  every ephemeron with an unreachable key is broken, and every unreachable registered object and payload is queued.
  Minors restrict steps 2–7 to young entries and treat old, immortal and immediate keys as live.

`patina-gc`'s conformance suite runs on `MarkRegion<TestModel>` and `NullGc<TestModel>`, and `patina-core` runs it on
`ActiveGc`: allocation and tracing; a live-key ephemeron never broken; after a `complete` collection a dead-key one
broken; a 16 K ephemeron chain resolved with linear work (counted, not timed); host-payload edges traced and an
unreached payload queued; `drain` in id order, `drain_all` at teardown, nothing finalized inside `collect`; the
epilogue order, checked with a recording payload and finalizer; and, with two `Mutator`s, both buffers retired and both
logs drained.

**The JIT ABI: `Mutator`, `#[repr(C)]`, in `x21` (`r15` on x64)**, offsets asserted in CI and frozen after the spike.

| Offset | Field | Type | Read by |
|---|---|---|---|
| 0x00 | `ap` | `Cell<usize>` | inline allocation (MMTk `BumpPointer` offset) |
| 0x08 | `alloc_limit` | `Cell<usize>` | inline allocation |
| 0x10 | `meta_bias` | `usize` (per-heap constant) | barrier fast path (hoisted) |
| 0x18 | `remset_cur` | `Cell<*mut usize>` | barrier slow path |
| 0x20 | `remset_soft` | `Cell<*mut usize>` | barrier slow path |
| 0x28 | `reg_top` | `Cell<*mut Word>` | calls (current green thread) |
| 0x30 | `reg_limit` | `AtomicUsize` | call check, back-edge poll; non-zero values written only through `set_limit` |
| 0x38 | `event` | `AtomicU32` | poll slow path |
| 0x3C | `pending` | `Cell<u32>` | barrier slow path (owner posts) |
| 0x40 | `ticks` | `Cell<i32>` | Tick mode only |
| 0x44 | `barrier_mode` | `u8` | slow paths only (0 generational; reserved: incremental update, SATB) |
| 0x45 | `safepoint_state` | `AtomicU8` | protocol (`Running`/`AtSafepoint`/`InSafeRegion`) |
| 0x48 | `thread` | `Cell<*mut GreenThread>` | helpers |
| 0x50 | `heap` | `*const HeapShared` | helpers |
| 0x58 | `status` | `Cell<u32>` | `Leaf` helper error and `Oom` status |

```rust
#[repr(C)] #[derive(Clone, Copy)]
pub struct GcAttrs {
    pub alloc: AllocAttrs,          // BumpPointer { ap_off: 0x00, limit_off: 0x08, granule: 16, max_inline: 256 }
    pub barrier: BarrierKind,       // None | GranuleLog { bias_off: 0x10, shift: 4, log_bit: 6,
                                    //   cur_off: 0x18, soft_off: 0x20, pending_off: 0x3C, limit_off: 0x30 }
                                    // | Card { .. } (runner-up, K2) | Call(extern "C" fn) (fallback; Leaf)
    pub value_filter_bit: Option<u8>,           // Some(2); None only if an SATB mode is ever configured
    pub poll: PollKind,             // LimitFold { limit_off: 0x30 } | Tick { ticks_off: 0x40, limit_off: 0x30 }
    pub watermark: WatermarkKind,   // None | ReturnBarrier { trampoline: usize, link_flag_bit: u8, stride: u16 }
    pub can_move: bool, pub can_pin: bool,
    pub initializing_stores_need_barrier: bool, // false for every shipped configuration
    pub layout: &'static LayoutTable,
    pub helpers: &'static HelperTable,          // class (Leaf | Transfer) and NoAlloc per runtime helper
}
```

The JIT's emitters switch on these values at compile time; the interpreter's `#[inline(always)]` Rust versions are
generated from the same table, and the conformance tests compare the two.

**One collector type, runtime modes.** No mutually exclusive cargo features exist, so CI's
`cargo clippy --all-targets --all-features` stays green. Every mode is a runtime knob of the one
`MarkRegion<CoreModel>` binary, as `PATINA_GC` is today; `NullGc` is never linked into it.

| Mode | Selected by | Purpose |
|---|---|---|
| whole-heap / sticky generational / with evacuation | per-heap policy (generational once M5 says so, K1; evacuation from stage 8) | production |
| off | `PATINA_GC=0`: no automatic trigger, but `(gc)` still runs a full major, as `GcMode::Off` does today | the differential reference run |
| null | `PATINA_GC=null`: no automatic trigger, `(gc)` a no-op, `BarrierKind::None`; a run that reaches `max_heap` fails | measurement only: the lower bound (JEP 318) |
| stress | `PATINA_GC_STRESS=n`: a collection every n allocations until 5e, every n polls after | differential lane |
| zeal | `PATINA_GC_ZEAL=major\|minor\|alternate\|move-all\|entry` (`jit-invalidate` once a JIT exists), `PATINA_GC_VERIFY=1`, `PATINA_GC_VERIFY_ROOTS=1` | torture lanes |
| debug-poison | debug builds: eager poisoning sweep, hole quarantine, `DEAD_SLOT` fill | use-after-free detection |
| two-mutator | `PATINA_GC_MUTATORS=2`: two `Mutator`s alternate deterministically on one OS thread | N-readiness lane |
| GC workers | `PATINA_GC_WORKERS=n` (stage P); 1 in the deterministic lanes | large live heaps |
| standalone | `Heap::new_standalone()` | `patina-compat`'s reader |

Today's mark-sweep is not ported; its oracle role passes to the off mode, the verifier and, during stage 5, the arena
build itself (lanes byte-identical before and after each kind moves).

**How a future collector plugs in.** A **copying nursery** (K4) is a `NurserySpace` composed into `MarkRegion`: its
memory is never armed, so the barrier is unchanged, and it promotes in place when survival is high or to-space is short.
**Incremental marking** (only on a latency goal) uses `barrier_mode` in slow paths; SATB would need
`value_filter_bit = None` and a JIT recompile. **Parallel stop-the-world marking** (stage P, independent of decision 7)
uses per-heap GC workers spawned lazily above a live-size threshold, CAS marking on the metadata byte during GC only,
work stealing, and a sharded ephemeron fixpoint; roots stay on the collecting thread, evacuation destinations stay
sequential, and a `workers=4` lane must match the single-threaded lanes byte for byte (SP holds the design). An
**MMTk-backed** `Collector<CoreModel>` would live outside the default workspace; pairs would need headers or side kind
bits, and its single instance per process conflicts with many heaps per process.

**Explicitly excluded:** load and read barriers; conservative scanning of VM, JIT or Rust stacks; collection inside
allocation or barrier slow paths, or reachable from a `Cx`; `dyn` on fast paths; patching machine code at GC, or
anywhere outside `install_code`; process-wide GC singletons; Java-style finalizers; Cranelift user stack maps, frame
walkers and `stack_switch`; raw slots in published frames, and native frames holding Scheme values across a suspension
point (S2 needs its own design, K11); embedding the `Mutator`, or a non-immortal address outside the body's unit
without a `WATCHED` dependency; racing parallel evacuation; more than one production collector.

---

## 12. Threading readiness

SRFI 18 ships as **M:1 green threads, VM first**; the GC's interfaces are written for **N mutators over one shared
heap**; isolates stay optional; no OS threads are built now. SRFI 18 needs a shared heap (threads share mutable state
and may invoke each other's continuations) but not parallelism, and Gambit, its reference implementation, meets it
with green threads. The carrier/thread split follows systems that run many threads over few carriers: Go gives each P
its own allocation cache (`runtime/mcache.go`), Loom mounts virtual threads on carriers (JEP 444), OCaml 5 gives each
domain its own minor heap (Sivaramakrishnan et al., ICFP 2020), and Chez gives each thread a context that it
deactivates around blocking calls (`c/thread.c`). ST, the threading-model issue filed at stage 0, records the
comparison and the cost estimates (decision 7). N-readiness that is not free is listed as obligations of the
`threaded` build, not claimed as done.

**`Mutator` (carrier) and `GreenThread` are separate.** The `Mutator` owns the ABI block of §11, the allocation buffer,
the store buffer and remember-whole list, the root-scope stack and `no_gc_depth`, the handle table, safepoint state and
`current_thread`; under M:1 there is exactly one. A `GreenThread` owns its register stack and `ThreadGcState`
(watermark, `ran_since_gc`), its frames and re-entry fields, its dynamic environment (parameterization, current ports,
handlers, winds, prompts, as heap data) and scheduler links. A switch saves `reg_top` and installs the incoming limit
through `set_limit` (§9). `allocs_since_gc`, `gc_threshold`, `gc_pending` and `gc_defer_depth` leave `Heap`, and no
runtime state stays in `thread_local!`.

**SRFI 18 objects** (layouts in §3).
- **Thread** (`T_THREAD`): name, specific, thunk or result, end exception and joiners are heap data; its id names a
  `GreenThread` in the per-heap thread table (−1 before `thread-start!` and after termination). `thread-start!` creates
  the `GreenThread` and reserves its stack, so a thread made and never started holds no stack.
- **Mutex** (`T_MUTEX`: owner or state, waiters) and **condition variable** (`T_CONDVAR`: waiters): waiter queues are
  heap lists of thread objects, written through the funnel. **Time** objects are pointer-free boxes of seconds.
- **Root rule.** The scheduler is a root provider reporting every started, non-terminated thread: the running thread,
  the run queue, the timer queue (sleeps and timed waits) and the blocked set; their frames and dynamic-state slots
  are roots in every collection (a minor scans only frames above the watermark of threads that ran). **A thread
  blocked for ever on objects nothing else reaches stays rooted until it terminates** (decision 23), as in Gambit,
  which links every non-terminated thread into its thread group (`lib/_thread#.scm:1346`, unlinked at termination,
  `lib/_thread.scm:1649`), and chibi, which keeps blocked threads on a global list (`lib/srfi/18/threads.c:165-206`).
- **Lifetime.** Every `GreenThread` is registered with `FinalKind::Thread`. Termination queues the entry at once
  (`FinalRegistry::queue`), so a terminated thread, joined or not, gives its register stack back at the next poll
  without a collection; its result stays in the heap object. A thread object that becomes unreachable before it
  terminates (possible only under decision 23's alternative) is queued by the collection that finds it, and teardown
  drains every remaining entry (R5). The finalizer unmaps the stack and frees the table entry, after the pause.
- **Tree-walker:** a green thread is one suspended `StepResult` in the thread table, reported through `pinned`; it
  switches only at the outermost trampoline safepoint; a blocking operation inside a nested trampoline raises, as one
  inside a VM re-entry does; its finalizer drops the `StepResult`.
- Under M:1, blocking inside a Rust re-entry raises an error, and blocking I/O stalls every green thread until a helper
  pool or descriptor polling exists. Stage 9 lands all of this on both backends, with the two-mutator lane.

**N-ready now, at zero single-thread cost:** per-mutator allocation buffers, store buffers and remember-whole lists,
never shared; a block pool behind an uncontended mutex; the mutators and threads lent to `collect` through `MutatorSet`;
every mutator read-modify-write of a metadata byte through `MetaByte` (plain under M:1, `fetch_and`/`fetch_or` under
`threaded`); heap words through `HeapSlot` (plain now, relaxed `AtomicU64` under `threaded`), with the funnel as the one
place a threaded build adds its publication fence besides the JIT's allocation groups; a safepoint protocol written as
request, acknowledge (at a poll or in a safe region), collect, release; posters that write only atomics; one interning
function; traced one-word inline caches; code installed through one function; the `Mutator` never embedded in machine
code; continuations that refer to no carrier state; the two-mutator lane (stage 9).

**Obligations of the `threaded` build** (before N > 1): atomic `u32`/`u8` sub-word stores for `string-set!` and
`bytevector-u8-set!`; acquire loads for heap references, or a documented dependency-ordering exception for JIT code,
beside the funnel's release fence; safe regions entered only with no unrooted value on the Rust stack or in JIT SSA,
with `read` lexing into Rust-owned tokens first; and, because Linux `mprotect` is process-wide, a dual-mapped code
reservation (a `memfd` with RW and RX views) before OS-thread carriers (macOS `MAP_JIT` toggling is per thread [P,
probe `mapjit.c`]). Marking needs nothing new: collection stays stop-the-world, and stage P's workers already mark by
CAS. The `threaded` cargo feature exists from stage 3 with the accessor bodies only, so CI's `--all-features` clippy
lints it; no lane runs it until stage 9, and enabling it needs the owner's shared-memory decision.

**Deferred until shared-memory parallelism is approved** (decision 7): OS-thread carriers, a `threaded` test lane,
handshakes, a `Send`/`Sync` heap, TSan, loom and Miri-concurrency lanes, freeze-and-slide stacks (C′). Parallel
stop-the-world marking needs GC worker threads, not mutator threads, so it is not on this list.

**Isolates** (decision 7's cheaper alternative): one interpreter per OS thread, sharing nothing. The design already
enables them (no runtime state in `thread_local!`, no process-wide singleton, teardown fixed, one reservation per heap);
they need `Interpreter: Send` for VM heaps after stage 5e plus an audit of the remaining `Rc`s, defined semantics for
stdin lookahead, the exit status and `exit`, and copied messages. With one mutator per heap they avoid every
`threaded` obligation, but they parallelize only share-nothing work; SRFI 18 threads stay green threads inside each.

---

## 13. Heap sizing, pacing and observability

**Triggers are in bytes**, counted at refill and at LOS allocation, plus **external bytes** (Chez's phantom bytes,
OCaml's `caml_alloc_custom_mem`): 8 KiB per open, unclosed file port; the Rust size of code bodies, macro bodies and JIT
code; tree-walker environments held by registered host payloads; string-port buffers. Continuations are heap objects
from stage 4e. Inputs are bytes and counts, never time, so pacing is deterministic. L is the live bytes after the last
major, plus LOS and external bytes.
- **Whole-heap heaps** (stages 5–6, tree-walker heaps for good, generational heaps in bypass): the next major after
  **`max(8 MiB, 2·L)`** bytes of allocation, which keeps the peak near 3× live. Racket CS's `L + 8192·√L` rule sits on
  an 8 MB-per-place nursery (`racket/src/cs/rumble/memory.ss:35-39` [S]); without one it would mean 1.15–2× more
  full-heap marking than 2·L on the large-live GBS workloads, and its 32 MiB first major would lift small programs
  from 15–23 MB peaks to about 45 MB [P].
- **Generational heaps** (stage 7 on): majors at `L + max(8 MiB, min(8192·√L, 2·L))`, only once an interleaved A/B shows
  it beats 2·L (about 1.8× live at peak at 100 MB, 1.26× at 1 GB); a fixed 4 MiB nursery budget (about 150 K objects
  [P]), adapting only through the deterministic bypass (§6.2).
- **Near the ceiling** the target never exceeds `max_heap` minus the emergency reserve; collections become more
  frequent instead. Backstops and descriptor pressure: §6.2 and §6.7.
- **Adaptive pacing is opt-in** (`PATINA_GC_PACING=adaptive`, decision 15): a MemBalancer major target
  `L + clamp(c·√(L·g/s), 0.25·L, 3·L)` (g the smoothed promotion rate, s the marking speed; EWMA with 10% hysteresis,
  because resizing at every GC is "giving control of your stereo's volume knob to a hyperactive squirrel",
  https://wingolog.org/archives/2024/09/18/whippet-progress-update-feature-complete) and a nursery sized toward the
  minor budget; never in test lanes, and the default only if it wins an A/B on RSS at constant GC time.
- **Free reserve:** after a major at least `max(8 blocks, 5%)` empty blocks, or the heap grows; two consecutive majors
  that each free under 1% raise the target ×1.5 (Wingo's livelock fix). Fragmentation (§6.5) is reported every major.

**Observability (stage 0 on).** Per collection, `GcStats` records kind, reason and `complete`; phase times; bytes
allocated, promoted, marked and freed; block counts, fragmentation and occupied against marked bytes; store-buffer
entries, range entries and logged granules; pinned blocks, mark-stack peak, frames scanned, deferred polls and buffer
discards; and **K16's high-water marks** (bytes allocated between a posted event and its servicing poll, and inside
one `NoGcScope`, each with its site). Outside the pause it logs finalizers, catch-up sweep, block classification, LOS
release, decommit (heap, stacks, store buffers), sampled refill time, watermark cold returns and time in bypass.
Cumulatively: the GC-time fraction, per-workload max pause for minors and majors, and an **MMU** from every
non-mutator interval at 1–100 ms windows (Larceny `gc_mmu_log.c`); comparisons use per-workload max pause and
MMU(10 ms), never pooled percentiles. Outputs: `PATINA_GC_LOG` (CSV), `PATINA_GC_TRACE` (JSON phase events for the
debugger hook system), and representation-independent `(gc-stats)` keys from stage 1 (`live-bytes`,
`committed-bytes`, `bytes-allocated`, `bytes-reclaimed`, `collections`, `minors`, `majors`, `pause-max-us`,
`mmu-10ms`); today's `last_pause_micros`, computed and never read, gets a reader.

---

## 14. Testing and verification

**Differential lanes** extend `scripts/run_gc_differential.sh`: every mode (off, default, stress, zeal, move-all,
two-mutator, GC workers) must be byte-identical to off on the chibi suite (`EXPECTED_TOTAL` 1226 [S]), in release and
debug-poison builds, on both backends wherever the mode applies; port-finalization tests stay outside. The lane table,
the named tests and the rewritten reclamation proofs live in `docs/TEST_ORGANIZATION.md`, each joining it in the stage
that creates it. **Reclamation proofs** assert representation-independent keys (`live-bytes` after a full `(gc)`,
`committed-bytes` across churn, `collections`, and `bytes-reclaimed` > 0 as a guard against vacuous passes), and move
in the change that first counts bytes in the trigger (stage 1).

**The heap verifier** (`PATINA_GC_VERIFY=1`, before and after each collection under zeal) checks every heap-bit word
against a start granule of a matching kind, `END` bits against sizes, invariant W, remembered-set completeness (every
old granule holding a young reference is logged, in a range, or on the remember-whole list; cells armed after a
generational major), no young reference below a watermark, a map at every frame pc, no reachable `FORWARDED`,
`GC_POISON` or `DEAD_SLOT` word, `live_granules`, code units, `HASH_MOVED` extensions and the LOS young list.
`PATINA_GC_VERIFY_ROOTS=1` also checks every `RootScope`, `Owned` and `RootProvider` word.

**Poison under lazy sweep.** From stage 5b the debug-poison and zeal lanes add an eager metadata-driven poisoning sweep,
a 2-collection quarantine of freed holes, `PROT_NONE` on wholly free 4 MiB runs in single-interpreter zeal lanes, the
accessor poison assertions and `DEAD_SLOT` fill, so a stale reference panics instead of reading reused memory (today's
release build silently printed `(45294)` [P]); these lanes compare program output, never identity-hash values.
**Miri** runs on `patina-gc` and on a `patina-core` subset (funnel, `HeapSlot` slices, slot visitor, `Mutator` access,
`RootScope`/`Owned`, the JIT-entry handoff on the Rust twins), with strict provenance.

**Contract tests:** `Mutator` offsets and `GcAttrs`; emitters against their Rust twins once the spike exists; a
call-graph test of the helper table (no `Leaf` reaches a `Transfer` function, no `NoAlloc` reaches an allocation);
embedded addresses limited to §8.2's three kinds; no `Drop` in layout payloads; trybuild tests of the brand; the
conformance suite (§11); the `InterruptHandle` tests.

**Scoreboards** gate every stage: both chibi scripts; the matrix on both backends with `escape_from_primitive.rs` and
the raise-site tests; the hygiene matrix; `ephemerons.rs`, `finished_forms_release_code.rs`, `gc_vm.rs`,
`gc_tree_walker.rs`, `unclosed_output_ports.rs`; `vm_callprimitive.rs` with its two set-after-use tests;
`parameters.scm`; the error-location tests in `interpreter_api.rs`; `run_suite_oracles.sh` with `DIVERGENCES.tsv`;
both Larceny lanes; `patina-compat check-smoke`; `check_embedding_features.sh`.

**The GC benchmark set (GBS):** twenty measured workloads in eight groups (allocation, mutation, large live heap,
flonum, continuations, deep recursion, library loading, supplementary), most of them Larceny-derived and so run from
`~/Project/reference/larceny` (LGPL, not vendored, skipped loudly when absent), plus vendored Patina-authored probes
(among them `large-live` at 0.5 and 1 GiB live, `deep-descent`, `deep-unwind`, `samedepth1000`,
`retained-continuations`, `frag-mix`, `port-churn`, `blocked-threads`), the barrier programs, fixnum twins, I/O
workloads and a tree-walker subset, run through a `gc` mode of `scripts/benchmarks.py`. The list and per-run metrics
live in `docs/TEST_ORGANIZATION.md`. **Statistics:** ABA ordering over at least 10 rounds; thresholds under 2% judged in
instructions retired or cycles, never wall time; bootstrap 95% confidence intervals, a gate passing only if its interval
clears the threshold; `PATINA_GC=null` as the lower bound.

**Named experiments.** **M2**, the interpreter's barrier tax: the barrier armed with minors off (every object treated as
old, bits re-armed and the buffer drained at each collection), against `BarrierKind::None`, in instructions, on the
barrier programs and the GBS; the bet is under 1% (K2). **M5**, the generational question: generational on; barrier on
with minors off; barrier off and whole-heap; each with the bypass on and off, on the GBS and the tree-walker subset.
Generational must win on throughput, or on per-workload max pause and MMU(10 ms) without losing throughput, to become
the default (K1).

**Each stage proves itself** by filing its issue first, keeping every lane and scoreboard green on both backends,
showing its acceptance numbers by interleaved A/B with confidence intervals, keeping the verifier clean under zeal, and
recording any behaviour change as an oracle-scored `DIVERGENCES.tsv` row.

---

## 17. Owner decisions

These are the open questions the research raised that this design depends on. Each has a **proposed default** and the
consequence of the alternative; once answered, the table records the decision.

| # | Decision | **Proposed default** | Consequence of the alternative |
|---|---|---|---|
| 1 | Tree-walker role | *Answered:* kept; may lag. **Whole-heap, non-moving, pinned roots, host payloads, collects at the outermost trampoline; library bodies do not collect between forms** | Full parity (heap frames) would cost 6–12 weeks and make tree-walker `define`s the dominant barrier site (12.48 M on nboyer [P]) |
| 2 | Global rebinding semantics | **Variant R at stage 4b** (no change: binding records and link tables, §8.6); **recommend C after stage 5** (chibi, Chez, Racket), with `DIVERGENCES.tsv` rows. 4b already deletes `GlobalCacheEntry`, `frame_globals` and `VmClosure.globals`; C also deletes the records' indirection, the shadow bitsets and `mark_if_*`, but **keeps per-binding guards**, because a program's `set!` of an imported primitive still writes the library's shared cell (#406): `CallPrimitive`, the inline operations and `JumpUnlessCellHolds` take their fast path only while `cell.value == expected`, keeping the tail-shape deopt; JIT sites rely on `WATCHED` (§7). The two set-after-use tests stay green; six define-after-use tests flip | Staying on R costs the JIT a second dependent load per global, or `WATCHED` on record and cell. C gives one-load JIT globals and guards that recover when a program restores the procedure (today's shadow bit never clears) |
| 3 | When `define` rebinds; mid-program imports | **Case by case**, over six shapes (p7, p8, p9, q1, q2/q3, d1) whose answers under R and C, oracle followed and `DIVERGENCES.tsv` rows are filed at stage 0 as the rebinding suite file. Under R every case answers as today and each oracle that disagrees gets a row now; under C most cases follow all the oracles, and p9 follows chibi | The oracles do not agree on every case (p9), so no single rule such as "bind at expansion time" describes C |
| 4 | Value encoding | **§2:** 61-bit fixnums, exact heap bit, 4-bit heap tags, sign-symmetric self-tagged flonums (subject to K6 with inline flonum operations in both arms), raw addresses | NaN-boxing loses 61-bit fixnums; boxed flonums keep 19.8% of allocations; offsets add one `add` per access |
| 5 | Strings and bignums | **UTF-32 inline** (O(1) `string-set!`; the 252 heap mentions in `strings.rs`/`characters.rs` keep working); **inline bignum limbs** | UTF-8 halves string bytes but rewrites the string primitives; `num-bigint` payloads would need finalization |
| 6 | Pauses | *Answered:* throughput first, stop-the-world, bounded. **"Bounded" means one budget form for both kinds of pause (§6.11):** (1) a minor ≤ c_min + a·(frames above the watermarks) + d·(logged granules) + b·(survivor MiB), with c_min ≤ 0.25 ms and d ≤ 10 ns per logged granule; stage 7b measures both and derives the store buffer's soft limit from d (128 K granules at 10 ns); 2 ms is the expected per-workload max on the GBS, not the budget; (2) a major ≤ c + a·(frames in all rooted stacks) + b·(live MiB), with c ≤ 1 ms, a ≤ 15 ms per million frames (≈13 ns measured) and b ≤ 0.35 ms per live MiB single-threaded (3–8 ns per object at the 26.9 B mean), divided by the worker count under stage P; stage 5a measures a, b and c; (3) **no pause term grows with dead objects or with heap size**: the catch-up sweep, block classification, LOS release, finalizers and decommit run after the world is released (§6.1, §6.10); (4) the GBS, `deep-descent`, `deep-unwind` and `large-live` (0.5 and 1 GiB live) check the budgets by max pause and MMU(10 ms). At 1 GiB live, about 40 M objects, a single-threaded major is about 120–320 ms: inside the budget, and the reason stage P is budgeted | A hard latency target would add incremental marking through `barrier_mode`; parallel marking (stage P) shortens majors without one |
| 7 | Shared-memory parallelism under SRFI 18 (open) | **Not a goal now:** M:1 with N-ready interfaces and the listed `threaded` obligations. Parallel GC marking (stage P) does not depend on this decision | **Yes, shared memory** → an M:N program later. ST's per-item estimate puts real shared-heap threads at 9–18 engineer-months from today's code; stages 1–9 deliver its heap redesign, heap call-site migration, precise rooting, per-mutator state and shared code store, which leaves an estimated 4–9 engineer-months [I] (the `Rc`/`RefCell` migration outside the heap, handshakes, §12's `threaded` obligations, concurrency testing and performance recovery). Nothing here needs undoing; MMTk's parallel GC would be re-examined. **Isolates instead** (§12) → one interpreter per OS thread, sharing nothing, about 3–5 weeks (ST); no `threaded` obligation applies; parallelism for share-nothing workloads only, not for SRFI 18 threads |
| 8 | JIT frame model and tiers | **S1 is the only baseline**; tier 2 publishes tagged values at every suspension point; no tier with native maps; native call/ret only as a separately designed stage if K11 fires | S2 needs native-stack abandonment at every `Transfer`, a depth cap and its own gates; native maps need a walker and deoptimization at capture |
| 9 | Async interrupts in JIT code | **Yes**: free at calls, 2 instructions at back-edges, serviced through `Transfer` returns; posted through a per-heap `InterruptHandle` that teardown invalidates safely (§9), and the REPL's SIGINT handler uses one | No → back-edge polls go (0.3–1.9% saved in tight loops) and Ctrl-C fails in tight loops |
| 10 | Identity hashing | **Side-metadata `HASHED` + BFG extension on move; heap-base-relative** | Pin-on-hash blocks evacuation on eq-table heaps; GC-rehashed native tables mean rewriting SRFI 69/125 in Rust |
| 11 | Weakness scope | **SRFI 124 + 254 ephemerons now; SRFI 125 weak tables over ephemerons; guardians and transport cells after stage 7; strong symbol table** | A weak symbol table adds one epilogue step |
| 12 | Port finalization | **Flush and close at GC, `EMFILE` collect-and-retry where collection is possible, GC timing declared observable** | Without it R2 and R3 stay divergent from chibi and Gauche, and descriptor exhaustion stays |
| 13 | Embedding API | **`Owned` handles (heap id, generation) and branded `with(&mut self)`; `Interpreter::call` as a driver entry that may collect; host primitives through `register_primitive`/`register_resumable`, branded, with a declared helper class, calling procedure arguments only through `Step::Call`; `interrupt_handle()`; mandatory teardown; many heaps per process; `Backend` migrated one step per stage. Stage 3, except handles and teardown (stage 2)** | Scoped-only handles block long-lived host references; without `call` a host must evaluate source text to call a procedure; without a declared class the JIT treats every host primitive as `Transfer`; without teardown every dropped interpreter leaks a reservation |
| 14 | Dependencies, MMTk | **No MMTk; `patina-gc` depends on `libc` only.** Before any spike, check the free gates: upstream multi-instance support (#100), dependency approval, whether process-per-test is acceptable | An MMTk spike is 4–6 weeks, needs crate downloads, and lives outside the default workspace |
| 15 | Footprint and limits | **Whole-heap interval `max(8 MiB, 2·L)` (about 3× live at peak); √L only for generational majors after an A/B; decommit with 2-major hysteresis, in 4 MiB runs, after the pause, for heap blocks, register stacks and store buffers; `max_heap` the smaller of 16 GiB and 75% of RAM (`PATINA_HEAP_MAX`/`--heap-max`); main register stack 8 GiB, other green threads 256 MiB (`PATINA_STACK_MAX`/`--stack-max`); exhaustion raises `&heap-exhausted` (after one collect-and-retry for user-sized requests and captures) or `&stack-exhausted`, a non-zero exit when uncaught; MemBalancer opt-in** | Today only RAM limits a program, so these defaults are new limits; larger ones cost VA per heap (318 heaps × 16 GiB is already about 5 TiB), and none at all would need chained reservations (K13's fallback, one more load in the barrier). √L without a nursery costs 1.15–2× more full marking and a 32 MiB first major; adaptive pacing by default costs lane determinism |
| 16 | Front-end sequencing | **Unread provenance stores deleted in stage 1; identifiers as ids and inline provenance (head-identifier form spans) in stage 4c, before the new heap; lazy scope propagation as a parallel front-end project** | Later: libload keeps 316 MiB of provenance, and syntax objects keep `Drop` |
| 17 | syntax-case timing | **After stage 5:** an `ExpansionContext` root provider with a literal pool and epoch-checked memos retires the last `NoGcScope` at point C; sooner if K16 fires at point C | Earlier: expansion needs rooting before the funnel exists |
| 18 | Continuation end state | **Design A** (stage 4e); C′ only if post-JIT capture-at-depth benchmarks demand it | C′ earlier: O(1) capture sooner, at medium-high risk to the matrix |
| 19 | Recording divergences | **Record now the rebinding cases of decision 3 under R (no row records them today); then the port `eq?` fix, flonum `eq?` and flonum-keyed ephemerons (if the oracles differ), GC-observable port flushing, C's rows if adopted, and decision 23's row if its alternative is chosen** | — |
| 20 | JIT code memory | **Own `MAP_JIT` reservation; W^X and icache maintenance only in `install_code`; replaced bodies freed by epoch when no native activation remains; per-unit freeing after a complete major; BTI landing pads if enabled; a dual mapping on Linux before OS-thread carriers** | `cranelift-jit`'s `JITModule` cannot free single functions, so long REPL sessions would grow |
| 21 | Budget order | **Representation first** (stages 1–5), algorithms second (7–8) | Collector-first builds an algorithm on 72 B slots and `Rc` payloads that must be rewritten anyway |
| 22 | This document's home, and the split | **Part I replaces the body of `docs/GC_DESIGN.md` and Part II the body of `PRD/future/GC_STAGE5_PRD.md`** (no new markdown file); Part III becomes the issues filed at stage 0 (S0 carries the measurement table and the holder inventory, ST the threading model, and the rebinding cases become a suite file with its `DIVERGENCES.tsv` rows), its test appendix goes into `docs/TEST_ORGANIZATION.md` lane by lane, and Part IV becomes the PR description. **Approving this approves the split as one documentation-only PR that** renumbers Part I's sections consecutively; turns every K*n* in Part I into a link to the anchor `#kn` that the matching row of Part II carries, and every §15 or §16 mention into a link to that file's stage list or kill-criterion table; replaces each S-label with its issue link once S0 is filed; and checks links and paths in both files beside the `git diff --check` AGENTS.md asks for | A new file needs explicit approval under AGENTS.md; one long file would repeat the Track L record AGENTS.md warns against; committing the text as written would leave Part I jumping from §14 to §17 with references into sections that live elsewhere |
| 23 | Threads blocked for ever | **Rooted until they terminate**, through the scheduler's root provider (§12). Gambit (thread groups) and chibi (its list of paused threads) both keep such threads, so no `DIVERGENCES.tsv` row is needed; the `blocked-threads` probe (100 K threads blocked for ever on fresh mutexes nothing else reaches, reporting retained memory) is scored by hand against both, as decision 3's rows are | Collecting a thread that is blocked with no timeout on objects nothing else reaches frees its stack and table entry through `FinalKind::Thread`, but diverges from both oracles (observable through Gambit's `thread-group->thread-list`, and through memory under SRFI 18 alone) and needs a `DIVERGENCES.tsv` row against each |

---

# Part II. The plan (→ `PRD/future/GC_STAGE5_PRD.md`)

## 15. Migration plan

**Rules for every stage.**
- A GitHub issue first, carrying the defect narrative and the work-item detail: scope, files, numeric acceptance
  and effort. The PR closes it, and this list keeps one line per stage.
- `docs/GC_DESIGN.md` is rewritten in place. Every other repository document whose rule a stage supersedes is
  updated by that stage (the "Docs updated" column), and each new test lane is added to
  `docs/TEST_ORGANIZATION.md` when it is created.
- Behaviour changes are scored against chibi and Gauche (and Gambit for thread semantics) and recorded in
  `DIVERGENCES.tsv`.
- Each semantic change is **its own PR**, never folded into a representation step: port identity and GC-time
  flushing, deep-bound `parameterize`, flonum `eq?` and flonum-keyed ephemerons, variant C.
- **No stage removes a safety property before its replacement has landed**: `Rc` code liveness stays until traced
  code liveness (4e); nested-loop deferral stays until the weak continuation tables are gone (4e); dead-slot
  clearing stays for good.
- Embedding-API changes follow #601's deprecation convention, one step per stage (S2, S3).
- **Every stage ships measured value or stays neutral within its gate.** No geomean regression above 1% unless the
  stage declares a budget; regressions and gains under 2% are judged in instructions or cycles with confidence
  intervals (§14).

**Stages.** "S*n*" is the issue filed for stage *n* at stage 0; its link replaces the label once it exists.

| # | Stage | Gate | Docs updated | Issue |
|---|---|---|---|---|
| 0 | Ground truth: a GC mode for the benchmark runner, the GBS with `large-live`, the pause/MMU log, K16's counters, the `gc-census` feature, the vendored probes; the defect, stage and threading-model issues; the measurement table and the off-heap holder inventory on the tracking issue; the rebinding suite file | the measurement table's baselines reproduced within their confidence intervals; no behaviour change | `docs/TEST_ORGANIZATION.md` (the `gc` benchmark mode, the GBS list, the census feature); AGENTS.md drift ("NaN-boxed", "24 transfer shapes") | S0 (tracking), ST |
| 1 | Quick wins on today's collector: byte trigger, `(gc)` collects at its call, `EMFILE` collect-and-retry, the unread provenance stores deleted | trigger blindness and descriptor exhaustion fixed; reclamation proofs non-vacuous; GBS ±1% | `docs/TEST_ORGANIZATION.md` (the rewritten reclamation proofs); `PRD/future/TREE_WALKER_HOOK_SYSTEM.md` §6 and `PRD/future/VISUAL_DEBUGGER_DESIGN.md` (the `locations` store is gone, so `prune_freed_locations` becomes a deprecated no-op, removed at 5e, and drivers stop calling it) | S1 |
| 2 | Root and boundary contract: slot visitor, `CallFrame.closure` as a value, `Owned` handles, heap teardown, top-level import hoisting, rooted loading | embedder use-after-free and teardown leak fixed; library bodies loaded through `import` collect; GBS ±1% | `docs/VM_DECISIONS.md` §4 and §5 (the closure as a traced value; the root inventory); `PRD/FFI_DESIGN.md` (handles in place of exported `HeapIndex` and `SharedHeap`; pinning, `Foreign` finalization through the registry, global handles for callbacks); `PRD/future/TREE_WALKER_HOOK_SYSTEM.md` §5.2 and `PRD/future/VISUAL_DEBUGGER_DESIGN.md` (the `GcRoots` seam becomes `RootProvider`, registered through `RootSet::register`, so `DebugHook: GcRoots` becomes `DebugHook: RootProvider`) | S2 |
| 3 | `Mutator`, the collect capability, `Cx` and the store funnel; polls at frame entry; `InterruptHandle`; `Interpreter::call` and host primitives | ABI, trybuild, interrupt and embedding tests on both backends; `ephemerons.rs:94,109,127`; geomean ≥ 0% in instructions | AGENTS.md (the `RefCell` borrow rule becomes the `Cx` and collect-capability rule); `docs/VM_DECISIONS.md` §6 and §8 (the primitive signature; no `SharedHeap`); `PRD/FFI_DESIGN.md` (plugins through `register_primitive`); `docs/TEST_ORGANIZATION.md` (zeal-entry lane, trybuild tests); `PRD/future/TREE_WALKER_HOOK_SYSTEM.md` §5.2 and §10.1 and `PRD/future/VISUAL_DEBUGGER_DESIGN.md` (`GcDeferGuard` and "no GC while paused" become `NoGcScope` around a paused hook's evaluation, counted by K16; the tier-policy flag of `GC_DESIGN.md`'s tier contract) | S3 |
| 4a | Canonical identity and ports | the five port tests of §6.7 on both backends; I/O workloads ±1% | `docs/TEST_ORGANIZATION.md` (port-finalization tests kept outside the differential lane); `DIVERGENCES.tsv` | S4a |
| 4b | Global cells and binding records (variant R) | the rebinding tests and decision 3's rows answer as today; the global path neutral or better than `frame_globals` | AGENTS.md ("An import installs a binding": records over shared cells replace forwarded slots and `Owner`); `docs/VM_DECISIONS.md` §3 (globals through link tables); `docs/TEST_ORGANIZATION.md` (the import-policy section's mechanism) | S4b |
| 4c | Identifiers as ids, inline provenance | hygiene matrix 139/139; libload `malloc` peak ≤ 450 MiB; caret tests | `docs/MACRO_SYSTEM.md` (`IdentifierData`, provenance) | S4c |
| 4d | Frames and stacks; the stack cap and its knob | matrix 64/64; raise-site tests; `finished_forms_release_code.rs` 9/9; 10 M-deep recursion | `docs/VM_DECISIONS.md` §4 (frames in a reserved stack rather than `derive(Clone)` structs in a `Vec`); `docs/VM_RUNTIME.md` §2.1 and §4 (frame layout, call and return) | S4d |
| 4e | Continuations as heap objects; traced code liveness | the matrix; `finished_forms_release_code.rs` 9/9 with no collection; capture targets | `docs/VM_DECISIONS.md` §4 (no continuation side tables or `VmContinuationRef`); `docs/VM_RUNTIME.md` §2.6 and §6 (continuation objects, roots) | S4e |
| 4f | Tree-walker host payloads | tree-walker lanes; no environment-driven collection on `(fib 25)` | — | S4f |
| 4g | Inline payloads | per-kind census | — | S4g |
| — | Stage-4 exit | the baselines stage 5 is judged against are re-measured: the GBS, the post-load major, the I/O workloads | — | on S5 |
| 5 | The new heap: non-moving, whole-heap, kind by kind; the weak contract; the `max_heap` knob | lanes byte-identical per sub-stage; a request that fits only after a collection succeeds near `--heap-max`, on both backends; exit K9 (§16) | AGENTS.md (the `TaggedValue` description; "New heap object type" becomes a `declare_layouts!` recipe); `docs/VM_DECISIONS.md` §2, §3, §5 and §9 (encoding, inline closures, the collector, inline strings); `docs/TEST_ORGANIZATION.md` (zeal-major, verifier and poisoning-sweep lanes) | S5 |
| 6 | JIT ABI spike and freeze | the spike answers its questions; K6, K7 and K11 decided | `docs/GC_DESIGN.md` (the frozen ABI) | S6 |
| 7 | Sticky generations, behind a switch, with young and old LOS lists | M2 (K2); M5 decides the default (K1); K8; per-workload max minor pause within the budget formula (constants measured in 7b); minors free dead young LOS runs (zeal-minor lane with an RSS bound) | `docs/TEST_ORGANIZATION.md` (zeal-minor lane) | S7 |
| 8 | Opportunistic evacuation | move-all lane green for 4 weeks; K3 | `docs/TEST_ORGANIZATION.md` (move-all lane) | S8 |
| 9 | Threads readiness: the `GreenThread` split; thread, mutex and condition-variable objects with their root rule and `FinalKind::Thread`, on both backends | two-mutator lane green; thread-lifetime tests on both backends; decision 23 recorded; ≤ 1% in instructions | `docs/VM_RUNTIME.md` §5.6 (the `parameterize` rows); `docs/TEST_ORGANIZATION.md` (two-mutator lane) | S9 |
| P | Parallel stop-the-world marking (§11): triggered by measurement, budgeted | starts only if `large-live`'s major at 1 GiB live exceeds 100 ms after stage 5, which the estimate (120–320 ms) predicts; then marking ≥ 2.5× faster on 4 workers, with output identical to one worker | `docs/TEST_ORGANIZATION.md` (workers lane) | SP |
| — | *Optional, each on its own trigger:* copying nursery (K4); C′ (decision 18, K15); boot-image static space; incremental marking (a latency goal); evaluator-owned `StepRoots` for the tree-walker; native call/ret (K11); isolates (decision 7) | — | — | filed when triggered |

Stages 0–9 and P total about 88–122 focused engineer-weeks [I]. Stage P is in the total because the pause estimate
(`GC_DESIGN.md`, pause budgets) says its trigger will fire. The per-stage scopes, file lists, numeric acceptance and
estimates are work-item detail kept in the stage issues, not in this list.

---

## 16. Risks, mitigations and kill criteria

All measurements are interleaved A/B with confidence intervals (§14), on the programs the second table names.

| # | Risk | Mitigation | Kill criterion → change of course |
|---|---|---|---|
| <a id="k1"></a>K1 | Generational collection loses (Wingo on nboyer and splay; Go's rejection of generational barriers; queue3 and deeprec). The counterweight is HotSpot, where every collector family went generational: generational ZGC (JEP 439), whose non-generational mode JEP 490 removed, and generational Shenandoah (JEP 521) | a per-heap switch; bypass with hysteresis; strided watermark | sticky generations become the default only if the GBS geomean in cycles is ≥ 0% (or ≥ −0.5% with the confidence interval excluding −1%), **and** no GBS workload regresses by more than 3% in cycles, **and** the pause benefit holds per workload in max pause and MMU(10 ms). A pause win cannot buy back a throughput loss, and pooled percentiles are never used (with many minors, p95 is a minor pause by construction). Otherwise whole-heap stays the default, generational stays opt-in for one release, then is deleted unless some workload gains ≥ 5% |
| <a id="k2"></a>K2 | Barrier tax | the one-bit filter; static elision; `letrec*` unboxing removes most `WriteCell`s; range entries for bulk stores | M2 > 1.5% in instructions → profile; > 3%, or JIT field logging > 2× the card cost on vecsort/hashtab → `BarrierKind::Card` with a young filter (invariant W makes card scanning sound) |
| <a id="k3"></a>K3 | Fragmentation before stage 8 | free reserve, medium overflow, granule holes, livelock rule | fragmentation (§6.5) > 10%, or occupied-block bytes over marked bytes > 1.5, at steady state on any workload, or a `frag-mix` livelock → stage 8 moves ahead of stage 7. (Committed/live is not used: pacing and the reserve keep it above 2 by construction) |
| <a id="k4"></a>K4 | The sticky nursery loses to a copying one | empty blocks freed without a scan; word-parallel sweep | after stage 7, lazy sweep plus refill > 4% of mutator time on any churn workload (deriv, destruc, fibfp, mbrot, generator, ctak, fibc, libload), or a prototype bump nursery reclaims ≥ 2× more bytes per GC-ms → build `NurserySpace` (§11). The prototype is scheduled only if the first condition is close (> 2%) |
| <a id="k5"></a>K5 | Writing `END` at mark time proves bug-prone | verifier; debug start-bitmap | two verifier escapes reach `main`, or mark time +3% → write begin/end metadata at allocation (Whippet; +2 stores in the fast path) |
| <a id="k6"></a>K6 | Self-tagged flonums do not pay | canonical boxes; constant folding; inline flonum operations first | immediate share < 70% on fibfp/mbrot/nucleic; or, **with inline flonum operations in both arms**, bytes allocated on those kernels not at least halved, or their cycles improved by less than half of the allocation-and-GC share their profile shows; or the spike's JIT float microkernel gains < 1.3×; or non-float cost > 1% in instructions → revert to 16 B boxes and keep `010`/`011` reserved. Decided before the stage 6 freeze |
| <a id="k7"></a>K7 | The 2-load call or the limit-fold poll costs too much | spike measurement | spike: 2-load call > 3% slower in cycles than a 1-load entry on fib/tak/nboyer → entry blocks with a branch patched only inside `install_code` (with icache maintenance). Tick mode > 1% over the fold → Tick only in deterministic-scheduler builds |
| <a id="k8"></a>K8 | The watermark return barrier breaks transfers or costs | one `return_into` choke point; strided lowering; matrix rows | any matrix row red under zeal-minor for > 1 week, or > 0.5% in instructions on deeprec or `deep-unwind`, where the barrier fires (fib/tak/nqueens at ≥ 1 s inputs are the control, where it should not) → raise the stride, or disable it (minors scan whole stacks; still correct), or enable it only above 10 K frames |
| <a id="k9"></a>K9 | B1 is wrong: representation does not pay | stage 5 is kind by kind and can stop | **Stage 5's exit gates**, every one against the stage-4 exit baseline (§15), never against today's measurement table: GBS geomean ≥ 5% faster (cycles, with CI); on **every** GBS workload GC CPU ≤ stage 4 + 5% and peak RSS ≤ max(stage 4 + 10%, stage 4 + 5 MB); peak RSS ≥ 30% lower on libload, gcold and queue3; gcold (RSS − 11 MB floor)/pacing target ≤ 1.15; queue3 max pause < 15 ms (41 today); the post-load major (a `(gc)` right after libload's imports) at most half its stage-4 baseline, with decommit outside the pause; memory returned after a load visible in resident size and footprint; `Drop`-carrying allocations ≤ 1% (census). Kill: < 5% geomean **and** < 30% peak-RSS gain, or any per-workload GC-CPU or RSS gate failed without a fix → stop and re-plan around a copying nursery over a mark-in-place old space |
| <a id="k10"></a>K10 | The strangler mixes two heaps badly | one combined marker; per-kind sub-stages | a sub-stage cannot keep lanes byte-identical within two attempts, or mixed-trace pauses > +10% → convert the remaining kinds in one flag-day PR behind a temporary feature, deleted at 5e |
| <a id="k11"></a>K11 | S1 trampolined returns are too slow | none inside this contract | spike: S1 > 8% slower in cycles than a native call/ret estimate on fib/tak/nboyer → open a **separate S2 design**, priced and gated on its own: native call/ret only between non-`Leaf` events; every `Transfer` helper, poll or collection abandons the native stack back to the driver through an SP-reset stub written outside Cranelift, and frames resume through their `(code, pc)` resume entries; a native depth cap that falls back to S1; the watermark check on the driver's resume path; gates: the matrix, 10 M-deep recursion, the two-mutator lane and zeal-minor. The GC contract of this document does not cover S2 |
| <a id="k12"></a>K12 | Tier 2's rule (only tagged values at suspension points) costs too much | re-tagging is 1–6 instructions; out-of-band doubles boxed through a `Leaf` allocation | tier-2 measurements show > 15% loss on call-heavy float code → first more `Leaf` helpers and inlining; native stack maps only through a separately approved deopt-at-capture design that amends §11's exclusions |
| <a id="k13"></a>K13 | VA reservations fail on CI (Linux overcommit, map count) | RW + `MAP_NORESERVE`; one VMA per heap; `MADV_DONTNEED` decommit (no VMA split); the many-heaps probe | any reservation failure → 4 GiB default with chained reservations and a per-chunk `meta_bias` (+1 load in the barrier) |
| <a id="k14"></a>K14 | Schedule overrun | each stage has standalone value | stage 5 at 2× its estimate → ship whole-heap `MarkRegion` (still the full representation win); defer 7 and 8 |
| <a id="k15"></a>K15 | C′, if pursued, regresses | design A first, same frame format | > 10% slower than A on deeprec or generator, or a matrix row unrestored after 4 weeks → stay at A |
| <a id="k16"></a>K16 | B2 is wrong in practice: a window that cannot collect lets garbage grow (one huge form expanded under `NoGcScope` at point C or inside nested tree-walker trampolines; a Rust primitive building a large structure; polls deferred under `NoGcScope`) | `NoGcScope` confined to point C, the residual `apply_proc` fallbacks, nested tree-walker trampolines and a paused debugger's evaluation; overdraft with an emergency reserve; `GcStats` keeps the high-water of bytes allocated since the last serviced poll and of bytes allocated under one `NoGcScope`, with the site (§13) | measured from stage 0 on the GBS, libload and both Larceny lanes: any window above the larger of 64 MiB and 25% of the pacing target, or any lane reaching `HEAP_EXHAUSTED` → add a collection point at that site: bring decision 17's `ExpansionContext` root provider (a literal pool and epoch-checked memos) forward for point C, make the primitive resumable so it builds across `Step::Collect`, or root the nested tree-walker trampoline with evaluator-owned `StepRoots`. Allocation itself never becomes a collection point |

**Programs and metrics for each criterion.**

| Criterion | Programs | Metric |
|---|---|---|
| K1 | the GBS, the barrier programs | cycles: geomean and per workload, with CIs; per-workload max pause and MMU(10 ms) |
| K2 | vecsort, hashtab, queue, tree, strport, letrec, plus the GBS (M2); the spike's store microkernels | instructions (interpreter); cycles (JIT) |
| K3 | `frag-mix` and the GBS | fragmentation and occupied/marked after marking |
| K4 | deriv, destruc, fibfp, mbrot, generator, ctak, fibc, libload | refill and lazy-sweep time over mutator time (GC log) |
| K5 | verifier lanes; the mark microbenchmark | escapes; mark ns |
| K6 | fibfp, mbrot, nucleic and their fixnum twins; the spike's float microkernel | immediate share; bytes allocated; collections; cycles |
| K7 | spike: fib, tak, nboyer at ≥ 1 s | cycles |
| K8 | deeprec, `deep-unwind`; control: fib, tak, nqueens at ≥ 1 s | instructions; cold-path returns |
| K9 | the GBS, the I/O workloads, `small-heap`; the post-load `(gc)`; the stage-4 exit baseline | cycles; GC CPU; peak RSS, footprint and resident size per workload; post-load max pause |
| K10 | lanes; mixed-trace pauses | byte identity; max pause |
| K11 | spike: fib, tak, nboyer | cycles |
| K12 | tier-2 float kernels (later) | cycles |
| K13 | `many-heaps` on Linux and macOS CI | reservation failures |
| K14 | — | elapsed engineer-weeks |
| K15 | deeprec, generator; the matrix | cycles; rows restored |
| K16 | the GBS, libload, both Larceny lanes (R7RS and the `--r6rs` emulation), the tree-walker subset | `GcStats` high-water bytes since the last serviced poll and under one `NoGcScope`, per site; `HEAP_EXHAUSTED` events |

**Risks without a numeric threshold:**
- **Hidden references** (Julia's lesson). Mitigation: the off-heap holder inventory (S0), the verifier, `VERIFY_ROOTS`,
  and the poison and move-all lanes.
- **Single-mutator shortcuts creeping in.** Mitigation: the §12 rules in `GC_DESIGN.md`, the two-mutator lane, the
  `threaded` feature linted in CI, and review of new `thread_local!`s and heap payloads.
- **Port-finalization flakiness.** Mitigation: those tests are kept out of the byte-identical lane.
- **Raw addresses turning rooting bugs into undefined behaviour.** Mitigation: collection only through the
  `&mut Heap` capability (§8.3), `#![forbid(unsafe_code)]` in the safe crates, release range checks at trust
  boundaries, the debug-poison sweep and quarantine, `PROT_NONE` from-space, the verifier and Miri.

---

---

# Part III. Work items, inventories and test organization (→ the stage-0 issues, and lane by lane `docs/TEST_ORGANIZATION.md`; not committed as a block)

## Appendix B. Measurement provenance (→ the S0 tracking issue)

**Setup.** Every **[P]** number in Part I was measured on one machine: Apple M4 Pro, macOS 27.2 arm64, 16 KiB pages,
release build at `28a94f8`. The 20 GBS workloads (§14) are Larceny R7RS and GC benchmarks at reduced inputs, plus three
Patina-authored programs: `deeprec`, `libload` (25–26 R7RS-large libraries) and `eqtable`. Numbers marked "probe" come
from small C and Rust programs written while this design was reviewed; the `madv.c` result was re-run while revising it.

The harness, the census and the probes lived outside the repository. Stage 0 brings them in: the harness as the `gc`
mode of `scripts/benchmarks.py`, the census as the `gc-census` feature, and the probes under `scripts/gc_probes/`.
Stage 0 then re-measures this table before any later claim relies on it.

| Number | Workload or probe |
|---|---|
| 2.05× bytes against a headered layout; 56% of objects exactly 16 B; 99.2% ≤ 128 B; 148 objects > 8 KiB; closures 35.3%, pairs 27.9%, flonums 19.8% of allocations | allocation census, 348 M objects over the 20 workloads |
| survival ≤ 1.5% on 10 of 20 workloads; nboyer 43% (byte survival 37.3% at 64 K objects, 32.1% at 256 K; dead-old 76.5%); mperm 58%; queue3 and deeprec 100% | survival census |
| immediate-value filter removes 64% of heap stores; 99.4% of the median workload's stores hit young holders | store-mix census |
| field-logging slow paths: nboyer 5, queue 96, tree 46 K, vecsort 51 K, hashtab 131 K; Chez store buffer 0.5–1.0 entries per barrier store; OCaml ref table 24,504 entries on nboyer | barrier simulation |
| pauses of 41 ms (queue3) and 178 ms (first collection after the library load: about 18 ms of it is sweeping 7.5 M slots, the rest releasing 2.9 M `Drop` payloads and pruning provenance); a `(gc)` right after that collection, over the same slots with nothing left to release, 14.8–15.4 ms; with collection off until the end, the first collection takes 201.5 ms and frees 269.9 MiB; sweep 2.4 ns per slot; mark 2.8–3.0 ns per live pair on a contiguous list, ≈4.7 ns per live vector on queue3, 8.1 ns per live closure; deeprec 12.3 ms mean mark over 960 K frames and 11.5 M registers | collector instrumentation |
| peak RSS today: empty program 11 MB, deriv 15 MB, destruc 18, fibfp 17, mbrot 18, quicksort 23, generator 25 | baseline runs of the GBS, no instrumentation |
| live heaps peak at 50 MB (queue3), 41 MB (mperm), 26 MB (deeprec) and 22 MB (nboyer) in the headered layout; deeprec reaches 960 K frames | per-collection live-heap census |
| 10 M-deep non-tail recursion: 0.50 s and 1.37 GB, about 137 B per frame | deep-recursion probe |
| 414 MB and 0 collections | 500 × `(make-vector 100000)` |
| 628 MB RSS with ≤ 12 MB live | gcold |
| `EMFILE` at the 1,021st open | 100 K unclosed file opens under `ulimit -n 1024` |
| 5.7 GB RSS | 80 K captures at depth 1000 |
| today's capture at depth 1000: **24 µs and about 171 KB per capture** (20 K captures reach 3.4 GB RSS) | `samedepth1000` |
| the continuation toy's model of today at depth 1000: 5.0 µs per capture and 64–80 KB retained (the toy, not Patina); design A about 5.8× below the model's capture overhead at depth 100 (755 → 130 ns over a 430 ns baseline), **without the per-frame dead-slot pass and with 24 B frames**; C′ 57 ns per capture at depth 1000 | the continuation toy, `scripts/gc_probes/continuation_toy.rs` |
| RSS 627 MiB; `malloc` peak 765 MiB, of which provenance is 316 MiB; 464 of 501 MiB empty capacity after the load; rbtree 211 → 118 MiB with between-form collection (inlined program) | library-load census |
| 1.66 M identity-hash calls on 500 K pair keys | eqtable |
| 289 ms in the bad order against 0.24 ms in the good order | 16 K-ephemeron chain |
| +55 MB of mark stack | 2 M-element list of 2-vectors |
| 318 live heaps in one test process; 4,096 × 16 GiB reservations succeed; `MADV_FREE` + `PROT_NONE` returns RSS at once | process and `mmap` probes |
| `MADV_FREE` alone and `MADV_DONTNEED`: 258 → 258 MB resident; `MADV_FREE_REUSABLE`: footprint 257 → 1 MB, resident unchanged; `MADV_FREE` + `PROT_NONE` and `mmap(MAP_FIXED)` over the range: 258 → 2 MB | probe `scripts/gc_probes/madv.c` (256 MiB touched in a 16 GiB RW `MAP_NORESERVE` mapping), re-run for this revision |
| 8 MiB allocate-and-write cycle: 0.23–0.32 ms without decommit, 0.71 ms with per-block decommit, 0.51 ms coalesced (≈25–60 µs per re-committed MiB); 464 MiB decommitted in 32 KiB pieces: 14.6 ms of syscalls, in 4 MiB runs: 1.8 ms | probes `scripts/gc_probes/cycle.c` and `decommit.c` |
| fibfp 1.42–1.44 s and 6.0e9 cycles against its fixnum twin's 0.49 s and 2.04e9; generic numeric dispatch ≈51% and allocation plus GC ≈8% of fibfp's samples; instructions retired stable to ±0.003% over 5 runs | fibfp's fixnum twin (GBS); a `sample` profile of fibfp; `/usr/bin/time -l` |
| `MAP_JIT` with per-thread write protection: 4,096 regions of 256 MiB and 400 of 1 GiB mapped and executed | probe `scripts/gc_probes/mapjit.c` |
| 2 MiB and 64 MiB anonymous maps land below 2³⁵; a 2⁴⁰ hint is honoured | probe `scripts/gc_probes/placement.c` |
| `(fib 25)`: 3.16 M Rust allocations and 1.09 M environments against 43 heap allocations; about 140 ns per suspended frame traced | tree-walker probes |
| embedder use-after-free: debug panic "use-after-free: pair slot 16002", release prints `(45294)` | `eval_str` result read after a collecting `eval_program` |
| +1.1–1.4% for today's per-instruction safe point | `docs/GC_DESIGN.md` §6.1 (repository document; not re-run) |

---

## Appendix D. Work-item detail for the stage issues

Each row is the first draft of its stage's issue body: scope with crates and files, numeric acceptance
(interleaved A/B where numeric), effort and standalone value. Once an issue is filed it, not this appendix,
is the source of truth, and §15 keeps one line linking to it. Effort is in focused engineer-weeks [I].

| # | Stage | Scope (crates and files) | Acceptance (interleaved A/B where numeric) | Effort | Value on its own |
|---|---|---|---|---|---|
| 0 | **Ground truth** | PR-1 (a `gc` mode for `scripts/benchmarks.py`, the GBS programs and probes including `large-live`, `deep-descent` and `blocked-threads`, the pause/MMU log, K16's high-water counters, the measurement probes under `scripts/gc_probes/`); the `gc-census` feature; issues for the present-day defects (embedder use-after-free, teardown leak, port `eq?`, `EMFILE`, the unrecorded rebinding divergence, with a suite file of decision 3's shapes and their `DIVERGENCES.tsv` rows under R; documentation drift: AGENTS.md "NaN-boxed" and "24 transfer shapes", `gc.rs:25`); one issue per stage, from this appendix; the tracking issue S0 carrying Appendix B's measurement table and E.1's holder inventory; the threading-model issue ST (E.7); the rebinding suite file and its rows (E.2) | no behaviour change; the harness reproduces the measurement table's timing, RSS and pause baselines within their confidence intervals; the census reproduces the allocation, survival and store-mix tables; K16's high-water marks reported for the GBS, libload and both Larceny lanes | 3–4 | every later claim becomes measurable |
| 1 | **Quick wins on today's collector** | PR-2 byte trigger `max(8 MiB, 2·L)`, representation-independent `(gc-stats)` keys, **the rewritten reclamation proofs**, live continuation payload bytes in L; PR-4 `Step::Collect` (`(gc)` collects at its call) and `Step::CollectAndRetry` for `EMFILE` (VM `resume_stub`, tree-walker `ResumePrimitive`), descriptor pressure on opens minus closes; PR-5 delete `SourceMap.locations`, child spans, the throwaway per-expansion `SourceMap` and the `locations` pruning test (`interpreter_api.rs:270`), and make the public `prune_freed_locations` a deprecated no-op (removed at 5e). `patina-core` `heap/{mod,gc}.rs`, `patina-primitives` `primitives/{gc,io/ports}.rs`, `patina-frontend`, `scripts/run_gc_differential.sh`, `tests/common/mod.rs` | 500× `(make-vector 100000)` peak RSS < 100 MB (today 414 MB, 0 collections); gcold peak RSS ≤ 120 MB (628); `samedepth1000` ≥ 10× lower RSS; `port-churn` completes on both backends (fails at the 1,021st open today); `open-close-10k` causes no descriptor-pressure collection; `retained-continuations` does not collect every 8 MiB of capture; libload peak −26 MiB and −355 MiB churn; reclamation proofs green with `bytes-reclaimed` > 0; GBS geomean ±1% | 4–5 | the measured blow-ups fixed now (R3, trigger blindness); `(gc)`'s timing made independent of the poll |
| 2 | **Root and boundary contract** | PR-3 slot visitor over `&self` providers, `CallFrame.closure` as a value, `PENDING_ESCAPE` into the evaluator, tree-walker roots through `pinned`; PR-6 `Owned` handles (heap id, generation, `Weak` table) and teardown: `Heap::teardown()` run from the backend's and `Interpreter`'s `Drop`, which tombstones every arena slot (dropping `VmClosure.globals`, `EnvironmentSpecifier` and `Macro` payloads, so the `Rc` cycle breaks) and flushes ports (`dropped_interpreter_flushes_ports`, Appendix F); **top-level import hoisting**: bare top-level imports processed outside the desugarer, recognized by binding; rooted loading at points A and B (registry `Loading` entry, traced `globals_stack`), so bodies of libraries loaded from top-level imports collect (D); `NoGcScope` at C, around nested VM loops and nested tree-walker trampolines. core, vm, runtime, frontend, interpreter, tree-walker | embedder probe passes in debug and release; teardown probe (the heap is freed on drop; `dropped_interpreter_flushes_ports` passes); `(nieper rbtree)` loaded **through `(import …)`** peaks ≤ 130 MiB (211); lanes; ±1% | 5–7 | embedding is sound; library bodies collect; no leak |
| 3 | **`Mutator`, capability, `Cx` and the store funnel** | `#[repr(C)] Mutator` (accounting and events off `Heap`); polls at frame entry and `Transfer` returns instead of before every instruction, with window initialization and servicing only on complete frames; the `GcDriver` capability and the `Cx<'gc>` codemod over ~1,300 sites, **bottom-up** (core accessors → macros and frontend, deleting `Environment.heap` and `CompiledMacro.heap` → primitives → runtime → VM and tree-walker), allocation on `&mut Cx`, the `RefCell` borrowed per operation; the `pub unsafe` raw-word API; `#![forbid(unsafe_code)]` in the safe crates; `Instance` split; the funnel; `vector_slice_mut`, `get_string_chars_mut` and `get_bytevector_mut` deleted; `#[helper(class, noalloc)]`; the `threaded` feature (accessors only); the zeal-entry lane; the `InterruptHandle` with the REPL's SIGINT handler (§9); `Interpreter::call`, `register_primitive` and `register_resumable` (§8.5) | ABI offset test; trybuild compile-fail tests, including the host-primitive and `call` cases of Appendix F; lanes including zeal-entry; `ephemerons.rs:94,109,127` on both backends; the interrupt and embedding tests of Appendix F on both backends; geomean ≥ 0% in instructions (expect a gain: +1.1–1.4% from dropping the per-instruction poll); the borrow checker fixes the 78 borrow-then-allocate functions | 10–13 | the JIT ABI object exists; who may collect is a type; every mutation is visible; hosts can call Scheme and register primitives soundly; Ctrl-C stops a tight loop |
| 4a | **Canonical identity and ports** | canonical record types, primitives, records and ports; `PortTable`, finalization registry and the process-wide table registry (R1–R6); standard ports as parameter objects with validating converters; `%parameterize-swap!` without callbacks, with the Scheme fallback loop and its `guard` undo; loader retry after `EMFILE` at points A and B | `(eq? (current-output-port) (current-output-port))` ⇒ `#t`; `garbage_port_flushed_by_collection`, `descriptor_exhaustion_retries`, `one_port_one_object`, `dropped_interpreter_flushes_ports` and `exit_flushes_every_interpreter` (§6.7) on both backends; `parameters.scm` `(caught 0)`; I/O workloads ±1%; divergence row updated | 4–5 | port semantics match the oracles; no primitive calls back for `parameterize` |
| 4b | **Global cells and binding records, variant R** | cells and binding records in the immortal space; namespaces map names to records; per-`CodeObject` link tables of records filled at load, with placeholder records for unbound names; `LoadGlobal`, `StoreGlobal` and `Define` by link index; a `define` over an import and an import re-point `record.cell` (§8.6); `FORWARDED`, `Owner` links, `GlobalCacheEntry`, the `env_id` cache and `frame_globals` deleted; `Library.exports` → `CellRef`; `VmClosure.globals` deleted, with `CodeObject` carrying its namespace id; shadow bitsets kept | the 6 rebinding tests unchanged; decision 3's suite rows (p7, p8, p9, q1–q3, d1) answer as today, with their `DIVERGENCES.tsv` rows; `set_after_use_deoptimizes` and `control_forms_set_after_use_deoptimize` green; `import_modifiers.rs` and `vm_global_cache.rs` green; an interleaved A/B of the global path against `main`'s `frame_globals` path on fib, tak and nboyer at ≥ 1 s, neutral or better in instructions with confidence intervals (`frame_globals` alone was 2.6% of samples [P]; the new path pays two dependent loads) | 3–4 | cells and link tables: no `Rc` clone, `RefCell` borrow or `FORWARDED` hop on the global path, and no environment held by closures |
| 4c | **Identifiers as ids, inline provenance** (with the frontend) | scope-set interning; identifiers = symbol + scope-set id + source id; per-document location tables, with form spans on head identifiers; `syntax_sources` deleted | hygiene matrix 139/139; libload `malloc` peak ≤ 450 MiB (765) and load CPU −15%; error-location tests: `same.scm:2:3` (`interpreter_api.rs:745-765`) plus new application-form and macro-use-form caret tests, both backends | 3–5 | the largest load-memory cost removed; syntax becomes `Drop`-free |
| 4d | **Frames and stacks** (isolated) | non-relocating reserved register stack; interleaved 40 B frame headers; byte-offset depths; one `depth()` accessor at every `frames().len()` site; window initialization at push; dense per-pc map index with maps at every suspension point (calls, inline operations, raise sites, pc 0); dead-slot clearing in the scan; `Rc` code liveness kept through a per-thread side vector; the verifier's map check; the stack cap with `PATINA_STACK_MAX`/`--stack-max`, `&stack-exhausted`, and register-stack decommit (§8.1). `vm:runtime/{execution_state,control,vm_state}.rs`, `vm:types/code_object.rs` | matrix 64/64 and `escape_from_primitive.rs` on both backends; the raise-in-`guard` collection and capture tests (a collection and a `call/cc` inside a `guard` handler for errors raised by `(car 5)`, `(vector-ref v 99)`, an unbound global and a shadowed `+`, both backends); `finished_forms_release_code.rs` 9/9, including `a_captured_continuation_keeps_its_form_s_code`; 10 M-deep recursion still works and gives its stack pages back after two observations; `&stack-exhausted` is catchable, and uncaught it exits non-zero; call path ≥ neutral in instructions; map memory measured on libload | 4–5 | a JIT-ready stack, without changing the interpreter protocol or code lifetime |
| 4e | **Continuations A and traced code liveness** | value-only `CONT` objects; code descriptors as heap objects with inline constants (arena slots until 5c); frame `code` words and procedure word 0 as descriptor references; units with an `escaped` bit that every holder sets (`CodeStore::escape`); the `Rc` side vector, VM weak tables and `live_closures` deleted; driver-level nested VM loops (`across_reentry`) audited and made collectable (loops beneath a `Cx` stay deferred); `environment` loads as `Step::LoadLibrary` | matrix on both backends; **`finished_forms_release_code.rs` 9/9, including the two 2,000-form tests with no collection**; capture targets per probe, set from the re-run toy; `samedepth1000` RSS ≤ 200 MB | 5–7 | capture becomes byte-visible; code release no longer depends on sweep; nested loops can collect |
| 4f | **Tree-walker host payloads** | `HostPayloadTable` with young/old lists; tree-walker environments charged when GC-coupled, credited at finalization | `gc_tree_walker.rs` and tree-walker chibi green; tree-walker libload garbage freed by the registry; tree-walker `(fib 25)` triggers no environment-driven collection | 2–3 | no drop-at-sweep left |
| 4g | **Inline payloads** | records, promises, cells and parameters (transitional values-vector layout) as plain fields; macro literal vectors | per kind: no `Rc` payload left in records, promises, cells, parameters, identifiers, ports and continuations (census) | 2–3 | the heap is ready to be moved into blocks |
| — | **Stage-4 exit baseline** | re-measure, at stage 4's exit, the GBS, the I/O workloads and the post-load major (a `(gc)` right after libload's imports): stages 1, 2 and 4c change the 178 ms of today, and K9 compares stage 5 with this baseline, not with today's measurement table | numbers posted on S5 before 5a starts | ≤ 1 | stage 5 is judged against what it actually replaces |
| 5 | **The new heap: non-moving, whole-heap, kind by kind** | 5a `patina-gc` crate (reservation, metadata, SOS, LOS, descriptor and immortal spaces, `MarkRegion`, `NullGc`, verifier, `VERIFY_ROOTS`, the debug-poison sweep and quarantine, Miri on `patina-gc` and a core subset, the heterogeneous mark microbenchmark that fixes a, b and c, the weak and finalization contract of §11 with its conformance suite on `MarkRegion` and `NullGc`); 5b pairs (raw constructors become `unsafe`); 5c procedures and fixed-size headered kinds (symbols, cells, record types, identifiers, descriptors, parameters in their transitional layout); 5d vectors, strings, bytevectors, bignums and the LOS; 5e delete the arenas, the old collector and the `RefCell` (core, VM, tree-walker, compat), allocation on `&Cx`, pacing, the post-pause catch-up sweep and block classification, decommit, `PATINA_HEAP_MAX`/`--heap-max` with the reservation derived from `max_heap`, and collect-and-retry on `Oom` for user-sized requests and capture; 5f inline flonum fast paths (own PR, measured against boxed flonums), then **the final tag map and self-tagged flonums** (isolated, revertible) | per PR: all lanes in off, default, stress and debug-poison on both backends; reservation placement verified. 5e: `large_request_after_garbage` on both backends (with the heap filled close to `--heap-max` by garbage, a `(make-vector N)` that fits only after a collection succeeds, and so does a deep capture in the same state). **Exit:** K9's gates (§16), against the stage-4 exit baseline; `large-live` reported against decision 6's budgets. 5f: K6 | 16–21 | the representation win: about 2× fewer bytes, bump allocation, pauses proportional to live data, memory returned |
| 6 | **JIT ABI spike and freeze** | a Cranelift prototype of about 20 opcodes **on an unmerged branch**, over `Mutator`/`GcAttrs`, driving `cranelift-codegen` into a `MAP_JIT` reservation; it can run beside stage 5. Then freeze the offsets, S1, the helper classes, the frame contract and the entry trampoline in `docs/GC_DESIGN.md` | answers: per-fragment prologue cost; 2-load call cost (K7); limit-fold poll and `Transfer`-return check cost; allocation grouping; inline barrier slow path against a `PreserveAll` stub; the return-barrier trampoline; the cost of results as the first `Tail` argument (`x2`); S1 against a native call/ret estimate (K11); a float microkernel with self-tagged and boxed flonums (K6, JIT side); BTI. Settled already from source: `x21` composes with `CallConv::Tail`; single-bit tests lower to `tbz`/`tbnz` | 3–5 | the JIT track can start without touching the GC |
| 7 | **Sticky generations (a switch)** | 7a barrier armed with minors off (M2); 7b sticky minors; the mark microbenchmark extended with store-buffer entries to measure c_min and d, and the store buffer's soft limit set from d (§6.11), with range and remember-whole granules counted against it; deferred discard and decommit above the soft limit; range entries for bulk stores; cell re-arming at majors; green-thread slots as minor roots; the strided watermark return barrier; young lists, including the LOS's young and old lists with dead young runs released after every minor; 7c adaptive bypass with hysteresis, and backstops; √L majors only after their A/B; **M5 decides the default (K1)** | M2 interpreter tax < 1% in instructions (K2); M5 (K1); zeal-minor lane green; verifier remembered-set completeness, cells included; deeprec under forced minors scans only new frames; per-workload max minor pause within §6.11's formula with 7b's constants on the GBS, `deep-unwind` and `deep-descent` (≤ 2 ms expected on the GBS); the zeal-minor lane runs `samedepth1000` and `retained-continuations` with peak RSS within 10% of whole-heap mode (`samedepth1000` ≤ 200 MB, as at 4e), and `bytes-reclaimed` counts the released young runs; K8 on deeprec and `deep-unwind` | 7–10 | minor pauses proportional to survivors |
| 8 | **Opportunistic evacuation** | move-all torture mode; BFG hash extension; ¾-live candidates; fragmentation trigger | move-all lane green for 4 consecutive weeks; `frag-mix` fragmentation ≤ 5% and occupied/marked ≤ 1.2; eqtable within 3% (K3) | 6–9 | fragmentation cured; moving proven; the K4 nursery becomes possible |
| 9 | **Threads readiness** | `GreenThread` split; the `set_limit` switch; `T_THREAD`, `T_MUTEX`, `T_CONDVAR` and time layouts in `declare_layouts!`; the per-heap thread table; the scheduler's root provider reporting every started, non-terminated thread (decision 23); `FinalKind::Thread` with `FinalRegistry::queue` at termination and teardown draining; the tree-walker's `StepResult` threads; a minimal internal scheduler (`%thread-*` primitives) for the tests, so SRFI 18 itself is library work on top; safe regions with the entry rule, blocking primitives as `Transfer` with their buffer restructuring (the reader lexes before it builds); two-mutator lane; **deep-bound `parameterize`** (its own PR, with new matrix rows scored against Gambit, chibi and Gauche; chibi is not the oracle for parameter inheritance); current ports in the dynamic environment; the `threaded` obligations of §12 filed as issues | two-mutator lane green; on both backends, a terminated thread that is never joined gives its stack back at the next poll (resident size returns), a thread never started holds no stack, teardown unmaps every remaining thread stack, and `blocked-threads` is scored by hand against Gambit and chibi with decision 23 recorded; ≤ 1% cost in instructions; `parameterize-loop` and `display-loop` within 1% | 7–10 | SRFI 18 (M:1) can start |
| P | **Parallel stop-the-world marking** (triggered by measurement and budgeted; independent of decision 7) | per-heap GC worker threads above a live-size threshold; CAS marking on the metadata byte during GC; work-stealing mark segments; the sharded parallel ephemeron fixpoint; sequential or address-ordered evacuation destinations (§11); `PATINA_GC_WORKERS`; the workers=4 lane. `patina-gc` | **Starts only if** `large-live`'s max major at 1 GiB live exceeds 100 ms after stage 5. **Accept:** at 1 GiB live, mark time ≥ 2.5× faster with 4 workers; the workers=4 lane byte-identical with the single-threaded lanes; no worker started below the threshold (`many-heaps`); GBS geomean ±1% | 4–6 | majors on large live heaps scale with cores, with no threading decision |
| — | *Optional:* copying nursery (K4); C′ (owner decision 18, K15); boot-image static space; incremental marking (only on a latency goal); evaluator-owned `StepRoots` for nested tree-walker trampolines; a native call/ret design (only if K11 fires); isolates (decision 7's alternative, about 3–5 weeks, after 5e) | | | | |

Stages 0–9 and P total **about 88–122 engineer-weeks, roughly 20–28 months for one engineer**; the optional row is
not included. Stage P is included because §6.11's estimate (120–320 ms for a single-threaded major at 1 GiB live)
says its 100 ms trigger will fire after stage 5; if `large-live` measures under 100 ms, the total falls by 4–6 weeks.
Most of the growth over the first draft's estimate comes from the census and harness work, the bottom-up codemod with
the VM and tree-walker conversion, the deferred-release and code-liveness split between 4d and 4e, the nested-loop
audit, the debug-poison sweep, Miri on core, the `RefCell` removal from four crates, inline flonum operations, stage
3's interrupt handle and embedding calls, and then stage 9's SRFI 18 objects on both backends (+2–3), stage P (+4–6),
the minor budget's measurements and young LOS lists (+0–1) and collect-and-retry for user-sized requests (+0–1).

Parallel work is limited by shared files: only 4a and 4c can run beside 4d–4e (4b, 4f and 4g edit the same dispatch
arms in `vm_state.rs` and the same weak seam in `gc.rs` as 4d and 4e, so they are serialized), stage 6 can run
beside stage 5, and stage P beside stages 7–9. A second engineer therefore shortens the calendar by about 14–21 weeks;
the rest is serial. The
collector proper is about 4–6 kLOC through stage 7, plus 2–3 kLOC for stage 8. Most of the effort is the common core
every candidate design pays for.

**The first three PRs.**

1. **PR-1: "A GC mode for the benchmark runner, the GC benchmark set, a pause/MMU log."** Issue: "No GC or pause
   benchmark exists; the repository suite collects at most 4 times."
   - Adds `crates/patina-tests/bench_programs/gc/` with the Patina-authored probes, the barrier programs, the fixnum
     twins and the vendored input and shim files of §14. No Larceny files.
   - Extends `scripts/benchmarks.py` and `workloads.json` with a `gc` mode: ABA ordering over at least 10 rounds;
     instructions, cycles, RSS, footprint and page reclaims (`/usr/bin/time -l` on macOS, `perf stat` and
     `/usr/bin/time -v` on Linux); bootstrap confidence intervals; MMU from the log. It runs the Larceny programs
     from the reference checkout. Its own tests join `scripts/tests`.
   - Adds phase timings to `GcStats`, the `PATINA_GC_LOG` CSV writer (including the non-mutator intervals outside
     the pause), pause fields in `(gc-stats)`, and K16's two high-water counters (bytes allocated between a posted
     collection and the safe point that runs it; bytes allocated under a `GcDeferGuard`), in
     `crates/patina-core/src/heap/gc.rs`.
   - Vendors the measurement probes (`madv.c`, `cycle.c`, `decommit.c`, `mapjit.c`, the `mmap` placement probe, the
     continuation toy) under `scripts/gc_probes/`, and adds the `large-live` probe to the GBS.
   - Fixes the stale `gc.rs:25` comment.
   - *Accept:* no behaviour change; lanes byte-identical; logging costs ≤ 0.5% in instructions when off; the
     timing, RSS and pause baselines are posted on the tracking issue. The census is a separate stage-0 PR.
2. **PR-2: "Count bytes, not objects, in the collection trigger."** Issue: "500 × `(make-vector 100000)` never
   collects; gcold reaches 628 MB RSS with 12 MB live."
   - `Heap::note_alloc(bytes)`. Every `alloc_*` charges its slot plus payload capacity: vector and string buffers,
     bignums, continuation snapshot registers and frames.
   - The threshold becomes `max(8 MiB, 2 × live_bytes)`, from marked bytes plus the payload bytes `trace_weak_ids`
     proves live.
   - Stress keeps counting allocations until 5e.
   - `(gc-stats)` gains `live-bytes`, `bytes-allocated`, `bytes-reclaimed` and `committed-bytes`.
   - **Rewrites the reclamation proofs** (§14) in the same PR: the 8 MiB floor is above today's 200 K-cons churn.
   - *Accept:* the stage 1 numbers; GBS geomean ±1%; all lanes.
3. **PR-3: "Slot-based root visitor, open root registration, `CallFrame.closure` as a value."** Issue:
   "`GcVisitor::visit` takes values by copy (`crates/patina-core/src/heap/gc.rs:485`), roots are a closed array
   (`crates/patina-tree-walker/src/eval/cps_eval/mod.rs:126-130`), and `CallFrame.closure` is a bare index."
   - `SlotVisitor` with `slot`, `slots`, `pinned`, `host` and `ephemeron`; slots are `&HeapSlot`/`&Cell<Word>`, so
     `GcRoots::trace_roots(&self)` (`gc.rs:178`) **keeps `&self`**. The tree-walker's `Evaluator` is reached only
     through a shared reference (`cps_eval/mod.rs:94-97,126-130` [S]), and it reports `pinned` anyway.
   - Every `GcRoots` implementation is ported: `VmState`, `LibraryRegistry`, `StepTracer`, and the tree-walker's
     `Evaluator`, `EscapeRoots` and `StepRoots`.
   - `RootSet::register` replaces the closed array, `visit_object_index` is deleted, and `PENDING_ESCAPE` becomes an
     evaluator field.
   - *Accept:* no behaviour change; GC microbenchmarks ±1%; all lanes; a unit test proves a registered root is traced
     and that a test visitor can rewrite and restore a slot.

Next come PR-4 (`Step::Collect` and `EMFILE`), PR-5 (deleting the unread provenance stores) and PR-6 (handles and
teardown).

---

## Appendix E. Inventories and transitional detail that leave the design

Each block names the issue (or file) that carries it once stage 0 has filed the issues. Part I keeps only the rule
each block serves.

### E.1 The fate of every off-heap holder (→ the S0 tracking issue)

The inventory of everything outside the heap that holds a value today, with its fate and stage. §8.6 keeps the one
fate that is a design (variant R's binding records).

| Holder today | Fate | Stage |
|---|---|---|
| VM register `Vec<TV>` and `frames: Vec<CallFrame>` | per-thread reserved stack with interleaved, initialized frames | 4d |
| `CallFrame.closure: Option<HeapIndex>` (special `visit_object_index` path) | a traced value; `visit_object_index` deleted | 2 |
| `CallFrame.code: Rc<CodeObject>` | 4d: raw `*const CodeObject` in the frame plus a per-thread `Vec<Rc<CodeObject>>` side vector cloned by capture; 4e: a descriptor reference, side vector deleted | 4d / 4e |
| `CodeObject.constants: Vec<TV>` in an `Rc` | inline descriptor constants | 4e |
| environments, globals, `FORWARDED`/`Owner` links, `GlobalCacheEntry` and the `env_id` cache | variant R (§8.6): each name in a namespace maps to an immortal binding record over an immortal cell; code reaches globals through per-`CodeObject` link tables of records | 4b |
| `VmClosure.globals: Rc<Environment>` (one link of the teardown cycle) | deleted: code reaches its globals through its link table, and `CodeObject` carries its namespace id (for top-level `define` and diagnostics), which moves into the descriptor at 4e | 4b |
| `Library.exports: HashMap<String, TV>` | `name → CellRef`; the stale copies disappear | 4b |
| `CompiledMacro` literals, `CompiledMacro.heap`, untraced `foreign_expansions` | a literal vector in the macro's host handle; the body becomes a host payload. The `heap` field is deleted in the stage-3 codemod, with `Environment.heap` | 3 / 4g |
| `syntax_sources` (316 MiB with entries at peak [P]), `SourceMap.locations`, child spans | stage 1 deletes the two unread stores, the throwaway per-expansion `SourceMap` and the pruning test that measured `locations` (`crates/patina-tests/tests/interpreter_api.rs:270-292` [S]). Stage 4c: identifiers carry a source id into a per-document location table; **a list form takes its location from its head identifier's entry, which also records the enclosing form's span**; a form whose head is not an identifier (`((lambda …) …)`) loses its location, which "lost, never misattributed" allows; `syntax_sources` is deleted | 1 / 4c |
| transient raw-bits sets (writer, parser, `quoted`, `OpenNodes`, memos, `PrimitiveCallMap.by_value`) | unchanged: still valid because no collection runs inside a `Cx` window, and the heap does not move until stage 8. syntax-case brings an epoch-checked `ExpansionContext` | — |
| `CoreExpr`/`CpsExpr` literals, `Step` state, macro literals held as raw words | the `pub unsafe` raw-word API under `NoGcScope` or a traced root; the `CoreExpr` literal pool (§8.3) when it lands | 3 |
| VM continuation side tables, `VmContinuationRef` | deleted: continuations are heap objects | 4e |
| `WindRecord.handlers: Rc<[H]>`, copied per `dynamic-wind` | a heap vector; persistent heap lists with C′ | 4e |
| `symbol_table`, `core_syntax_table` (re-marked every GC) | an interner over immortal symbols, live as ephemeron keys (§6.6); core syntax in cells | 5c |
| `Parameter{values: Rc<RefCell<Vec>>}` (shallow binding) | 4g: a transitional heap layout (descriptor, converter, values vector), which 5c keeps; stage 9: deep-bound parameterization as heap data | 4g / 9 |
| `%parameterize-swap!` calling parameter-like procedures from Rust, with a transactional undo (`crates/patina-primitives/src/primitives/parameters.rs:195-245` [S]) | stage 4a: the standard ports become parameter objects with validating converters, so the swap installs only into parameter objects and never calls back; any other parameter-like procedure is installed by a Scheme loop in `lib/scheme/base/parameters.scm` with a `guard`-based undo around a non-reentrant `%parameter-install!`. `parameters.scm:167-176`'s `(caught 0)` row gates it | 4a |
| record `Rc<RTD>` + `Rc<RefCell<Vec>>`, promise `Rc`, `MutableCell` `RefCell` | inline heap fields through the funnel | 4g / 5c |
| `Port(Rc<Port>)` and the `thread_local!` current ports | `PortTable` + port object, tables in a process-wide weak registry (R1); current ports in the dynamic environment | 4a / 9 |
| tree-walker `StepResult`, CPS graphs, `PENDING_ESCAPE` | pinned roots (by value) in non-moving tree-walker heaps; payloads as host ids; `PENDING_ESCAPE` becomes an evaluator field | 2 / 4f |
| values held by embedders | `Owned` handles | 2 |
| tracer snapshots, debugger hook storage, profiler samples, green-thread scheduler | registered `RootProvider`s (open registration); descriptor references only through `CodeStore::escape` | 2 |

### E.2 Rebinding cases of decision 3 (→ the rebinding suite file and its `DIVERGENCES.tsv` rows, filed at stage 0)

Shapes measured 2026-10-01 with chibi 0.12, Gauche 0.9.15 and Chez, using a
`(counter)` library that exports `count`, `bump!` and `get-count`; p1, p1b and p4 are decision 2's rows. chibi
cannot define a library in a script, so it arbitrates these rows by hand, as it does for
`stdlib/library-bindings.scm`; Chez is quoted in notes, not run as a lane oracle. The class of each row is set when
it is added. R7RS §5.2 makes redefining an imported binding, importing one identifier with different bindings, and
referring to an identifier before it is imported errors in a program or library, while a REPL "should permit" them
[S, R7RS §5.2], which suggests `latitude` for p8, p9, q1–q3 and d1 [I].

| Case | Shape | Under R (stage 4b; today's answer) | Under C | Oracle followed | `DIVERGENCES.tsv` rows |
|---|---|---|---|---|---|
| p7 | a reference compiled before a later `define` of the same name | works: a placeholder record, whose own cell the `define` fills | works: a placeholder cell, which the `define` reuses as the program's own | all three agree | none |
| p8 | a reference compiled before a mid-program `(import (counter))` that supplies the name | follows the import: `0` | stays unbound: an error | chibi, Gauche and Chez (all error) | R: chibi and Gauche; C: none |
| p9 | the program defines `count` and compiles `show`, then imports `(counter)` and calls `(bump!)` | the import re-points the record, so `show` sees the library's `count`: `(1 1)` | `show` keeps the program's binding, later code sees the import: `(mine 1)`, with chibi's warning "importing already defined binding" | chibi, warning included | R: chibi and Gauche; C: Gauche, which answers `(mine mine)` (the definition wins everywhere) |
| q1 | `call-with-values` and `dynamic-wind` sites run, then the program redefines both | the earlier sites use the new definitions: `mine mine` | the earlier sites keep the imports | chibi and Gauche | R: chibi and Gauche; C: none for the answers (Gauche's anomalous `dynamic-wind` result gets its own row if the suite row observes it) |
| q2, q3 | `(import (rename (only (scheme base) cdr) (cdr car)))` after `f` was compiled, with and without a prior call of `f` | `f` follows the re-import: `((2) (2))` | `f` keeps `car`: `(1 (2))` | chibi and Gauche | R: chibi and Gauche; C: none |
| d1 | `(begin (error "boom") (define list-copy 5))`, then `(list-copy '(1 2))` | the `define` never ran, so `list-copy` is still the import: `(1 2)` | the rebinding is committed when the unit compiles, so the call is an error | chibi, Gauche and Chez (all error) | R: chibi and Gauche; C: none |

### E.3 `Backend` and `SourceMap` migration, per stage (→ S2 and S3)

The public `Backend` trait returns bare `TaggedValue` and takes `&Rc<Environment>`
(`crates/patina-runtime/src/backend.rs:33-93` [S]), and CI compiles an external host that implements it
(`crates/patina-interpreter/tests/support/feature_consumer.rs`, `scripts/check_embedding_features.sh` [S]). It
migrates with #601's deprecation convention, one step per stage, with `check_embedding_features.sh` green at each:

| Stage | `Backend` / `Interpreter` | `SourceMap` |
|---|---|---|
| 1 | unchanged | the `locations` store is deleted; its accessors (`get`, `len`, `iter_locations`; only tests read them today) are deprecated, answer empty, and go at 5e; error formatting unchanged |
| 2 | `Owned`-returning `eval_*` added; bare-value forms deprecated | — |
| 3 | `Backend::eval` takes and returns raw words through the `pub unsafe` API; `with`, `call`, `register_primitive`, `register_resumable` and `interrupt_handle` added | — |
| 4b | `&Rc<Environment>` replaced by a namespace handle; `global_env` deprecated | — |
| 4c | — | locations come from identifier source ids; `format_interpreter_error` unchanged |
| 5e | deprecated forms removed | — |

The public `prune_freed_locations` (re-exported by `patina-interpreter`, `crates/patina-interpreter/src/lib.rs:52`
[S]) follows the `locations` accessors: a deprecated no-op from stage 1, removed at 5e. `display_tagged(impl AsValue)`
keeps most of the ~200 test call sites compiling unchanged.

### E.4 Pause estimates (→ S5 and S7)

[I] estimates, calibrated with [P] rates where noted; §6.11 holds the budgets they are checked against.

| Pause | Expected |
|---|---|
| Minor | ≤ 1.5% survival on 10 of 20 workloads → tens of µs; nboyer, mperm, queue3 and deeprec run mostly in bypass at a 4 MiB budget (§6.2). Inside the formula, the large terms are a non-allocating descent of 1 M frames (≈13 ms), a full store buffer at d = 10 ns (≈1.3 ms) and 4 MiB of survivors (≈1.4 ms) |
| Major | b is 2.8–3.0 ns per live pair on a contiguous list today, but ≈4.7 ns per live vector on queue3 and 8.1 ns per live closure [P]; a is ≈13 ns per frame including its registers (deeprec: 12.3 ms mean mark over 960 K frames and 11.5 M registers [P]). So about 5–15 ms at the measured maximum of 50 MB live with shallow stacks, plus about 13 ms per million suspended frames; post-load (10–15 MB live) about 2–5 ms. Stage 5's post-load gate compares with the stage-4 baseline, not with today's 178 ms, because stages 1, 2 and 4c remove most of that number first (K9) |
| Major, 1 GiB live | 2³⁰ B / 26.9 B ≈ 40 M objects; at 3–8 ns each, about 120–320 ms single-threaded, in whole-heap and generational modes alike, since majors stay full stop-the-world collections. Inside b's budget (0.35 ms per MiB ≈ 358 ms at 1 GiB) but long; stage P divides the b term by the worker count, and `large-live` measures it |
| Sweep in the pause | none: the catch-up sweep and block classification run after release (§6.1); today sweep is most of queue3's 41 ms |
| Decommit | none in the pause: after release, rate-limited (§4) |

### E.5 Transitional arrangements (→ S1, S3, S4d, S4e, S4g and S5)

- **Continuation bytes before 4e (S1).** Stage 1's byte trigger charges snapshot bytes at capture, and
  `trace_weak_ids` reports the bytes of the payloads it proves live into L, so retained continuations raise the target
  instead of forcing a full collection every 8 MiB of capture.
- **The capability in the arena era (S3).** In stages 3–5d the capability is a unique, non-`Clone` `GcDriver` token
  owned by the outermost driver; `Cx` windows borrow it, so no `Cx` can coexist with `driver.collect()`. The heap data
  still sits behind the `RefCell`, and `Cx` borrows it **per operation**, as today's code does: the codemod changes
  receivers, not borrow scopes. Interior references are `Ref` guards tied to a `&Cx` borrow, so a re-entrant call (a
  residual `apply_proc` fallback, the parser reading a port) never holds a borrow across the re-entry, where a borrow
  held for a whole primitive call would fail at run time with `BorrowMutError`. At 5e the token and the heap become
  one `&mut Heap`.
- **Allocation receivers (S3, S5).** In the arena era `get_bigint(&self) -> Option<&BigInt>`
  (`crates/patina-core/src/heap/mod.rs:2769` [S]) and interior slices point into relocating `Vec` arenas, and arena
  allocation is `Vec::push`; `&mut Cx` is what makes the borrow checker flag the 78 borrow-then-allocate functions.
  Stage 5e removes the `RefCell` from core (36 borrow sites), the VM (64), the tree-walker (35) and `patina-compat`
  (32 `SharedHeap` mentions) [S, `rg` counts at review].
- **Codemod order (S3).** Bottom-up, against the crate graph: core accessors first; then macros and frontend, where
  `Parser`, `Expander` and `Environment` stop owning a `SharedHeap` and take the context as a parameter
  (`crates/patina-frontend/src/parser/mod.rs:80` [S]); then primitives, which depend on the frontend
  (`crates/patina-primitives/Cargo.toml:9` [S]; `read` builds a `Parser` over a heap clone,
  `primitives/io/read.rs:22-30` [S]); then runtime, VM and tree-walker. Raw constructors (`TaggedValue::from_raw`,
  `pair(index)`, `object(index)`) become `unsafe` at stage 5b.
- **Code liveness before 4e (S4d).** Today a frame or a captured continuation keeps its unit loaded through
  `Rc::strong_count(code) > 1` (`crates/patina-vm/src/runtime/vm_state.rs:1003-1008` [S]). Stage 4d keeps that: the
  frame's `code` word caches a raw `*const CodeObject` for dispatch, and a per-thread side vector of `Rc<CodeObject>`
  (pushed and popped with frames, cloned by capture) holds the strong reference.
  `a_captured_continuation_keeps_its_form_s_code` (`finished_forms_release_code.rs:201` [S]) gates 4d. From 4e the
  word is a descriptor reference and the side vector goes; only then do the four `Rc` operations per call/return pair
  disappear.
- **Map memory (S4d).** Today's per-pc `Vec<Vec<u64>>` costs about 40 B per pc, 65% of the instruction stream [P,
  library-load census]; the dense index is expected to be at least 90% smaller [I], which 4d measures on libload
  before any memory claim is made.
- **Descriptors in the arena era (S4e, S5).** From 4e until 5c a descriptor is an `objects`-arena slot with the same
  fields; it moves into the descriptor space (primitive descriptors into the immortal space) with the other
  fixed-size kinds at 5c. Every code reference is a value from 4e onwards.
- **`Drop` census (S4g, S5).** 4g's gate is per kind: no `Rc` payload left in records, promises, cells, parameters,
  identifiers, ports and continuations. The ≤ 1% census target is met at stage 5's exit, when closures, vectors,
  strings, bytevectors and bignums have left the arenas.
- **Transitional encoding (S5).** Stages 5b–5e keep today's tag map for objects already in blocks: `011` pair, `100`
  vector, `101` string, `110` procedure (a tag never minted today), `111` object, with addresses in place of indices.
  An arena index (below 2³⁵) is told from a block address by the placement rule below: transitional decoding is
  `raw ≥ 2³⁶ ⇒ block address`. Stage 5f switches to the final map in one isolated PR with a revert criterion (K6).
- **Placement rule (S5).** Arena references are `index << 3 | tag`, always below 2³⁵, and an ordinary `mmap` does not
  keep block addresses above that: on the development machine 2 MiB and 64 MiB anonymous maps returned
  `0x10854c000` and `0x10874c000`, both below 2³⁵, while a hint at 2⁴⁰ was honoured exactly [P, probe
  `scripts/gc_probes/placement.c`]. So the heap requests its reservation at a hint of 2⁴⁰ + k·reserve, verifies that
  the result is at least 2³⁶, retries with the next hint, and refuses to construct after 16 failures. The rule
  disappears with the arenas at 5e.

### E.6 Layouts against today's representation (→ S5)

| Kind | Tag | Words | Bytes | Today |
|---|---|---|---|---|
| pair | `0100` | car, cdr | 16 | 16 (arena) |
| procedure: VM closure | `0101` | descriptor ref, fv₀…fvₙ₋₁ | 8+8n | 72 + `Vec` + `Rc<Environment>` |
| procedure: primitive | `0101` | descriptor ref (one immortal descriptor per primitive), pad | 16 | 72 + `Rc` |
| procedure: parameter | `0101` | descriptor ref (PARAM), converter, value vector (shallow binding, transitional); from stage 9: descriptor ref, converter, global-value cell | 32 | 72 + `Rc<RefCell<Vec>>` |
| procedure: continuation | `0101` | descriptor ref (CONT_INVOKE), `CONT` object | 16 | id into weak side table |
| procedure: tree-walker lambda | `0101` | descriptor ref (TW_LAMBDA), host id (fixnum) | 16 | 72 + `Rc<CpsLambda>` |
| vector | `0110` | header(len), e₀… | 8+8n | 24 + `malloc` |
| record | `0111` | record-type reference, f₀…fₙ₋₁ | 8+8n | 72 + `Rc<RTD>` + `Rc<RefCell<Vec>>` |
| string | `1100` | header(len), UTF-32 units | 8+4n, pf | 24 + `malloc` |
| symbol | `1101` | header(len), hash (fixnum), UTF-8 bytes | 16+len, pf, immortal | 72 + `String` key |
| bytevector | `1110` | header(len), bytes | 8+n, pf | 72 + `Vec<u8>` |
| flonum box | `1111` | header, f64 | 16, pf | 72 |
| bignum | `1111` | header(limbs; sign by type code), u64 limbs | 8+8n, pf | 72 + `num-bigint` |
| ratnum / complex | `1111` | header, num, den / re, im | 32 | 72 / 3 × 72 |
| cell (box, `MutableCell`) | `1111` | header, value | 16 | 72 |
| record type | `1111` | header, name, parent, uid, field names, nfields (fixnum), flags (fixnum), pointer mask | 64; descriptor space | a fresh wrapper per `%record-type-of` |
| identifier | `1111` | header(scope-set id in the length bits), symbol, source id (fixnum) | 32 | 72 + `Rc<str>` + 40 B `SmallVec` |
| ephemeron | `1111` | header, key, value (reported through `SlotVisitor::ephemeron`, never as two strong slots), GC link (collector-private: fixnum 0 outside a collection, never traced) | 32 | 72 |
| promise | `1111` | header, box → `[hdr PBOX][done (fixnum)][value-or-thunk]` (SRFI 45 sharing) | 16 + 32 | 72 + `Rc` |
| values | `1111` | header(n), v₀… | 8+8n | 72 + `Vec` |
| condition | `1111` | header(kind), message, irritants, extra | 32 | 72 |
| port | `1111` | header, port id (fixnum) | 16 | 72 + `Rc<Port>`; a new wrapper per call |
| host handle | `1111` | header(kind), host id (fixnum), optional literal vector | 16–32 | 19 `Drop` variants |
| environment specifier | `1111` | header, namespace id, flags | 32 | `Rc` |
| continuation (design A) | `1111` | header(words), meta (fixnums), dynamic-state references, frame words | variable; LOS above 8 KiB | 5 cloned `Vec`s in weak `Rc` tables |
| code descriptor | `1111` (`T_CODE`) | see §3 | 48 + 8·nconst; descriptor space | `CodeObject` 160 B + `Rc` |
| global cell (never a value) | — | header, value, name symbol, namespace id + flags (fixnum; includes `WATCHED`) | 32; immortal space | environment slot value |
| binding record (variant R only; never a value) | — | header, cell reference, name symbol, flags (fixnum; includes `WATCHED`) | 32; immortal space | environment slot + `FORWARDED` + `Owner` link (§8.6) |

### E.7 The threading model (→ ST, the threading-model issue)

**Verdict.** Build one mutator now and design the GC's interfaces for N mutators sharing one heap: SRFI 18 ships as
M:1 green threads on one OS thread, VM first (8–12 weeks after the GC work, then the tree-walker, +2–3, then
non-blocking I/O, +2–4); the design target is N carriers over one shared heap with stop-the-world collection, the
shape of Gambit SMP, OCaml 5, Racket's parallel threads and Loom, reached through M:N and not built now; isolates are
optional and GC-neutral. The rule for the GC: every per-thread concept gets an explicit owner object, and every
protocol is written for all mutators, compiling to plain code while there is one.

**Primary sources.** SRFI 18 (https://srfi.schemers.org/srfi-18/srfi-18.html: parallelism allowed, not required;
another thread's continuation is well defined; a new thread inherits the dynamic environment; neither a switch nor
`thread-terminate!` runs `dynamic-wind` thunks); SRFI 226 (https://srfi.schemers.org/srfi-226/srfi-226.html,
parameter inheritance); Go's per-P allocation cache (https://go.dev/src/runtime/mcache.go); Loom's carrier threads
(https://openjdk.org/jeps/444); OCaml 5 domains (https://arxiv.org/abs/2004.11663); Chez thread contexts and
`Sdeactivate_thread` (Chez `c/thread.c`); Gambit's scheduler and thread groups (Gambit `lib/_thread.scm`,
`lib/_thread#.scm`); chibi's SRFI 18 scheduler (`lib/srfi/18/threads.c`); Racket's parallel threads
(https://blog.racket-lang.org/2025/11/parallel-threads.html); PEP 703 (https://peps.python.org/pep-0703/) for the
single-thread cost of retrofitting a shared heap onto a reference-counted runtime.

**Cost of real shared-heap threads (decision 7).** Per item, in focused engineer-weeks [I, estimates by analogy]:

| Item | Weeks | Delivered by stages 1–9? |
|---|---|---|
| heap and arena redesign with per-carrier allocation buffers | 6–10 | yes (3, 5) |
| `Rc`→`Arc` and `RefCell`→locks or atomics (about 600 `Rc<` sites, 115 `RefCell<` types) | 8–14 | only the heap's own (4g, 5e); the rest remains |
| heap call-site migration (787 sites) | 4–6 | yes (the stage-3 codemod) |
| precise rooting to remove deferral | 4–8 | yes (2, 3, 4e) |
| handshakes, safe regions, per-mutator GC state | 3–5 | per-mutator state and safe regions (3, 9); handshakes remain |
| shared code store and caches | 2–4 | yes (4e) |
| concurrency testing (loom, TSan) and performance recovery | 8–16 | no |
| **total** | 35–63, which the cost study rounds with risk to **9–18 engineer-months** | remaining after stage 9, with §12's `threaded` obligations (2–4): about 19–36 weeks, **4–9 engineer-months** |

Isolates: spawn, channels, the message codec, join and `exit` semantics, and moving the process-global leftovers,
about 3–5 weeks; a shared compiled-library cache across isolates, 4–8 more, optional.

**Open questions for the owner:** is shared-memory parallelism under SRFI 18 a goal (decision 7)? May invoking another
thread's continuation be an error (assumed supported, as SRFI 18 and Gambit require)? Is it acceptable that blocking
I/O stalls all threads in the first SRFI 18 release?

---

## Appendix H. Mechanism detail for the stage issues

Part I states each mechanism's rule; this appendix keeps the step-by-step detail the stage issues need. Each block names
its issue.

### H.1 JIT sequences (→ S6, the spike, which confirms or replaces each)

**What the JIT gets** (aarch64, tag checks included):

| Operation | Sequence | Instructions |
|---|---|---|
| `(car p)` | `and t,p,#15; cmp t,#4; b.ne slow; ldur r,[p,#-4]` | 4 |
| `if` / `(null? x)` | `cmp x,#1` / `cmp x,#0x11`, then a branch | 2 |
| fixnum `+` | `orr t,a,b; tst t,#7; b.ne slow; adds r,a,b; b.vs ovf` | 5 |
| `(vector-ref v i)` | tag test (3); `ldur h,[v,#-6]`; `tst i,#7; b.ne`; `cmp i,h,lsr #13; b.hs oob`; `add a,v,i; ldur r,[a,#2]` | 10 |
| closure free variable / cell value | `ldur r,[c,#3+8i]` / `ldur r,[c,#-7]` | 1 |
| record field and type test | tag test (3); `ldur t,[r,#-7]; cmp t,x_rtd`, where `x_rtd` is an immediate only under §8.2's embedding rule, else one load from the descriptor's constants | 5–6 |
| procedure call | `and t,f,#15; cmp t,#5; b.ne slow; ldur d,[f,#-5]; ldur e,[d,#-7]; br/blr e` | 3 + 2 dependent loads + branch |

The vector bounds check works because vector type codes are below 32: header bits 13–15 are zero, so `h >> 13` is
`len << 3`, the tagged length, and one unsigned compare also rejects negative indices.

Flonum tests and conversions:

```
; is-flonum:      and t, w, #6 ; cmp t, #2 ; b.ne not_flo                  (3)
; decode → d0:    ror x0, w, #4 ; add x0, x0, xK ; fmov d0, x0              (3)
; encode d2 → w:  fmov x2, d2 ; sub x2, x2, xK ; ror x2, x2, #60
;                 and t, x2, #6 ; cmp t, #2 ; b.ne box_cold                 (6)
```

**JIT fast path** (aarch64; `x21` is the pinned `*Mutator`; `ap` at offset 0 and `limit` at offset 8, MMTk's
`BumpPointer` offsets):

```
; at fragment entry, and after any call not marked NoAlloc:
ldp   x22, x23, [x21, #0]          ; ap, limit
; one basic block, two conses and a 2-free-variable closure, grouped: 16 + 16 + 32 = 64 B
add   x9, x22, #64
cmp   x9, x23
b.hi  refill_cold                   ; str x22,[x21]; bl rt_refill(x21, 64) → (x0 = ap, x1 = limit); retry
stp   xa, xb, [x22]                 ; cons 1: initializing stores, no barrier
add   xp1, x22, #4                  ; tag 0100 — always add: ap is only 16-byte aligned, so orr is wrong
stp   xc, xp1, [x22, #16]           ; cons 2, whose cdr is cons 1
add   xp2, x22, #(16 + 4)
stp   x_dref, xfv0, [x22, #32]      ; closure: word 0 = descriptor reference (non-moving)
stp   xfv1, xzr, [x22, #48]         ; fv1, then the padding word (fixnum 0): every word is written
add   xclo, x22, #(32 + 5)
mov   x22, x9
; before any call not marked NoAlloc (Leaf or Transfer): str x22, [x21, #0]
```

- Tags are applied with `add`: `x22 | 20` equals `x22 + 20` only when bit 4 of `ap` is clear, and most multi-bit tag
  immediates are not encodable as aarch64 logical immediates.
- One check covers a block's summed allocation (OCaml's Comballoc). `rt_refill` is a `Leaf` helper (§8.2) that
  returns the new `(ap, limit)` in registers.
- **`NoAlloc` helpers** are the only calls across which `ap`/`limit` stay in SSA; every other call has them written
  back before and reloaded after. A call-graph test enforces the attribute, and debug builds assert that
  `Mutator.ap` is unchanged across a `NoAlloc` call.
- A conformance test checks that the emitter writes every word of every inline-allocated kind against the
  `declare_layouts!` size. A later optimizing tier may reserve a region's straight-line allocation at its poll.

**The slow path** stays out of line but call-free, for register pressure: no tier declares stack maps (§8.2), so a
call costs only what it clobbers. Cranelift's `PreserveAll` convention (`call_conv.rs:49-62` [S]) removes even that,
so the stage-6 spike measures a `PreserveAll` call to a generated append stub against this inline sequence and keeps
the cheaper; both have the same semantics.

```
log_cold:
      and   w4, w4, #0xBF ; strb w4, [x20, x3]        ; disarm (`ldclrb` under the `threaded` feature)
      ldr   x5, [x21, #0x18]                          ; remset_cur
      and   x6, x2, #~15 ; str x6, [x5], #8           ; append the granule address
      str   x5, [x21, #0x18]
      ldr   x7, [x21, #0x20] ; cmp x5, x7 ; b.lo 1b   ; below the soft limit → done
      ldr   w8, [x21, #0x3C] ; orr w8, w8, #EV_GC_MINOR ; str w8, [x21, #0x3C]   ; owner post (pending)
      str   xzr, [x21, #0x30]                         ; reg_limit = 0: the next call or back-edge polls
      b     1b
```

### H.2 `MarkRegion` collection, step by step (→ S5, S7 and S8)

**6.1 Major collection.**

1. Retire every mutator's allocation buffer (`ap = limit = 0`).
2. **Flip** the mark epoch (1 → 2 → 3 → 1). Three epochs plus "free" mean no clearing pass.
3. **Drop every mutator's store buffer, range entries and remember-whole list.** In generational mode the major then
   re-derives every `LOG` bit: marking arms every pointerful object it marks (step 5), and epilogue step 9 arms every
   pointerful object of the root regions, which are never marked (the global cells). Without that second step a cell
   disarmed by a store logged before the major would stay disarmed, a later `(set! g (cons 1 2))` would go unlogged,
   and the next minor would free the pair.
4. **Trace roots** (§8): pinning roots first; every rooted green thread's register stack through its safepoint maps
   (all frames, ignoring the watermark) and its dynamic-state slots; root scopes and handles; the cells.
5. **Mark.** When the start byte's `STATE` is not the current epoch: write `STATE = epoch`, keeping
   `KEYHINT | HASHED | HASH_MOVED`; set `END` on the last granule; in generational mode set `LOG` on every granule of
   a pointerful object (a `u64` store covers 8 granules); add the granule count to `block.live_granules`, first
   zeroing it and stamping `counted_epoch` if the block was last counted in an earlier major; push the fields. Size
   comes from the tag (pair: 16), from word 0 (procedure → its descriptor's `nfree`; record → its type's `nfields`;
   both non-moving) or from the header, plus 16 if `HASH_MOVED`.
6. The weak fixpoint and the epilogue (§6.6, §6.10).
7. **Catch-up sweep, after the pause.** A block whose `swept_epoch` lags by two majors must have its metadata swept
   before the next flip, or a stale epoch could alias the epoch after next. The epilogue queues such blocks; the
   sweep (word-parallel, metadata only) runs in slices from the poll slow path once the world is released, logged as
   a non-mutator interval. A major that finds part of the queue unswept finishes it before its flip.
8. **Block classification, after the pause.** The same slices walk the block table: a block marking did not touch
   (stale `counted_epoch`) holds nothing live. They classify blocks as empty, recyclable or full, advance
   `empty_majors`, queue decommit candidates (§4) and release dead LOS runs, young and old. The allocator classifies a
   block it reaches first (§5 step 4): it clears an empty block's metadata, marks it `SOS_ACTIVE` and stamps
   `counted_epoch`, and the slices skip blocks stamped this cycle, so a block in use is never reclassified as empty.

**6.2 Minor collection (sticky marks; generational heaps only).**

"Old" means the start byte holds the current epoch; "young" means `STATE` 0. Minors do not flip the epoch.

1. Retire every mutator's allocation buffer.
2. **Roots:**
   - for each green thread with `ran_since_gc`, the frames **above its watermark** (§8.1). Any write into a
     non-running thread's frames (`thread-start!`'s initial frame, a value or exception the scheduler delivers) goes
     through `return_into`, which sets that thread's `ran_since_gc`;
   - **every** rooted green thread's dynamic-state slots (parameterization, handlers, winds, prompts, current ports):
     a few words per thread, scanned whether or not the thread ran;
   - root scopes and handles; the remember-whole lists and range entries; young host-payload registrations; machine
     roots.
3. **Store-buffer entries** are granule addresses. For each, re-read both words as values (invariant W), mark young
   referents, then re-arm the granule. Range entries are scanned whole, then re-armed.
4. Mark young objects only; never trace through old ones. **Marking is promotion**: an object becomes old on its
   first survival, with `STATE = epoch` and its granules armed. Promotion cannot fail, because nothing is copied.
5. Weak processing over young entries only (§6.6); then the young lists move to old.
6. **Young LOS runs.** A marked run moves to the LOS's old list. An unmarked one is dead: it is queued for release
   into the recycle cache after the pause, as at majors, and its bytes are counted in `bytes-reclaimed`. MMTk's
   treadmill keeps the same nursery list. Without it every dead young object over 8 KiB (a deep continuation capture,
   about 88 KB at depth 1000; a large vector; a string-port buffer) would wait for a major, up to 256 minors away.
7. Lazy sweep re-sweeps only the blocks allocated into since the last GC (`YOUNG_ALLOC`).
8. Reset each thread's watermark (§8.1).

**Where sticky loses**, each watched by a kill criterion: dead young objects cost a metadata sweep where a copying
nursery pays nothing (K4); locality is worse in recycled holes (K4); survivors stay where they are until evacuation
(K3). The pre-planned fix is a `NurserySpace` (§11): a copying nursery that promotes **in place** when to-space is
short or survival is high, with the same barrier and ABI.

**Adaptive bypass, with hysteresis.** Enter when the bytes surviving minors exceed 30% of the budget for 3
consecutive minors (the heap then runs majors only); stay for at least 2 majors; leave only when a major shows that
under 15% of the bytes allocated since the previous major survived. A 50% exit rule would oscillate on nboyer, whose
dead-old share is 76.5% while its byte survival is 32–37% [P]. The barrier stays armed in bypass, and a store-buffer
overflow simply forces the next major. M5 (§14) runs with bypass on and off.

**Forced-major backstops.** A major runs after 256 consecutive minors, or once nursery allocation since the last
major exceeds max(1 GiB, 64·L). This bounds how long old ports wait for finalization, code for release and dead-key
ephemerons for breaking; Larceny's `ephemeron` suite needs a major within about 100 M pair allocations (1.6 GB at a
4 MiB budget is 400 minors).

**6.3 Marking order and a bounded mark stack.**

- **Pairs:** push the cdr and continue with the car, so the stack is bounded by car-nesting depth rather than list
  length (today's car-then-cdr order cost +55 MB on a 2 M-element list of 2-vectors [P]).
- **Large objects:** vectors, records, continuations and descriptors push one `(object, next_index)` range entry and
  are processed 256 words per pop (Racket BC's segmented mark stack).
- **The stack** is a chain of 4 KiB segments from a recycled pool. Past a cap of max(1 MiB, heap/32) it stops pushing
  and sets the `RESCAN` flag on the block of each unscanned object; when the stack drains, the flagged blocks (kept on
  a list) are rescanned by walking start bytes, their flags cleared, and the process repeats. No heap parsability is
  needed.

**6.5 Evacuation and pinning (stage 8).**

- **Trigger.** Evacuate at a major when the previous cycle's fragmentation exceeds 10%, until it falls below 5%
  (Whippet `mmc.c:656-715`). **Fragmentation** is free granules not in usable holes (holes ≥ 256 B) over committed
  SOS granules, measured after marking and before decommit. It is also K3's metric; committed/live is not, because
  pacing and the free reserve hold it above 2 by construction.
- **Candidates:** SOS blocks under ¾ live at the previous major (Chez/Racket CS's `use_marks` rule) with
  `pin_count == 0` and no pin this cycle, lowest occupancy first. **Targets:** the evacuation reserve.
- **Mechanism.** Pinning roots are traced first and pin their blocks for the cycle. A reachable object in a candidate
  block is copied once: its start byte becomes `FORWARDED`, its word 0 the new address, the referencing slot is
  rewritten, and a `HASHED` object gets its extension granule (§6.9). When the reserve is exhausted, the rest are
  marked in place. Destination granules are armed by the marker; source granules are cleared by sweep.
- **Torture mode `move-all`** makes every unpinned SOS block a candidate with an unbounded reserve; from-space is
  filled with `GC_POISON` and, in debug builds, `mprotect`ed `PROT_NONE` until reused. It exercises the production
  copy path.
- **Never moved:** the LOS, the descriptor and immortal spaces, pinned blocks, and every tree-walker heap.

The stack watermark, as a return barrier (S7):

- After a minor, every frame below the executing frame T is clean (only old references; a suspended frame cannot be
  written), so T's caller is the watermark. The collector sets the `WM` flag in T's `link` word (interpreter) and
  swaps T's `ret` for `WM_TRAMPOLINE`, saving the real target in the thread **as `(code, pc)`**, never as a raw
  address (JIT); the trampoline re-derives the target from `desc.resume[pc]`, so freeing or replacing a body never
  leaves it pointing into freed code.
- Both words are read by every `Return` anyway, so **normal returns pay nothing**. Returning out of T takes the cold
  path, which **lowers the watermark by a stride of k frames** (k = 64–256, tuned in stage 7) or to the stack base:
  the flag moves to the frame k levels down, and the frames between are clean but rescanned at the next minor.
  Lowering one frame at a time would put deep allocating unwinds (deeprec's `build`; `map` in
  `lib/scheme/base/higher_order.scm:46-49` [S]) on the cold path for nearly every return. `GcStats` counts cold-path
  returns.
- A tail call that replaces T copies `link` and `ret`. **Every write into, or activation of, a suspended frame** goes
  through `return_into(frame)`, which lowers the watermark first and sets the owning thread's `ran_since_gc`: value
  delivery to a deeper frame, `ResumeWindJump`, raise stubs, abort landing, delimited append, scheduler deliveries,
  debugger writes. Reinstating a continuation sets the watermark to the stack base.
- A minor scans `[watermark_frame.child … top]` of each thread with `ran_since_gc`, which removes deeprec's per-minor
  scan of 11.5 M registers (about 12–15 ms [P]). The verifier asserts that no frame below the watermark holds a young
  reference. K8 can switch the watermark off; minors then scan whole stacks and stay correct.

### H.3 Heap parameters, the store buffer and decommit (→ S5 and S7)

**Sizes**, from the allocation census over the 20 GBS workloads [P]:

| Parameter | Value | Basis |
|---|---|---|
| granule | 16 B | 56% of objects are exactly 16 B: pairs, cells, flonum boxes and 1-free-variable closures fit in one |
| block | 32 KiB (2,048 granules = 2 macOS pages) | the Immix block; the 8 KiB LOS threshold bounds block-level waste at 25% |
| line | none: the metadata byte *is* the line table, at granule size | Whippet `collector-mmc.md` |
| medium threshold | 256 B | Whippet `NOFL_MEDIUM_OBJECT_THRESHOLD`; 99.2% of objects are ≤ 128 B |
| LOS threshold | over 8 KiB | 148 of 348 M objects |
| recyclable block | ≥ 25% free granules after sweep | Immix/nofl block promotion (`nofl-space.h:623-648`) |
| free reserve | ≥ max(8 blocks, 5% of committed) empty blocks after a major | Wingo's livelock fix |
| evacuation reserve (stage 8) | 2.5% of blocks, plus up to 50% of free blocks while compacting | Immix |
| initial commit | 64 blocks (2 MiB) | bootstrap fits |
| decommit unit | 4 MiB-aligned runs of 128 empty blocks | the run's 256 KiB of metadata is whole pages on 4 KiB and 16 KiB systems |

**Block table**, 16 B per block: `space: u8`; `state: u8` (FREE, SOS_ACTIVE, RECYCLABLE, FULL, LOS_HEAD, LOS_TAIL,
DESC, IMMORTAL); `flags: u8` (`EVAC_CANDIDATE`, `PINNED_THIS_CYCLE`, `HAS_PENDING_KEYS`, `YOUNG_ALLOC`, `RESCAN`);
`pin_count: u16`, sticky on saturation (recounted from the live `PinToken` table at the next major); `live_granules:
u16` with `counted_epoch: u8`, the major (count mod 256) in which it was counted, so no pause resets it (§6.1);
`swept_epoch: u8`; `empty_majors: u8`; `owner: u16`; `next: u32`.

**Store-buffer capacity.**
- The SSB is 256 MiB of reserved VA per mutator, lazily committed, with a guard page at the hard end; the Rust twin
  checks the hard end explicitly. Its **soft limit** is derived from the minor-pause budget (§6.11): about
  1.25 ms / d logged granules, 128 K at d = 10 ns, set by stage 7b from the measured d. Reaching it posts a minor.
- **Range entries.** Bulk stores (`store_range`, `fill_range`: `vector-fill!`, `vector-copy!`, string-port growth into
  an old vector) into an armed object log **one range entry** `(object, start, len)` and disarm its granules with one
  metadata `memset`; one entry per granule would need 35 M entries for a 560 MB `vector-fill!`, which `max_heap`
  allows. Range entries and remember-whole objects count their granules against the soft limit. One that would pass
  the remaining capacity is not logged, and the next collection is forced major.
- **Deferred collection.** If the poll slow path finds the buffer past its soft limit while collection is deferred
  (`NoGcScope`, or the `HEAP_EXHAUSTED` overdraft), it **discards** the buffer, resets `remset_cur` and forces the next
  collection to be a major, which is sound because a major re-derives every bit (§6.1). Polls occur at every call and
  back-edge, so between two polls the inline path appends at most one loop iteration's stores; the hard end is out
  of reach, and the guard page turns any mistake into a fault rather than corruption. Pages above the soft limit that
  an overflow committed are decommitted once the buffer is drained, with §4's hysteresis.

**Decommit** has hysteresis, works on coalesced runs and never runs inside the pause.
- A block becomes a candidate after **2 consecutive majors empty** (`empty_majors`, counted by the post-pause block
  classification of §6.1). The heap keeps at least `(target − L) + free_reserve` of committed empty capacity, so the
  next cycle's budget never re-faults (25–60 µs per MiB here [P, probe `cycle.c`]).
- Only **4 MiB-aligned runs** are decommitted, data and metadata together: one 16 KiB metadata page covers 8 blocks,
  so decommitting one block's metadata would zero live neighbours' bits. In 4 MiB runs, 464 MiB took 1.8 ms of
  syscalls against 14.6 ms in 32 KiB pieces [P, probe `decommit.c`].
- Decommit is queued by the epilogue and performed after the world is released, from the poll slow path,
  rate-limited to 64 MiB per poll and logged as a non-mutator interval.
- LOS runs go into a **recycle cache of up to 32 MiB** and are decommitted only after two consecutive majors unused,
  so deep continuation snapshots do not fault pages in and out every cycle.
- **Register stacks and store buffers** follow the same rule: stack pages above `top` unused through two
  observations, and store-buffer pages above the soft limit once drained, are decommitted in 4 MiB runs after the
  pause. An observation is a major or the driver's return to the top level, so a program that stops allocating after
  a deep recursion still gives the memory back (a 10 M-frame recursion would otherwise keep about 1.37 GB [P]).

### H.4 Parallel stop-the-world marking (→ SP)

- **Parallel stop-the-world marking (stage P)**, independent of decision 7: it needs GC worker threads, not mutator
  threads. Workers are per heap, spawned lazily when a major on that heap first passes a live-size threshold (about
  32 MiB), parked between collections and joined at teardown, so small heaps never start one; they get raw views of
  the reservation, metadata and block table, and run only while every mutator is stopped, so no `threaded` build is
  needed. Marking sets `STATE` with a CAS loop on the metadata byte, only during GC; work stealing runs over the mark
  stack's 4 KiB segments; the ephemeron fixpoint is sharded by key, with `KEYHINT` set by `fetch_or` in the same byte
  as `STATE`, so a key marked while another worker registers an ephemeron on it is seen by one of the two. Roots,
  frame walks and host payloads stay on the collecting thread. **Determinism:** a non-moving mark finds the same
  objects in any order and the per-worker sums commute; evacuation destinations stay sequential (one thread, or
  candidates copied in source-address order after the parallel mark), so addresses and identity hashes never depend
  on scheduling. A `workers=4` lane must stay byte-identical with the single-threaded lanes.

### H.5 Rooted loading and nested loops (→ S2 and S4e)

- **Top-level import hoisting.** A bare top-level `(import …)` is recognized in `eval_datum` on both backends **by
  the binding of `import`** (the core-syntax marker the global environment binds to that name, never its spelling),
  as `is_define_library_form` already routes `define-library` (`crates/patina-vm/src/backend.rs:238` [S]), and its
  import sets are processed outside the desugarer with only the set list rooted. Today every program import runs
  inside `desugar_with_imports`' defer guard (`crates/patina-frontend/src/desugarer/mod.rs:1841` [S]).
- **Points A and B** (between a library body's forms; between libraries) are collection points. The registry's
  `Loading` entry holds the body as one heap list and the in-progress `lib_env`; `with_globals`' saved environment
  moves onto a traced `globals_stack`. A body loaded from a hoisted import runs in the outermost VM loop, so it
  collects inside its forms too (point D).
- **Nested VM loops** (`across_reentry`, `run_apply_proc`, a load requested by running code) stay deferred until stage
  4e deletes the weak continuation tables, whose soundness rests on "nested loops defer"
  (`crates/patina-vm/src/runtime/vm_state/gc_roots.rs:21-28` [S]). Then the `environment` family's loads become
  `Step::LoadLibrary`, and after an audit the loops the VM enters from its own driver level (`across_reentry`) may
  collect; loops entered beneath a `Cx` (residual `apply_proc` fallbacks, point C) stay deferred by type, for good.
- **Point C** (an import met mid-form) and nested tree-walker trampolines keep `NoGcScope`.
- **`%parameterize-swap!`** stops calling back at stage 4a: the standard ports become parameter objects with
  validating converters, so the swap installs only into parameter objects; any other parameter-like procedure is
  installed by a Scheme loop in `lib/scheme/base/parameters.scm` with a `guard`-based undo around a non-reentrant
  `%parameter-install!` (today's transactional undo is in Rust, `crates/patina-primitives/src/primitives/
  parameters.rs:195-245` [S]).

---

## Appendix F. Test lanes, named tests and the GC benchmark set (→ `docs/TEST_ORGANIZATION.md`)

Each entry is added to `docs/TEST_ORGANIZATION.md` by the stage that creates the lane, test or program; until then it
is work-item detail of that stage's issue. §14 keeps the rules these serve.

### F.1 Differential lanes

| Mode | Builds | Backends | From |
|---|---|---|---|
| off (`(gc)` still collects) | release, debug-poison | both | now |
| default | release, debug-poison | both | now |
| stress | release, debug-poison | both | now (every n allocations); 5e (every n polls) |
| zeal-major + verify | debug | both | 5a |
| zeal-entry (a collection at every poll site; `+ verify` from 5a) | debug | VM | 3 |
| zeal-minor + verify (with `samedepth1000` and `retained-continuations` under an RSS bound) | debug | VM | 7 |
| move-all + verify (poison, `PROT_NONE` from-space) | debug | VM | 8 |
| two-mutator | debug | VM | 9 |
| GC workers = 4, compared with the single-threaded lanes | release, debug-poison | VM | P |

The `StepTracer` tests run in the debug-poison and zeal lanes.

### F.2 The reclamation proofs

**Rewriting the reclamation proofs, with stage 1's byte trigger.** They assert collector internals today:
- `(gc-stats)` `'pairs`, `'free-pairs` and `'last-swept` in `crates/patina-tests/tests/common/mod.rs:644,706,844`;
- `'pairs` and `'collections` in `run_gc_differential.sh:139-175`;
- allocation-counted stress (`:38-40`).

They must move in **the change that first counts bytes in the trigger**, not at 5e:
- its 8 MiB byte floor is above the default-mode proof's churn (200 K conses ≈ 3.2 MB of pair slots), so that
  proof would stop collecting at all (`run_gc_differential.sh:160-175` [S]);
- at 5b pairs leave the arena, after which `'free-pairs` stays 0 and every `'pairs` delta passes vacuously, the
  failure mode the script itself warns about (`:48-66` [S]).

The rewritten proofs use representation-independent keys: `live-bytes` after a full `(gc)` minus a baseline below a
bound; `committed-bytes` that do not grow across a churn loop; `collections`; `bytes-reclaimed`. Churn is sized
above the byte floor (at least 16 MiB of allocation). A guard asserts that `bytes-reclaimed` is non-zero, so no
later sub-stage can pass vacuously. The old keys stay as diagnostics only. Stress still counts allocations until
5e, so "more than 1000 collections over 20 K conses at stress 16" still holds; after 5e it counts polls, and each
iteration makes a call.

### F.3 Tests named by the design

Port finalization (§6.7; both backends; outside the byte-identical lane because GC-time flushing is observable):

| Test | Requirement | What it checks (chibi and Gauche answer the same) |
|---|---|---|
| `garbage_port_flushed_by_collection` | R2 | a file port written and dropped mid-program is empty on disk before a collection and holds its output after it, once with an explicit `(gc)` and once with a collection triggered by allocation |
| `descriptor_exhaustion_retries` | R3 | 100 K unclosed opens under `RLIMIT_NOFILE` = 1024, set in a subprocess, all succeed (today the 1,021st fails); the `port-churn` probe of §14 measures the same shape |
| `one_port_one_object` | R6 | `(eq? (current-output-port) (current-output-port))`, the same inside `with-output-to-file`, `(eq? p p)`, and `(eq? (current-output-port) p)` inside `parameterize` answer `(#t #t #t #t)` (today `(#f #f #t #f)`) |
| `dropped_interpreter_flushes_ports` | R5 | an embedder that drops its interpreter, with an unclosed file port reachable from a global, and returns from `main` finds the file written |
| `exit_flushes_every_interpreter` | R1 | `exit` from one of two interpreters writes out both interpreters' open file ports |
Other named tests, all on both backends unless marked:
- **Raise sites (stage 4d):** a collection and a `call/cc` inside a `guard` handler for errors raised by inline
  operations: `(car 5)`, `(vector-ref v 99)`, an unbound global, and a shadowed `+`.
- **Interrupts (stage 3):** a helper thread posts `SIGNAL` into a self-tail loop that does not allocate, and the loop
  stops with the condition; a thread that posts in a tight loop while interpreters are created and dropped never
  writes freed memory (debug-poison lane, and Miri over the handle and a `Mutator` twin).
- **Embedding (stage 3):** trybuild compile-fail cases for a host `Prim` that stores its argument in a `static` and for
  a `Value` from `with` passed to `call`; `call` of a closure that allocates enough to collect while its arguments are
  held only by their handles; a host `Prim` and a host `Leaf, NoAlloc` primitive; a resumable host primitive whose
  callback captures a continuation that is re-entered after the primitive returned, which must resume the call as
  chibi and Gauche do (#471).
- **`large_request_after_garbage` (stage 5e):** with the heap filled close to `--heap-max` by garbage, a
  `(make-vector N)` that fits only after a collection succeeds, and so does a deep capture in the same state.
- **Young large objects (stage 7):** in the zeal-minor lane, `samedepth1000` and `retained-continuations` keep peak RSS
  within 10% of whole-heap mode, and `bytes-reclaimed` counts the young LOS runs minors release.
- **Thread lifetime (stage 9):** a terminated thread that is never joined gives its register stack back at the next
  poll; a thread never started holds no stack; teardown unmaps every remaining thread stack; `blocked-threads`,
  scored by hand against Gambit and chibi.

### F.4 The GC benchmark set

**The harness.** Stage 0 adds a `gc` mode to the existing checked runner (`scripts/benchmarks.py` with
`crates/patina-tests/bench_programs/workloads.json`, reworked in #599) rather than a second runner, and vendors the
measurement probes the measurement table cites (`madv.c`, `cycle.c`, `decommit.c`, `mapjit.c`, the `mmap` placement
probe and the continuation toy) under `scripts/gc_probes/`, each self-describing in a header comment, so the evidence
behind the decommit mechanism and the reservation rules stays reproducible.
1. **Programs.**
   - **The 20 measured workloads**:

     | Group | Workloads |
     |---|---|
     | allocation | nboyer, deriv, gcbench |
     | mutation | destruc, quicksort, gcold |
     | large live heap | mperm, queue3 |
     | flonum | fibfp, mbrot, nucleic |
     | continuations | ctak, fibc, generator |
     | deep recursion | deeprec |
     | library loading | libload |
     | supplementary | hashtable0, eqtable, dynamic, earley |

     The Larceny-derived programs are LGPL and **not vendored**. They run from `~/Project/reference/larceny`, as
     `run_larceny_tests.sh` does, and are skipped loudly when absent. Their scaled inputs and run-benchmark shims
     are Patina-authored parameter files and are vendored. Inputs are scaled so that every run takes at least 1 s.
   - **Patina-authored probes**, vendored in `crates/patina-tests/bench_programs/gc/`: `samedepth1000`,
     `escape1000`, `pingpong1000`, `ctakdeep`, `abort100`, `frag-mix` (16 B and 48 B objects with interleaved
     lifetimes, the shape of Wingo's livelock), `ephem-chain-16k`, `port-churn` (100 K unclosed opens under
     `ulimit -n 1024`), `open-close-10k`, `retained-continuations`, `small-heap` (20 MiB of garbage, about 0 live),
     `many-heaps` (300 interpreters), `deep-unwind` (a non-tail `map` over 1 M elements), `deep-descent` (a recursion
     1 M frames deep that allocates nothing until the bottom, so one minor scans the whole descent), `blocked-threads`
     (100 K threads blocked for ever on fresh mutexes, reporting retained memory; decision 23), `display-loop`,
     `parameterize-loop`, and two loops that must poll on every iteration under `PATINA_GC_STRESS`: an allocating
     mutual tail recursion (`even?`/`odd?` with constant windows) and a `call/cc` re-entry loop.
   - **`large-live`**, a Patina-authored probe at 0.5 GiB and 1 GiB of live data in the new layout (a balanced tree of
     records and vectors built once, then steady short-lived allocation that forces majors). It reports max major pause
     and MMU(10 ms), and it is what checks decision 6's budgets on a large heap and gates stage P. No other GBS program
     exceeds 50 MB live [P], so without it nothing measures a major on a large heap.
   - **The six barrier programs** of the barrier study (vecsort, hashtab, queue, tree, strport, letrec), which K2
     names.
   - **Fixnum twins** of fibfp, mbrot and nucleic, which separate encoding costs from numeric dispatch (K6).
   - **I/O workloads** from the Larceny checkout: cat, wc, string, slatex, bibfreq and read1. Stages 4a and 9 change
     every port operation.
   - **A tree-walker subset**: fib 25, nboyer, deeprec at 200 K, libload, and a 200 K-deep tree-walker recursion.
2. **Metrics per run:** wall and user time, instructions retired, cycles, peak RSS, peak footprint and page reclaims
   (`/usr/bin/time -l` on macOS; `perf stat` and `/usr/bin/time -v` on Linux); resident size and footprint after a
   final `(gc)`; the GC CSV of §13.
3. **MMU** from the timeline of every non-mutator interval, per workload.
4. **Statistics.** ABA ordering (main/branch/main) with at least 10 rounds. Thresholds under 2% are judged on
   instructions retired or cycles, never on wall time: `/usr/bin/time` reports real time at 10 ms resolution, while
   instructions retired were stable to about 0.003% over 5 runs of fibfp [P]. Bootstrap 95% confidence intervals on
   per-workload ratios and on the geomean; a gate passes only if its interval clears the threshold. `PATINA_GC=null`
   gives the lower bound.
5. **The kill-criterion table** names the programs and the metric for every criterion.

The allocation census, survival sampling and store-mix counters behind most [P] numbers (a 1,477-line patch over
13 files in 5 crates, written for the research) land in stage 0 as the `gc-census` cargo feature, which must stay
clippy-clean under `--all-features`, so later census-based gates (4g's per-kind check, the 5e `Drop` census,
survival for bypass and K4, store mix for M2) have an in-tree tool.

---

# Part IV. Review record (→ the description of the PR that lands Parts I and II; not committed)

## Appendix A. How this design answers the panel's findings

This records how the design answers the panel that reviewed the five candidate proposals it was synthesized from
(named by their short names: chez, bounded, evolve, mmtk, jit-first); the "judges" are that panel's reviewers, by
specialty.

**Fatal flaws raised by the judges, and their fixes.**

| Flaw | Where it was raised | Fix here |
|---|---|---|
| Releasing code only at majors breaks the two 2,000-form tests that run with no collection | every proposal | the per-unit `escaped` bit and eager release at form end (§6.8) |
| The interpreter calling convention redesigned together with continuations, before the spike | jit-first | 4d changes the frame *layout* only and keeps `return_reg` and `Rc` code liveness; 4e is separate; the JIT protocol waits for the stage 6 spike |
| Unforwarded young registrations finalized, so live ports close under sticky or in-place promotion | chez | young entries are finalized only if **neither forwarded nor marked** (§6.7) |
| Young ephemerons with old keys broken at minors; guardians after breaking | jit-first | in minors, old, immortal and immediate keys count as live; guardians resurrect inside the fixpoint, before breaking (§6.6, §6.10) |
| A 2⁴⁰ fuel quantum loses interrupts; non-atomic event words written by signal handlers | bounded; evolve, mmtk, jit-first (unsoundness) | every asynchronously written word is atomic; the Dekker owner-reset protocol and `set_limit`, so no request is lost (§9) |

**Must-haves that conflicted, and how they were resolved.**
- **Who writes the limit word.** The JIT judge wanted events posted by zeroing the limit word; the Rust judge wanted
  remote requesters to write only the event word.
  - Both posters write `reg_limit`, but it is `AtomicUsize`. The owner writes non-zero values only through
    `set_limit`, which re-checks after a `SeqCst` fence (OCaml's `domain.c` precedent).
  - The Rust judge's underlying concerns, soundness and lost requests, are met. The literal "limit words are
    owner-only" is not, because then non-self tail-call loops could not see a remote event without a tick.
- **The return barrier.** The JIT judge wanted "no compare on every `Return`"; the migration judge wanted "no
  GC-installed return barrier before the spike".
  - The barrier arrives at stage 7, after the spike.
  - In the interpreter it is a flag bit in a word `Return` already loads, so it is not a protocol change.
  - K8 can switch it off.
- **Result delivery.** The JIT judge wanted results delivered in a register to the caller's resume code. That is
  adopted as the JIT's own convention (the first `Tail` argument); the interpreter keeps `return_reg`, per the
  migration judge; the frame header carries both.
- **Stack growth.** The bounded proposal's stack-scan budget is rejected. The GC-theory judge asked that stack growth
  never trigger extra minors without measurement, and deeprec would take on the order of 10³–10⁴ extra minors.
- **Poll sequence in test lanes.** The JIT judge asked that lanes run the poll sequence production ships. The GC
  lanes do; Tick mode serves only deterministic SRFI 18 preemption, and K7 measures the gap.
- **Pin-on-hash** (evolve's default) is replaced by the BFG extension (GC-theory and Rust judges). Pinning becomes
  per block (Chez), which frees a metadata bit for `HASH_MOVED`.

---

## Appendix C. What the adversarial review changed

**What changed since v1.** Five adversarial reviews (R7RS semantics, JIT feasibility, migration, performance,
Rust soundness) raised 66 findings, one of them a blocker. Every finding was checked against the source at
`28a94f8` and resolved. The largest changes: `(gc)` collects at its call; safepoint maps cover every pc where a frame
can be suspended (raise and deoptimization sites included), windows are initialized and dead slots are still
cleared; two helper classes (`Leaf`, `Transfer`) replace three; code references are ordinary heap references, so a
captured continuation is a value-only object; stage 4d keeps `Rc` code liveness until 4e; collection needs the
driver's `&mut Heap`, which no `Cx` can produce; whole-heap pacing is `max(8 MiB, 2·L)`; decommit has hysteresis
and runs outside the pause; four kill criteria are re-specified; the effort total is corrected. A completeness
round then specified variant R's binding records, defined bounded pauses and added the optional parallel-marking
stage, made weak processing part of the contract, added K16, the embedding calls, the interrupt handle, isolates as
an alternative, the heap and stack limits, and split this text into the four parts of its header. A second
completeness round put the minor pause in the same budget form as the major, with the store buffer's soft limit
derived from it; gave the LOS young and old lists so minors free dead large objects; made user-sized requests and
capture collect and retry before failing; added SRFI 18's objects, root rule and stack lifetime; made the collector
generic over its object model and lent it the mutators; moved primitives into the immortal space; pinned the
interpreter tier while debugger hooks are attached; compared the contract with HotSpot's GC interface; replaced
unsourced and secondary citations; corrected the 1 GiB pause and capture numbers; budgeted stage P; and moved the
inventories, dated estimates and test lists out of Part I into Parts III and IV, with decision 22 now covering the
split itself.

The full record, one row per finding with its verification and disposition, is the review-dispositions file attached
to this PR. The decisions that changed in the adversarial round:

| Area | v1 | v2 |
|---|---|---|
| `(gc)` | a heap-only `Leaf` primitive that posts a request | a `Transfer` primitive returning `Step::Collect`, from stage 1; collects at its call (§6.6) |
| Safepoint maps | at calls and polls only; dead slots skipped | at every suspension point (raise and deopt sites included), dense O(1) index; windows initialized at push; dead slots cleared at every scan (§8.1) |
| Helper classes | `Leaf`, `MayGc`, `MayTransfer` | `Leaf` (+ `NoAlloc`) and `Transfer`; every non-`Leaf` helper answers "continue" or a target (§8.2) |
| Tier 2 | raw slots marked in maps | tagged values only at suspension points (§8.2) |
| Code references | raw `CodeDesc*`, a `SlotVisitor::code` edge | ordinary references to descriptors; continuations value-only (§2, §6.8, §10) |
| Stage 4d | raw code pointers, "four `Rc` operations gone" | `Rc` side vector kept until 4e (§8.1, §15) |
| Polls | frame push, self tail call, `MayGc` returns; non-self tail calls only on resize | every frame entry, every tail call, every `Transfer` return; serviced only on complete frames (§9) |
| Who may collect | `NoGcScope` counter; brand claimed to make retention a compile error | the `&mut Heap` / `GcDriver` capability; `Cx` cannot collect; VM core declared a trusted island; unsafe boundary written down (§8.3) |
| Handles | index + generation | heap id + generation + `Weak` table; scopes own the capability; pin counts sticky `u16` (§8.3, §4) |
| Remembered set | store buffer dropped at majors; cells re-armed only by minors | cells re-armed at every generational major; green-thread slots minor roots; bulk stores one range; deferred overflow discarded (§6.1, §6.2, §7) |
| Variant C | "deletes caches, shadow bits and per-site guards" | keeps per-binding guards; `WATCHED` test on cell stores (§17 decision 2, §7) |
| Pacing | √L target, first major at 32 MiB, for all heaps | `max(8 MiB, 2·L)` whole-heap; √L only for generational majors after A/B (§13) |
| Decommit | `MADV_FREE` to `reserve + 1·L` after every major, in the pause | per-OS mechanism, 2-major hysteresis, 4 MiB runs, after the pause (§4) |
| Kill criteria | K1 p95 escape; K3 committed/live; K6 interpreter wall time; K8 on fib/tak; K11/K12 fallbacks inside the contract | per-workload cycles and MMU; fragmentation metric; inline flonum ops in both arms; deep-unwind workloads; S2 and native maps as separate designs (§16) |
| Stage 2 | rooted loading A/B/D/E, nested loops collect | top-level import hoisting; A, B and outermost D only; nested loops wait for 4e; tree-walker E dropped (§8.3) |
| Effort | 62–88 weeks (table summed to 70–99) | 88–122 weeks, stage P included (§15, Appendix D) |

---

## Appendix G. Alternatives rejected, by prior-art row

The third column of §0 before it moved here; §0 keeps what was taken and its primary sources.

| Decision (§0 row) | Alternatives rejected, and why |
|---|---|
| Allocation never collects; collection only at polls; Rust holds raw values between polls | Allocation that may collect, with handles everywhere (SpiderMonkey's three-year exact-rooting effort, V8 `HandleScope` plus gcmole, Racket BC `xform`): it would put a rooting obligation on ~1,300 Rust call sites |
| Raw tagged addresses; tag folded into the displacement; 16 B alignment; headerless pair, procedure (code first) and record (type descriptor first) | Today's per-arena `u32` indices (a per-type base that moves). 32-bit compressed fields (V8): they break 61-bit fixnums in fields. NaN-boxing: it loses 61-bit fixnums |
| Self-tagged flonums, judged with inline flonum operations in both arms | Boxed flonums (19.8% of all allocations [P]); NaN-boxing. Judging the encoding by interpreter wall time while generic numeric dispatch is 51% of fibfp's samples [P] |
| 16 B granules with one side metadata byte each; mark-region allocation (bump into holes); lazy sweep that reads only metadata | Chez's 19 spaces × 8 generations and its 3-level radix lookup. Size-class free lists (OCaml 5): RC Immix measured +7% retired instructions for free lists |
| Opportunistic evacuation: candidates under ¾ live, start above 10% fragmentation and stop below 5%, a free-block reserve | Full copying (2× space). Never moving: Wingo shows a fragmentation livelock with no cure |
| Sticky mark bits for generations, behind a switch | Assuming generational collection pays: Wingo found it slower on nboyer and splay (https://wingolog.org/archives/2025/02/09/baffled-by-generational-garbage-collection), and Go rejected it after its request-oriented collector's barrier cost 30–50% (Hudson, ISMM 2018 keynote, https://go.dev/blog/ismmkeynote). Assuming it loses: HotSpot's ZGC went generational (JEP 439) and dropped its non-generational mode (JEP 490), and Shenandoah followed (JEP 521). Hence a switch and a measurement (K1). A copying nursery first: it needs the largest prerequisite set, so it is kept as kill criterion K4 |
| Field-logging barrier with armed-bit polarity, an inline slow path, a per-mutator store buffer | Chez's store buffer: 0.5–1.0 entries per barrier store [P]. Cards: they need heap parsability and misfire under sticky marks. OCaml's ref table has no deduplication: 24,504 entries against 5 on nboyer [P]. G1's pre-JEP-522 barrier bloat |
| One GC interface consumed by every execution tier; barriers expanded late from one definition; initializing stores need no barrier | From HotSpot's interface (JEP 304): several production collectors chosen at startup (`-XX:+UseSerialGC`, `-XX:+UseG1GC`, `-XX:+UseZGC`, …) behind virtual calls, which HotSpot itself is pruning (JEPs 474, 490); per-collector code-generation hooks inside each compiler (`BarrierSetC1`, `BarrierSetC2`), which HotSpot needs for two JIT compilers and several collectors, while Patina has one JIT and one production collector, so a data description is enough. Expanding barriers early (JEP 475 measured 10–20% of C2 compile time) |
| Ephemerons resolved by key; an ephemeron is never older than its key or value; one fixpoint | Round-based rescans: O(n²), 289 ms on a 16 K chain [P] |
| A finalization registry split by generation; finalizers in Rust only; guardians fill queues | Java-style finalizers (deprecated by JEP 421: resurrection, unpredictable latency) |
| Identity-hash state in side metadata, plus an extension word on move | Pinning on first hash (Guile's `hashq`): it blocks evacuation on eq-table heaps (eqtable: 500 K hashed pair keys [P]). A moved-hash side table: a probe on every call. GC-rehashed native tables: Patina's tables are written in Scheme |
| JIT values published, as tagged values, to VM frames before every non-`Leaf` call; no native stack maps | Cranelift user stack maps in any tier. Raw (unboxed) slots in published frames: the same `(code, pc)` would hold tagged values in one tier and raw bits in another. Conservative stack scanning: Patina's #423 ephemeron tests forbid it |
| Two helper classes: `Leaf` (no GC, no Scheme, no transfer) and `Transfer` (caller publishes; helper answers "continue" or a new target) | A class that may collect but must return to the same fragment: preemption, signals and deoptimization all resume elsewhere |
| The minor-GC stack watermark as a return barrier, lowered in strides | A compare on every `Return`. A budget that forces a minor whenever the stack grows. Lowering one frame per cold path: deep allocating unwinds take it on nearly every return |
| Poll folded into the stack limit; only the owner resets it, then re-checks | HotSpot's poll page: it relies on a trap, which Cranelift cannot express as a safepoint. Today's poll before every instruction costs +1.1–1.4% (GC_DESIGN §6.1) |
| A deterministic tick quantum for preemption | Preemption driven by a timer in the test lanes |
| Triggers in bytes plus external bytes; whole-heap interval 2·L; a √L major rule only alongside a nursery; MemBalancer opt-in | Counting objects (today). The √L rule without a nursery: 1.15–2× more full marking than 2·L on the large-live GBS workloads and a 32 MiB first major that inflates small programs [P, review arithmetic]. Time-based sizing in deterministic lanes |
| Decommit with hysteresis, in coalesced runs, after the pause; per-OS mechanism | Decommitting to `reserve + 1·L` after every major (re-faults the next cycle's budget, 25–60 µs per MiB [P]); syscalls inside the pause (14.6 ms for 464 MiB in 32 KiB pieces [P]); `MADV_FREE` alone, which on macOS leaves RSS unchanged [P] |
| Continuations as immutable traced snapshot objects (design A) | Segments or C′ first: medium-high risk to the matrix. Cranelift `stack_switch`: x64-only and one-shot |
| Fast paths as data; static selection; spaces composed into plans; a null collector as lower bound | A catalogue of production collectors: ZGC, Shenandoah and Lua each retired a second mode for its maintenance cost. `dyn` on fast paths |
| One declarative layout specification generates every traversal | Hand-written tracers: Patina's went exponential before deduplication (6.8 s per GC at depth 26) |
| Rust API: a context argument that cannot collect; collection only through the driver's `&mut Heap`; a `'gc` brand; root scopes and owned handles (heap id plus generation) only at boundaries | Shadow stacks everywhere. Conservative scanning of the Rust stack (Alloy). A brand without a separate collect capability: values survive a nested collection |
| The mutator is the carrier, not the green thread; protocols written for N mutators | OS threads now. Isolates as a premise of the GC |
| A bespoke in-tree engine with MMTk-shaped seams | MMTk as the day-one engine: one instance per process (issue #100) against 318 heaps in one test process [P], GC on worker threads, an untagged `ObjectReference` that forces pair headers, aarch64-darwin at tier 2 |
| Patina's own code reservation with `MAP_JIT`; W^X toggled in one install function; no code patching | `cranelift-jit`'s `JITModule`: plain `mmap` plus `region::protect`, no `MAP_JIT`, and it frees only whole modules |
| No incremental or concurrent marking now; an SATB arm reserved for slow paths | Load barriers (ZGC, Shenandoah: about 5.4% average read-barrier cost); Larceny's regional collector (about 1.8× elapsed time) |
| Parallel stop-the-world marking on per-heap GC worker threads, a measured and budgeted stage for large live heaps, independent of mutator threading; single-threaded in the deterministic lanes | Chez's ownership-partitioned parallel collector (no speed-up with one mutator). Racing evacuation with CAS forwarding: addresses would depend on scheduling, and Whippet's parallel evacuation has a known race (https://wingolog.org/archives/2025/07/08/guile-lab-notebook-on-the-move) |
| Young and old lists in the large-object space | Releasing LOS runs only at majors: every dead young object over 8 KiB would wait up to 256 minors or max(1 GiB, 64·L) of allocation |
| SRFI 18 objects; threads blocked for ever rooted until they terminate | Collecting a thread blocked with no timeout on unreachable objects: it frees memory, but diverges from Gambit and chibi, which both keep such threads (decision 23) |
