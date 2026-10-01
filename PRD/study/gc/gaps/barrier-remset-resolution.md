# Write barrier and remembered set for Patina: resolving the conflicting reports

Scope: Patina at `28a94f8` (read-only), its planned generational, sticky-mark or mark-region heap, and a
Cranelift JIT. Tier 1 keeps Scheme values in the VM register file at safepoints. Tier 2 may use
Cranelift user stack maps. Tags: **[V]** means verified in source, a primary document or my own
measurement; **[I]** means inference. The instrumentation was a patched *copy* of the repo (not retained;
the `gc-census` feature replaces it), and its workloads are in `PRD/study/gc/probes/barrier-remset/work/`; the repo itself was not
modified. I found no `workload-demographics` report with a "store mix" in the study's working directory, so §2 measures
the store mix directly.

---

## 0. Bottom line

1. **Card scanning is sound with headerless 16 B pairs under stated conditions.** immix-mmtk.md
   §7/§12.6 says it is "impossible". That is wrong as stated. What card scanning needs is:
   - a scannable *space*: every word in a card is either a valid value or an immediate-looking
     header or padding, or the block holds only one kind of object;
   - dead objects masked by retained mark bits;
   - the nursery never card-scanned.

   Chez does exactly this (§3). Each condition, however, is a **constraint on the representation
   redesign**, and Patina's `HeapObjectData` does not meet it today.
2. **Patina's measured store mix** (§2) rules out a Chez-style SSB.
   - Chez's runtime filter skips only fixnums, so 50–100% of barrier-site stores would become 8-byte
     entries carved out of the nursery. On a queue workload that is 1.5 M entries (≈12 MB).
   - Most remaining heap-valued stores go into **young** holders (5–97%). Most old-holder traffic hits
     a few hot fields (nboyer: 32,882 old→young stores into **5** distinct fields across 5 simulated
     minor GCs) or many distinct small objects (hashtab: 131 K).
3. **Recommendation:** a **field-logging barrier on an "armed" log bit in side metadata**, as in
   Whippet, LXR and MMTk's FieldBarrier.
   - Polarity is LXR's: 1 means armed. Fresh or nursery memory is 0, so stores into young holders
     fall through with no young check.
   - The barrier is **pre-write**, with a dynamic or static immediate-value pre-filter.
   - The slow path is an **inline, call-free** append of the slot address to a sequential store buffer
     in reserved virtual address space. Crossing a soft limit raises the existing GC-request flag, as
     OCaml does.
   - The remembered set is that SSB, deduplicated by the bit, holding exact slot addresses. The minor
     GC re-reads each slot and then re-arms its bit.

   Whippet uses this *same barrier kind* both for its sticky-mark collector (mmc) and for its copying
   nursery collector (generational pcc), so the choice does not depend on the heap plan [V]. It
   requires no heap parsability. It can later carry incremental-update marking, or SATB once the
   value filter is dropped, without changing the fast-path shape.
4. **Cranelift fact that decides the slow-path shape:** every non-tail `call` is a safepoint.
   `cranelift-frontend` spills each needs-stack-map value that is live across *any* safepoint **at its
   definition**, and rewrites **every use** as a reload. So an out-of-line barrier call in a cold
   block degrades the hot path of tier-2 code. ocaml-gambit.md §4.4's assumption that a known-non-GC
   call can be exempted is false for Cranelift [V]. Card marking avoids this by having no slow path;
   the recommendation avoids it by making the slow path inline.
5. **Runner-up:** card marking with a young-holder filter, if (and only if) the representation team
   independently commits to Chez-style value-only pointer spaces plus retained per-granule mark bits,
   and the heap plan is a copying nursery. Keep a `BARRIER_KIND` constant in the JIT ABI (Whippet's
   `gc_write_barrier_kind`) so this stays a one-module switch.

---

## 1. The conflicting claims, resolved

| Report | Claim | Verdict |
|---|---|---|
| immix-mmtk §7, §12.6 | No cards: headerless pairs make card scanning impossible | **Overstated.** It is possible with value-only spaces (Chez) or per-granule start/extent metadata (Whippet nofl), plus dead-object masking. It is a heavy constraint, not an impossibility (§3). |
| chez.md §1, §8 | Chez card-scans headerless pairs two words at a time; adopt a direct card mark, not the SSB | **Correct** on the mechanism [V `c/gc.c:2290-2320`]. Its transfer note underplays the cost: Chez needs *spaces* segregated by pointer-ness and the record start-finding code (`gc.c:2363-2475`, "abandon hope all ye who enter here"). |
| racket-larceny §5.2 | Copy Chez's SSB | **Reject.** On Patina's measured mix, SSB entries are 50–100% of barrier-site stores (§2). The overflow stub is a call, which is a safepoint in Cranelift (§6). |
| whippet-misc §1.6, §3.2 | Field logging beat cards by 1.05–1.5× on sticky-mark heaps | **Correct**, and stronger than stated: Whippet's copying-nursery collector (pcc) uses field logging too [V `api/pcc-attrs.h:56-78`]. Caveat: Wingo's generational configurations still lost to whole-heap collection overall, and he flagged the barrier cost as unmeasured. |
| java-hotspot §3.3 | Filtered card mark or SSB | Its filter *order* (immediate first, then young holder) is right, and the initializing-store elision rule is right. For Patina, the field-log bit subsumes the young filter. |
| js-engines L7 | JSC host threshold byte | A good *ABI idea* (one barrier for both modes, switched by a VmCtx threshold), but object-granular. That gives whole-object rescans of large vectors, and pairs would need a side state bytemap (6.25%). |
| ocaml-gambit §2.3, §4.4 | OCaml ref table plus SATB, with a non-safepoint slow path | Ref table: no dedup except "old value was young"; nboyer makes **24,504** entries against 5 for field logging. Its fast path is ~15 instructions inline. OCaml *calls* `caml_modify` per store. "Non-safepoint call" is impossible in Cranelift [V]. |

---

## 2. Patina's mutation channels and the measured store mix

### 2.1 Inventory [V]

- **`&mut Heap` setters**:
  - `set_car`/`set_cdr` (`heap/mod.rs:743-763`; callers include `lists.rs:673,690` and `list-set!` at `lists.rs:733,756`);
  - `vector_set` (`:804`; `vectors.rs:196`; bulk `vector-copy!` at `vectors.rs:483` and `vector-fill!` at `vectors.rs:560`);
  - `vector_slice_mut` (`:816`), whose only caller is the VM `VectorSet` arm (`vm_state.rs:2352-2385`, store at `:2380`);
  - `set_vm_closure_free_var` (`:1394`), reached only from `StoreClosure` (`vm_state.rs:1470-1490`). The compiler never emits `StoreClosure`: it appears only in a match arm, `pass5_codegen.rs:335`;
  - `promise_update` (`:1078-1086`; callers `lazy.rs:132`, `control.rs:2069`, TW `continuation.rs:376`).
- **`&Heap` plus `RefCell`**:
  - `write_mutable_cell` (`:1273-1286`), from VM `WriteCell` (`vm_state.rs:2406-2415`);
  - `break_ephemeron` (`:1237`): GC-internal, so it needs no barrier.
- **Stores through `Rc<RefCell<…>>` that bypass the heap**:
  - record fields (`records.rs:250-262`, store at `:262` via `fields_ref[index] = args[2]`);
  - parameter value stacks (`parameters.rs:168`, `install`; `:267`, `install_parameter`);
  - promise state (`heap/mod.rs:1083`).
- **Off-heap environments**:
  - `set_slot_value` (`environment.rs:620`), `define` (`:712`), `set` (`:920`);
  - the VM's `StoreGlobal` (`vm_state.rs:1508-1527`), which **already loads the old value** for `mark_if_shadowing_primitive_value`, and `Define` (`:1530-1535`).
- **Cells:**
  - Boxed params and boxed internal defines are allocated in the lambda prologue as `AllocCell` of `UNSPECIFIED` (`pass5_codegen.rs:1135-1147`).
  - `WriteLocalCell` and `WriteClosureCell` then emit `WriteCell` (`:812-836`).
  - So every `letrec*` initialization is a *separate* store into a cell allocated earlier, with a dispatch safepoint (`vm_state.rs:1195-1206`) in between.
- **Static type knowledge** in the compiler:
  - Only *literals* are classified as immediate (`pass5_codegen.rs:698-700`; fixnum-literal `*Imm` forms at `:437-455`).
  - Inline boolean producers are `Not`, `NullP`, `PairP`, `VectorP`, `Eq`, `Lt`, `NumEq`, `LtImm` and `NumEqImm` (`primitive_calls.rs:73-137`).
  - `Add`, `Sub`, `Mul`, `AddImm` and `SubImm` can overflow to bignums, so they are **not** statically immediate.
  - There is no type inference beyond this.

### 2.2 Method

The instrumented copy hooks every channel above:
- a birth stamp at `alloc_pair`, `alloc_vector`, `alloc_string_chars` and `alloc_object`;
- a classifier at each store, given the old value where available.

It simulates a promote-all nursery: a minor GC every *E* allocations, with *E* = 65,536 by default and 16 K and 256 K checked as well. Objects born since the last simulated GC count as young. Per simulated cycle it records:
- distinct old holders (object-remembering slow paths);
- distinct old fields (field-logging slow paths);
- approximate cards, as arena index >> 5. This is exact-ish for pairs and objects, and **underestimates vectors**, which count as one card;
- Chez SSB entries (non-fixnum stores, any holder);
- OCaml ref-table entries (old holder, young new value, old value not young).

Counters reset after bootstrap, so library loading is excluded. Release build, VM backend. Workloads:
- the `jit-readiness/hist` benchmarks;
- six new mutation-heavy programs in `…/barrier-remset/work/`: SRFI-69 hash table churn, in-place quicksort of an aged 20 K vector of pairs and fixnums, a record-headed FIFO queue, a mutable record BST, `parameterize` plus string ports, and `letrec*`-captured defines.

### 2.3 Results (E = 65,536) [V, measured]

| workload | allocs | barrier-site stores | immediate % | heap→young holder % | heap→old holder % | old→young % | Chez SSB entries | obj-log slow | field-log slow | OCaml ref entries | WriteCell share |
|---|---|---|---|---|---|---|---|---|---|---|---|
| nboyer | 332 K | 227,896 | 78.8 | 6.8 | 14.4 | 14.4 | 132,867 | 5 | 5 | 24,504 | 92.7% |
| hashtab | 1.42 M | 233,169 | 10.8 | 32.8 | 56.4 | 56.4 | 213,026 | 100,013 | 131,517 | 131,515 | 14% |
| vecsort | 759 K | 883,986 | 48.4 | 5.3 | 46.3 | **0.3** | 456,863 | 7 (each = 20 K-slot rescan) | 50,946 | 2,421 | 3% |
| queue | 2.10 M | 2,401,938 | 37.5 | 25.7 | 36.8 | 24.7 | 1,501,935 | 64 | 96 | 65 | 0 |
| tree | 711 K | 513,364 | 61.6 | 29.3 | 9.1 | 9.1 | 468,260 | 42,576 | 46,269 | 46,267 | 17.6% |
| strport | 2.28 M | 401,751 | 49.9 | 50.1 | ~0 | ~0 | 201,751 | 10 | 10 | 10 | 49.8% |
| letrec | 115 K | 102,871 | 3.1 | **96.9** | ~0 | ~0 | 102,871 | 2 | 2 | 2 | 99.2% |
| deriv / primes / nqueens | 7–17 K | 522–2,909 | 13–16 | 84–87 | 0 | 0 | ≈ all | 0 | 0 | 0 | — |

Observations [V, plus I where marked]:
- **Young holders dominate the heap-valued stores.**
  - In `letrec` and `strport` they are almost entirely `WriteCell` into cells born fewer than 16 allocations earlier (the "fresh16" column, in the raw output).
  - In `tree`, `queue` and `deriv` they are list building through `set-cdr!` on fresh pairs.

  A barrier without a young-holder filter (unconditional card, Chez SSB) pays for all of these.
- **Old-holder traffic comes in two shapes.**
  - Hot fields: nboyer's 2 boxed variables, the queue's head and tail record. Every deduplicating scheme handles these: 5–96 slow paths per run.
  - Many distinct small objects: hashtab's `(key . value)` entries, tree's records. Every precise scheme records ~1 event per field per cycle; cards compress them about 6× (hashtab 21,529 cards).
- **vecsort**: 407 K of 409 K old-holder stores are old→old shuffles. A value-young check (OCaml's, or Whippet's in the slow path) removes them. Field logging without one logs 50,946 fields. Object remembering logs 7 objects but rescans the whole 20 K-element vector each time.
- **OCaml's "old value was young" dedup** works for the queue (65 entries) but not for nboyer, where `unify-subst` is reset to `'()` and then re-consed: 24,504 entries for 2 fields.
- **Nursery size** changes the counts by less than 2× (`run-epochs.txt`). At E = 16 K, hashtab field-log slow paths are 144,918; at 256 K, 86,011. queue: 384 → 24.
- **The tree-walker** sends **12.48 M** `Environment::define` calls through nboyer (VM: 10). If tree-walker frames move into the heap, these must be *constructor* (initializing) stores, not barriered defines [V count; I conclusion].

Caveats: allocation counts, not bytes; promote-all; single runs; the `hist` benchmarks are small (deriv, primes and nqueens never reach a simulated GC).

---

## 3. Question 1: is card scanning sound with headerless 16 B pairs?

Precondition common to every variant: a card scan treats each word in the dirty card as a potential
root. That is sound iff every word read is (a) an immediate, (b) a valid reference to a live-or-retained
object, or (c) skipped by metadata. How Chez meets it [V]:
- **Value-only spaces.** Vector type words are fixnums (`mkgc.ss:387-388`: "Assumes vector lengths look like fixnums"). Padding is `FIX(0)` (`:381-383`, `:403-404`). All-pointer records, whose first word is the rtd *pointer*, also go to `space_impure` (`:347-363`); a real pointer is as scannable as a fixnum.
- **A filter on non-heap words.** `relocate_dirty` filters with `FIXMEDIATE` and `MaybeSegInfo(...) != NULL` (`gc.c:685-707`), so a code-entry word or a foreign word is ignored.
- **An end marker.** The scan stops at `forward_marker` or the allocation frontier `nl` (`gc.c:2267-2272, 2313`).
- **Dead-object masking.** In mark-in-place segments it scans **only marked granules** (`gc.c:2299-2311`), and marking sets a bit for *every* 16 B granule of a multi-granule object (`mkgc.ss:2185-2275`, `within-loop-statement`).
- **Typed spaces get special handling.** Mixed-field records live in `space_impure_record` and need backward mark-bit walking or a forward walk from the start of the segment group (`gc.c:2363-2475`). Symbols and ports use arithmetic start-finding (`:2323-2361`).
- **Precise cards after scanning.** Each scanned card is rewritten to the youngest generation found (`gc.c:2545-2550`).

Per scenario:

| Scenario | Sound? | Conditions | Cost |
|---|---|---|---|
| **Segregated pair blocks** (BiBOP pair space) | **Yes** | Start of object = `addr & ~15`; both words are values. Dead pairs must be masked or overwritten. Today Patina's release sweep **does not** overwrite dead pairs (`gc.rs:906-913`: tombstone only under `cfg!(debug_assertions)`). Zeroed memory is fixnum 0 (`TAG_FIXNUM = 0`, `tagged_value.rs:76`), so a fresh tail is safe. | 1 mark-bit test per 16 B, or a store per dead pair at sweep. Card table 0.2%. |
| **Mixed bump nursery** | N/A | The nursery is evacuated or traced, never card-scanned. The question moves to *where survivors land*. | — |
| **Mixed old space produced by promotion** | Only with one of: (i) Chez's value-only layout plus segregation of raw payloads (strings, bytevectors, flonums, bignum limbs, `Rc` or code pointers) into pointer-free spaces; (ii) per-granule start/extent metadata, as in Whippet's `NOFL_METADATA_BYTE_END` (`nofl-space.h:251-269`), plus per-type tracing | Headerless pairs mixed with headed objects cannot be parsed forward from a card boundary. (i) or (ii) is required, plus dead masking. | (i) constrains every object layout. (ii) costs 1 byte per granule (6.25%), which a mark-region heap may already have. |
| **Immix lines, conservative line marking** | **Not with line marks alone.** A marked line holds dead objects whose pointers may target freed, reused lines. | Needs object-level liveness covering full extents (Chez's per-granule marks) or start/end bits, *retained between collections*. Young objects in recycled holes are unmarked and are skipped, which is fine because the minor trace reaches them. | Retained mark bitmap (0.8% at 1 bit/16 B), with a test per granule during the scan. |
| **Headers encoded as immediate-looking words** | Yes, given that: | Every header and padding word decodes as an immediate. In Patina that is `TAG_SPECIAL = 0b001` with a reserved payload range (`tagged_value.rs:77`, with `GC_POISON = 0xF8|1` and `FORWARDED = 0xF0|1` already showing the idiom). Raw payloads go elsewhere. | Forbids unboxed fields inline in pointer-bearing objects, e.g. flonum record fields or raw code pointers in closures, unless they are disguised as fixnums (Chez's code word) or the object gets a start table. |
| **Dead objects left in non-moving old blocks** | Unsound **unless masked or cleaned** | Masking: retained mark bits (Chez `marked_mask`). Cleaning: overwrite dead granules with an immediate at sweep. In a *copying* old generation, dead objects between collections of their generation are harmless [I]: cards keep their younger referents alive, so their pointers never dangle. | Eager cleaning costs time proportional to dead bytes and conflicts with lazy sweep. Masking costs a bit test. |

Contrast [I, argued]: object or field remembering **never needs parsability**. The barrier fires only
on a store into an object that is reachable at that moment. Since the last GC, only minor GCs have run,
and those keep old objects and use the remembered set as roots. So every field of a remembered holder is
valid at the next minor GC, even if the holder has died since. After a major GC the remembered set is
cleared (Whippet `clear_remembered_set`, `mmc.c:806-813` [V]).

---

## 4. Question 2: the decision matrix

### 4.1 Candidate properties

Conventions:
- `h` is the tagged holder, `v` the new value, `a = h + (OFF − TAG)` the slot address. `a` is usually needed for the store anyway.
- `vmctx` is a function argument. Fields marked "hoist" are loop-invariant between safepoints.
- aarch64 instruction counts exclude the store itself.
- **Value filter** (current tags, heap iff `(v & 7) ≥ 3`, `tagged_value.rs:76-84`) is `band_imm v,7; icmp_imm ult t,3; brif` → `and; cmp; b.lo` = **3**.
  - If the representation reform gives heap references a dedicated bit, this becomes 2 (`tst; b.eq`), or 1 if Cranelift selects `tbz`. Whether Cranelift's aarch64 backend does that was not verified [I].
  - **Recommend making "is heap reference" a single bit** in the new encoding.

| # | Candidate | Fast path (CLIF sketch) | aarch64 instrs | Slow path | Metadata | Precision → minor-GC scan | Serves SATB later? | Rust-side (interpreter, primitives) |
|---|---|---|---|---|---|---|---|---|
| 1 | Unconditional card byte (HotSpot Parallel) | `i=ushr_imm a,9; p=iadd cardbias,i; istore8 0,p` | **2** (+3 with value filter) | none | 1 B / 512 B = 0.2% | 64 words per dirty card, plus card-table sweep; needs §3 parsability | No (incremental update via a remark rescan only) | `card_mark(a)` after each store; bulk = mark a range |
| 2 | Chez SSB (`cpprim.ss:665-725`, `cpnanopass.ss:6808-6833`) | fixnum test; `eap,ap` loads; `brif ule eap,ap → ovf_cold`; `eap-=8`; write `eap` through to vmctx; `[eap]=a` | **~9**, 3 stores | `scan-remembered-set` is a **call**; it drains to cards (`alloc.c:440-498`) and only flags a collect (`S_maybe_fire_collector` → `S_fire_collector` sets `SOMETHINGPENDING`, `schsig.c:574-594`) | 8 B per logged store, carved from the nursery, plus cards | as cards, after the drain | No | push to a `Vec`, drain to cards |
| 3 | Card + young-holder filter | Plan A: `d=isub h,nlo; brif (icmp ult d,nsz) → done`, then card. Plan B: `ldrb meta[h>>4]; and 7; cmp YOUNG` | **5** (A), **7** (B); +3 with value filter | none | 0.2% (+ nursery bounds) | as #1, fewer dirty cards | No | as #1 |
| 4 | Object unlog bit (MMTk `ObjectBarrier`) | `o=h−TAG; byte=uload8 bias+(o>>6); ubfx (o>>3)&7; lsr; brif bit0 → slow` (`mmtkUnlogBitBarrier.hpp`: `meta = BASE + (addr>>6)`, `shift = (addr>>3)&7`) | **~6** (+3) | clear bit, push object. MMTk: CAS clear, then semantics (`barriers.rs` `log_object`) | 1 bit / 8 B = 1.56% | whole object: large vectors fully rescanned | Yes, as a pre-write object barrier with an object snapshot (MMTk `SATBBarrier` keys on the same unlog bit) | `if v.is_heap() && unlogged(obj) { log_obj(obj) }` |
| 5 | **Field logging** (Whippet, LXR, MMTk `FieldBarrier`) | dense bit: `byte=uload8 bias+(a>>6); ubfx (a>>3)&7; lsr; tbnz → slow`. Per-granule byte with static parity *k*: `ldrb meta[a>>4]; tbnz #(LOG0+(k&1))` | **5** dense, **3** static-parity; +3 with value filter | **inline**: disarm the bit, `[cur]=a`, `cur+=8`, compare with the soft limit, set `gc_request`. About 10 instructions, **no call** | 1.56% dense, or 2 bits inside a per-granule metadata byte (0 extra if the heap has that byte) | exact slots; ≤ distinct fields per cycle | Yes: first-overwrite logging of the old value (LXR's single barrier) if the value filter is off in that mode; incremental update works with it on | `write_field(slot, v)`; bulk = range entry |
| 6 | OCaml ref table + SATB (`memory.c:306-327`) | `old=load a`; holder-young range (3); marking flag (2); old is block and young (3+3); new young (3) | **~14** + extra load; OCaml native *calls* `caml_modify` | inline bump into the ref table; at the soft threshold, `caml_request_minor_gc` (flag), then grow (`minor_gc.c:1088-1121`) | 8 B per entry, no dedup | exact slots; one entry per old→young store unless the old value was young | Yes, built in (`caml_darken(old)`) | as written in `memory.c` |
| 7 | JSC threshold byte (js-engines §4) | `s=uload8 meta[h>>4]` (pairs have no header); `th=uload8 vmctx+THRESH`; `brif ule s,th → slow` | **~5** (+filter, usually static); the threshold is reloaded after safepoints | re-check, set state grey, push object | header byte, or a side bytemap of 6.25% unless shared | whole object (array rescans) | Incremental update by raising the threshold: one fast path for both modes [V via js-engines]; SATB no | `if state(h) <= threshold { remember(h) }` |

### 4.2 Heap plans × candidates: what each needs and what the minor GC pays

The plans:
- **A**: copying nursery in one contiguous reservation, with non-moving (optionally defragmenting) old space.
- **B**: sticky-mark Immix. Young objects sit in recycled holes of old blocks.
- **C**: mark-region with optional evacuation. Non-generational, so a barrier exists only for a future incremental marker.

| Candidate | Plan A | Plan B | Plan C |
|---|---|---|---|
| 1 Unconditional card | Sound iff old space is parsable (§3) and dead objects are masked. Nursery cards are ignored or cleared. Cost: young-holder stores still write card bytes (cache traffic only). | **Misfires**: a store into a young object dirties a card covering old objects, which are then rescanned (Wingo 2024-10-03 [V]). The scan must skip young or dead granules, so it needs extent metadata. | Incremental update only: re-dirty cards and rescan at remark. |
| 2 Chez SSB | As #1, plus SSB capacity carved from the nursery tail. Patina: 0.5–1.0 entries per barrier store (§2.3). | No single nursery region, so a separate buffer, plus #1's misfire after the drain. | Not useful. |
| 3 Card + young filter | Range test; parsability as #1. **Best card variant** for A. | Young test is a metadata load; parsability needs extent metadata. | — |
| 4 Object unlog | Arm the bit when an object is promoted. Large vectors need a card or field fallback. | MMTk StickyImmix uses exactly this [V via immix-mmtk §3]. Holes reused for allocation must have their bits cleared. | SATB pre-write mode (MMTk). |
| 5 **Field log** | Arm a promoted object's words at copy. The nursery's bits are never armed, so young holders fall through. Whippet `generational-pcc`: `GC_OLD_GENERATION_CHECK_SMALL_OBJECT_NURSERY` plus `GC_WRITE_BARRIER_FIELD`, with 8 fields per byte (`pcc-attrs.h:42-78`) [V]. | Whippet mmc: `GC_WRITE_BARRIER_FIELD` with 2 log bits per granule byte; slow path checks the value's generation then CASes the bit (`mmc-attrs.h`, `mmc.c:1143-1157`, `nofl-space.h:1152-1166`) [V]. Wingo measured 1.05–1.5× over cards [V]. | The same bits drive incremental marking. |
| 6 OCaml | Natural: range checks need the contiguous nursery. | Three metadata loads (holder, old, new): poor fit. | SATB part only. |
| 7 JSC | Works. Set state to black at promotion; arrays get whole-object rescans. | Native fit: JSC itself is sticky-mark [V via js-engines]. | Retreating-wavefront incremental marking. |

Minor-GC remembered-set cost on the §2.3 workloads, in events per run [V counts; scan sizes I]:
- **Field logging**: 5 (nboyer), 131 K (hashtab), 51 K (vecsort), 96 (queue), 46 K (tree), each one 8-byte slot re-read.
- **Object logging**: similar counts, but vecsort's 7 events are 7 × 20 K slots. Hashtab's bucket vectors (8 objects) are rescanned whole each cycle.
- **Cards** (approximate): 21.5 K × 64 words for hashtab, plus every vector card per cycle for vecsort (312 cards × 11 cycles).
- **Chez SSB**: 0.13–1.5 M drain entries.

---

## 5. Question 3: which stores need barriers, and what can be elided

**Need a barrier** (the holder can be old and the value a young reference):

1. `set-car!`, `set-cdr!`, `list-set!`, `vector-set!` (VM `VectorSet` and Rust), `vector-fill!` and `vector-copy!` as **bulk** barriers.
2. `WriteCell`. This is the dominant VM site: 93% of nboyer's barrier stores, and 2.2% of all nboyer dispatches per jit-readiness §2.3.
3. Record field set.
4. Parameter value set (`%parameter-set!`, `%parameterize-swap!`).
5. `promise_update`, which stores the inner state *and* rewrites `objects[inner]` to share the outer box.
6. Global binding cells, once they become heap cells (jit-readiness R5). `StoreGlobal` and `Define` into an existing cell are barriered.
   - `StoreGlobal` already loads `old` (`vm_state.rs:1513`), so a SATB variant costs nothing extra there.
   - Until then, environments must either be minor-GC roots (offheap.md §3.1: 25–31 µs per GC for the global environment) or be remembered as whole objects. That is OCaml's "generational global roots" idea: a dirty-environment list keyed by a flag on the `Environment`.
7. Closure free-variable stores (`StoreClosure`): never emitted today. A future `letrec` closure fix-up (patching mutually recursive closures after allocation) would be initializing stores, so it is elidable when no safepoint intervenes.
8. Hash tables: implemented in Scheme over vectors and pairs (`lib/srfi/69/…`), so they are covered by items 1 and 2.
9. Reader and printer fix-ups (`parser/mod.rs:633-646`, `datum_writer_properties.rs:233-241`, `heap/source.rs:168-172`, `eval.rs:721`): mostly young holders, but they must still go through the barriered API.

**Need no barrier:**
- Strings and bytevectors (no references).
- `break_ephemeron` and weak-table maintenance (GC-internal; immediates).
- **Register-file and frame stores.** These are roots, so the tier-1 JIT writes them freely. The register file must then be scanned completely at every minor GC. If a stack watermark is added later (OCaml `Already_scanned`, JEP 376, vm-runtime §9), use a *return barrier* that lowers the watermark on frame pop before `Return` writes the caller's `dst`, not a store barrier.
- **Continuation objects.**
  - Capture copies the frames and register file into a *fresh* object. Those are initializing stores; so is retiring dead temporaries in the copy (`alloc_vm_continuation`).
  - Reinstatement copies *out*. Captured continuations are never mutated in place.
  - If a capture is large enough to be allocated directly in old or large-object space, it must be **pre-logged**: pushed whole onto the remembered set at allocation. This is G1's `on_slowpath_allocation_exit` compensation (java-hotspot §3.3 [V via report]).
  - The same rule covers code-object constant vectors, if they are not made immortal (jit-readiness item 23).

**Elision**:

| Kind | Rule | Patina status | Measured share |
|---|---|---|---|
| Static immediate value | Literal immediates, char literals, results of `Not`/`NullP`/`PairP`/`VectorP`/`Eq`/`Lt`/`NumEq`/`LtImm`/`NumEqImm`. **Not** `Add`/`Sub`/`Mul`/`*Imm` arithmetic (bignum overflow). Chez's rule: quoted immediates, `*result-type*` ∈ {fixnum, boolean}, through `if`/`seq` with fuel 5 (`cpprim.ss:671-688`) [V]. | Only literals are classified today (`pass5_codegen.rs:698`) | Dynamic immediates are 3–79% of stores; the static share is unmeasured (M8). |
| Dynamic immediate value | Tag test before the bit test. Valid for generational and incremental-update modes. **Invalid under SATB**, where an immediate overwriting a snapshot reference must log the old value. | — | 37–79% on nboyer, vecsort, queue, tree, strport |
| Initializing store | Holder allocated with **no safepoint** in between (no call, no poll, no allocation slow path), per HotSpot JEP 475. | Today every dispatch is a safepoint (`vm_state.rs:1195-1206`), so only stores *inside* a constructor qualify. Constructors must take contents (`alloc_vm_closure(captured)`), and tree-walker frames must be built with their bindings, not `define`d. In the JIT: group a block's allocations into one bump, so `AllocCell`→`WriteCell` and `cons` chains qualify. | Young-holder stores are 5–97% of heap stores. `letrec`'s are almost all prologue-cell initializations separated by a `MakeClosure` allocation, so **unboxing write-once `letrec*` defines** (jit-readiness item 13) removes them better than the barrier can. |
| Dynamic young holder | Free in field logging (nursery bits never armed); a range test in card variants. | — | as above |
| Dynamic old value | Whippet checks this in the slow path, without setting the bit; OCaml checks it inline. | — | vecsort: 407 K old→old stores |

---

## 6. Question 4: interaction with the JIT tiers

**The Cranelift facts** [V Cranelift `user_stack_maps.rs:6-13`, `safepoints.rs:776-850`, `ir_instructions.rs:319-321`]:
- "Currently all non-tail call instructions are considered safepoints. (This does *not* allow, for example, skipping safepoints for calls that are statically known not to trigger collections…)". `is_safepoint = is_call && !is_return`.
- The frontend rewriter spills each needs-stack-map value live across *any* safepoint **right after its definition**, adds stack-map entries at each safepoint, and replaces **every use** with a reload.

So a barrier slow path written as a CLIF `call` in a cold block still forces every GC reference live across it into a stack slot for its whole live range, on the hot path too. cranelift-gc.md §2.1 reached the same conclusion.

**Tier 1 (register file, no stack maps).** Nothing is declared `needs_stack_map`, so a slow-path call costs only caller-saved register pressure around a cold call.
- The real constraint is semantic: **the barrier slow path must never collect, allocate in the GC heap, or run Scheme.** Then no cached SSA values need flushing to the register file.
- Field logging's slow path meets this by construction. Buffer overflow sets `gc_request`, which is honoured at the next poll (back edge, entry, allocation slow path), exactly as OCaml's ref table does at its soft threshold [V `minor_gc.c:1100-1104`]. Chez's SSB overflow likewise only flags (`S_maybe_fire_collector`) [V].
- Allocation slow paths *are* GC points. Tier 1 flushes dirty VM registers before them (cranelift-gc.md §6.3), and they break initializing-store elision.

**Tier 2 (user stack maps).** Emit the slow path **inline, without a call**: about 10 instructions in a `set_cold_block`.
- The buffer is a large **reserved, lazily committed** virtual region (e.g. 256 MiB to 1 GiB) whose soft limit sits well below its end. In the worst case one slot address is pushed per store between polls, and polls bound straight-line code, so the hard end is unreachable.
- Rust primitives that loop (`vector-fill!`) use range entries and may grow or flush the buffer themselves, because they are not JIT code.
- Card marking satisfies tier 2 trivially, since it has no slow path.
- The OCaml and JSC barriers can use the same inline-push technique.

**Continuations across JIT frames.**
- Tier 1: capture stays a copy of the register file and frames (`capture_full`), so the only barrier rule is "fresh object, or pre-logged if large".
- Tier 2: native-frame capture copies stack chunks into heap objects (the Loom model). These are immutable once frozen, so they are initializing stores plus pre-logging. Their *roots* need the stack-map descriptors, which is a stack-map problem, not a barrier one.

**Late expansion.** Emit every store site through one `emit_write_barrier(builder, holder, slot_addr, value, static_facts)` helper. This is the JEP 475 lesson (java-hotspot §3.13): static facts are value-is-immediate and holder-allocated-since-last-safepoint, and they decide whether to emit anything at all.

---

## 7. Recommendation: barrier, remembered set, JIT ABI

### 7.1 Barrier semantics

1. **Field log bits.**
   - Each 8-byte heap word has one *armed* bit in side metadata, addressed by the word's address.
   - Polarity: armed = 1 means "first store this cycle must be logged". Fresh memory is 0, so nursery objects and newly allocated old objects are unarmed by construction. This is LXR's "new objects are zeroed, hence logged" [V via immix-mmtk §6].
2. **The barrier is pre-write**:
   `if value_filter(v) && armed(a) { slow(a) }; *a = v`.
   Pre-write keeps the old value available for a future SATB mode.
3. **Inline slow path**: clear the bit; `remset[cur++] = a`; `if cur >= soft_limit { gc_request = 1 }`. When `barrier_mode == MARKING_SATB` (future), also push `*a` (the old value) before the store.
4. **Value filter.** Static elision of immediates, plus a dynamic tag test. Documented as valid for the generational and incremental-update modes. A SATB mode would set `BARRIER_VALUE_FILTER = false` at JIT compile time, so code compiled before the switch must be invalidated.
5. **Arming.**
   - Plan A: the minor GC arms every word of each promoted object at its destination.
   - Plan B: marking a young object arms its words.
   - Evacuating an old object (major-GC defrag) arms the destination and clears the source (Whippet `clear_logged_bits_in_evacuated_object`, `nofl-space.h:1714-1733` [V]).
   - Sweeping clears the bits of freed granules before reuse. That matters for Plan B and lazy sweep.
6. **Minor GC**: for each entry `a`, read `*a`. If it is a young reference, evacuate or mark it and update `*a`. Then re-arm `a`. Duplicates are impossible within a cycle; stale entries from holders that died are harmless (§3).
7. **Major GC**: discard the remembered set and re-derive the bits from liveness, i.e. arm all words of live old objects. This is a metadata pass over 1.56% of the heap.
8. **Pre-logged allocations.** An object allocated directly into old or large-object space (large vectors, continuation chunks, code constants) is left *unarmed* and appended to an object-granular "remember whole" list. That list is scanned at the next minor GC, after which the object is armed.
9. **Bulk stores.** `vector-fill!` and `vector-copy!` either push a `(start | RANGE_TAG, len)` entry, or walk the armed bits of the range a byte at a time.

### 7.2 JIT ABI (`#[repr(C)] VmCtx`, pinned for the VM's lifetime)

| Field / constant | Type | Read by | Notes |
|---|---|---|---|
| `alloc_ptr`, `alloc_limit` | `usize` | inline allocation | `limit = 0` forces the slow path (GC request); cranelift-gc §6.3 |
| `log_meta_bias` | `*const u8` | barrier fast path | `= table_base − (heap_lo >> LOG_BYTE_SHIFT)`. A global reservation covering nursery, old space and LOS, the MMTk style. Hoistable (`readonly`) |
| `remset_cur` | `*mut usize` | inline slow path | in reserved VA |
| `remset_soft_limit` | `*mut usize` | inline slow path | crossing it sets `gc_request` |
| `gc_request` | `u8` (shared with the poll word) | polls; slow path writes it | single-threaded, so plain stores |
| `barrier_mode` | `u8` | **slow path only** | 0 = generational; reserved: 1 = incremental-update marking, 2 = SATB marking |
| `nursery_lo`, `nursery_size` | `usize` | slow path only (optional value-young check, Plan A) | the fast path never needs them |
| `BARRIER_KIND` | const | JIT compile time | `None` (Plan C today) / `FieldLog` / `Card` (runner-up); Whippet `gc_write_barrier_kind` |
| `LOG_BYTE_SHIFT = 6`, `LOG_BIT = (addr>>3)&7` | const | fast path | dense layout, 1.56%. Alternative if the heap adopts a per-16 B-granule metadata byte: `LOG_BYTE_SHIFT = 4`, `LOG_BIT0 = 6`, bit = `LOG_BIT0 + ((addr>>3)&1)`, which is **static** for fixed field offsets of 16 B-aligned objects (Whippet mmc) |
| `HEAP_REF_TEST` | const | value filter | tag mask and compare; ideally a single bit |

Fast path, CLIF for `(set-car! h v)` with dense bits (aarch64 shown in comments). The pair-tag mask
test that `set-car!` performs anyway is not shown.

```
a    = iadd_imm h, 0-PAIR_TAG                 ; often folded into the store address
t    = band_imm v, 7
imm  = icmp_imm ult t, 3
brif imm, store, chk                          ; and; cmp; b.lo            (3)
chk:
mb   = load.i64 readonly notrap vmctx+LOG_META_BIAS   ; hoisted
bi   = ushr_imm a, 6
byte = uload8 notrap mb+bi                    ; lsr; ldrb                 (2)
sh   = ushr_imm a, 3 ; sh = band_imm sh, 7
bit  = ushr byte, sh                          ; ubfx; lsr                 (2)
brif (band_imm bit,1), slow_cold, store       ; tbnz/tst+b.ne             (1-2)
slow_cold:                                    ; set_cold_block, NO call   (~10)
  nb = band byte, ~(1<<sh) ; istore8 nb, mb+bi
  c  = load vmctx+REMSET_CUR ; store a,[c] ; c2 = iadd_imm c,8 ; store c2 -> vmctx+REMSET_CUR
  l  = load vmctx+REMSET_SOFT ; brif (icmp uge c2,l), req, store
  req: istore8 1 -> vmctx+GC_REQUEST ; jump store
store:
  store.i64 v, a
```

Totals:
- Dense layout: **8–9** instructions on the heap-value path and **3** on the immediate path.
- Per-granule-byte layout with static field parity: **6** (`and;cmp;b.lo; lsr;ldrb;tbnz`).
- An unconditional card is **2**, or **5** with a value filter. Field logging therefore costs about 1–4 instructions more per heap-valued store than cards, in exchange for no parsability constraints, exact remembered sets, Plan B robustness and a SATB option.

### 7.3 Rust side (interpreter and primitives)

- **One choke point.** `Heap::write_field(&self, holder: Gc, slot: *mut TaggedValue, v: TaggedValue)` (`#[inline(always)]`) applies the same filter, bit test and `#[cold]` logging function as the JIT. The barrier state (`remset_cur` etc.) is a `Cell` in the same `VmCtx` the JIT reads.
- **Remove `vector_slice_mut`.** Its only caller is `vm_state.rs:2380`; it becomes `vector_set_checked`.
- **Move `Rc<RefCell>` payloads into heap objects.** Records, parameters, promise state and `MutableCell` all move, so that every store goes through `write_field`.
- **Bulk API.** `write_range(holder, start, len)` covers `vector-fill!`, `vector-copy!` and `list->vector` copies into old vectors.
- **Constructors take contents.** Initializing stores happen before the object is published, with no barrier.
- **Environments.** Until environments or global cells move into the heap, `Environment::set_slot_value` and `define` set a `dirty` flag and push the environment onto a minor-GC root list (OCaml's generational global roots). This is cheaper than rescanning all 2,427 bindings (offheap.md §1) at every minor GC.
- **Cost in the interpreter.** It is negligible: each store already pays a `RefCell` borrow and an arena bounds check (`heap/mod.rs:743-816`).

---

## 8. Measurements needed to confirm the choice

1. **M1: store mix at scale.**
   - Rerun the `barrier-remset` instrumentation on r7rs-benchmarks-sized programs (ecraven suite) and on library loading and the tree-walker, at E = 16 K, 64 K and 256 K and *byte*-based epochs.
   - Add the sizes of remembered objects (object-remembering scan cost) and exact card counts for vectors. Today's counts are approximate.
   - Pass criterion for field logging: the slow-path rate is low enough that inline slow paths cost under 1% of the run at JIT speeds. Calibrate against M3.
2. **M2: interpreter barrier tax.**
   - Implement `write_field` with every object treated as old (all bits armed after each GC) and the remembered set drained at each existing collection.
   - Interleaved A/B against main (GC_STAGE5 P3 method). Expect under 1%: the interpreter pays a `RefCell` borrow per store already.
3. **M3: JIT microbenchmarks (tier 1, then tier 2).** On vecsort, queue, nboyer, hashtab and `letrec`, compare these barrier kinds on aarch64 and x86-64:
   - none (the "speed of light" baseline);
   - unconditional card;
   - card plus young filter;
   - field log, dense layout;
   - field log, per-granule static parity;
   - field log with the slow path as a CLIF *call*.

   Inspect the disassembly for tier-2 spills when the slow path is a call; this confirms §6. Also check whether Cranelift emits `tbz`/`tbnz` for single-bit tests.
4. **M4: remembered-set processing.** ns per entry in the minor GC, plus range entries, against ns per dirty card. Use hashtab and tree, which have the most entries.
5. **M5: Wingo's question.**
   - Run three variants end to end: generational on; barrier on with minor GCs disabled (barrier cost alone); barrier off and whole-heap.
   - Do this for nboyer, deriv, splay-like workloads and the tree-walker. Generational must win on throughput or pauses for the barrier to stay.
6. **M6: arming cost.** Metadata writes per promoted byte (Plan A) or per young survivor (Plan B), and the major-GC re-arm pass time per MB.
7. **M7: buffer bounds.** Maximum remembered-set entries per cycle against the soft limit. Count how often `gc_request` fires from the remembered set rather than from allocation.
8. **M8: elision coverage.**
   - The fraction of dynamic barrier-site stores removed statically: immediates, boolean producers, initializing stores after allocation grouping.
   - The `WriteCell` count after `letrec*` unboxing.

---

## 9. Verified versus inferred

**Verified [V]:**
- Chez:
  - dirty-card word scanning, `marked_mask` restriction and the record start-finding code (`gc.c:2217-2565`);
  - per-granule extent marks (`mkgc.ss:2185-2275`);
  - the space-assignment and header-as-fixnum assumptions (`mkgc.ss:333-405`);
  - `build-dirty-store` elision and the SSB push (`cpprim.ss:665-725`, `cpnanopass.ss:6808-6833`);
  - the dirty-card drain and the flag-only overflow (`alloc.c:396-498`, `schsig.c:574-594`).
- Whippet: the barrier fast path (`gc-barrier.h`), the mmc and pcc attributes, the slow path with its value-generation check (`mmc.c:1143-1157`), the metadata byte layout with LOGGED bits (`nofl-space.h:245-269`), and remembered-set clearing.
- MMTk: `ObjectBarrier` and `SATBBarrier` on the unlog bit, and `BarrierSelector` including `FieldBarrier` (`src/plan/barriers.rs`); the mmtk-openjdk unlog-bit address arithmetic.
- OCaml: `write_barrier` (`memory.c:306-327`), ref-table processing (`minor_gc.c:557-628`), and the soft threshold that requests a minor GC (`minor_gc.c:1088-1121`).
- Cranelift: the safepoint definition and the spill-at-definition / reload-at-use rewrite.
- Patina: all repository line references in §2.1; store-mix numbers from my instrumentation.

**Inferred [I]:**
- the soundness argument for object and field remembering without parsability;
- that dead objects in copying generations are harmless;
- aarch64 instruction counts;
- that the reserved-VA buffer makes overflow unreachable;
- SATB compatibility of the first-overwrite log with the value filter off (this follows LXR's design but was not proved here);
- the ranking of the candidates.

## 10. Open questions

1. Will the heap redesign have a per-16 B-granule metadata byte (mark state, start/end, young)? If so, the 2-bit static-parity layout gives the cheapest fast path. If not, use the dense 1-bit-per-word table.
2. Plan A or B? The barrier does not decide it, but Plan A lets the slow path check the value's age with a range test, and Plan B makes the young holder free only through the bits.
3. Should the dynamic immediate filter stay? It saves the metadata load on 37–79% of stores but closes off SATB without recompiling. Incremental-update marking, which re-reads the log at remark, does not need it removed.
4. Should environments and global cells move into the heap (one barrier), or stay off-heap with a dirty-environment root list?
5. Is a large-object space inside the same reservation, so that one `log_meta_bias` covers it, or separate with an "always slow" kind as in Whippet's LOS?
6. Will `letrec*` write-once unboxing land before the JIT? It decides whether `WriteCell` remains the hottest barrier site.

### Sources

- Local:
  - `~/Project/reference/ChezScheme` @ `7d82bd86` (`c/gc.c`, `c/alloc.c`, `c/schsig.c`, `s/mkgc.ss`, `s/cpprim.ss`, `s/cpnanopass.ss`);
  - `~/Project/reference/ocaml` @ `70165ff2` (`runtime/memory.c`, `runtime/minor_gc.c`, `runtime/caml/minor_gc.h`);
  - Whippet, fetched and not retained (`api/gc-barrier.h`, `api/mmc-attrs.h`, `api/pcc-attrs.h`, `src/mmc.c`, `src/nofl-space.h`);
  - Cranelift (wasmtime `main`, 2026-09-30), fetched and not retained: `user_stack_maps.rs`, `safepoints.rs`, `ir_instructions.rs`.
- Web:
  - <https://raw.githubusercontent.com/mmtk/mmtk-core/master/src/plan/barriers.rs>
  - <https://raw.githubusercontent.com/mmtk/mmtk-openjdk/master/openjdk/barriers/mmtkUnlogBitBarrier.hpp>
  - <https://wingolog.org/archives/2024/10/03/preliminary-notes-on-a-nofl-field-logging-barrier>
  - <https://wingolog.org/archives/2025/02/09/baffled-by-generational-garbage-collection>
- Measurements: the workloads in `PRD/study/gc/probes/barrier-remset/work/`; their outputs (`run65k*.txt`, `run-epochs.txt`) were not retained. The instrumentation was the patched copy's `crates/patina-core/src/heap/storestats.rs` (not retained).
