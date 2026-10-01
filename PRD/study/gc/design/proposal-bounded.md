# RegionGen: a bounded-pause generational mark-region collector for Patina

Panel proposal, starting philosophy "bounded-pause, OCaml-5 style" (candidate E). Date 2026-10-01, repo `main` at
`28a94f8` (read-only). Evidence keys follow DIGEST.md (`[demographics §5]`, `[barrier-res §7]` …). **[V]** = verified
in source or measured by a cited report, **[I]** = my inference or estimate. Every number without a tag is a design
parameter I am proposing.

**Where I deviate from candidate E, and why (stated up front so the judges can weigh it):**

| # | Candidate E said | I propose | Evidence that forced the change |
|---|---|---|---|
| D1 | Size-class pool major heap (OCaml 5) | Whippet-style **mark-region** old space (16 B granules, 1 metadata byte each, bump into holes, metadata-only lazy sweep) | Free-list allocation costs +7% retired instructions (RC Immix) [whippet §1.3, DIGEST pitfall 20]; one forwarding protocol then serves nursery copying *and* later old-space defragmentation, where OCaml needed a separate compactor (5.2) [ocaml-gambit §2.8] |
| D2 | `young_limit` is the universal poll word | A **fuel counter** (Chez `%trap`) is the poll word; the allocation limit is zeroed with it so allocation sites *at* safepoints can fold the poll | Allocation must never collect (DIGEST resolution 2: ~230–890 Rust functions hold unrooted values across allocation); deterministic preemption for green threads needs a counter [threads-rec item 7] |
| D3 | OCaml ref table + inline SATB checks (~14 instr) | **Armed-bit field logging** with no value filter: 3 instructions (static field) / 6 (vector element); SATB lives entirely in the inline slow path | OCaml's table has no dedup (nboyer 24,504 entries vs 5) [barrier-res §4]; LXR/Whippet field logging 1.05–1.5× over cards [whippet §1.6] |
| D4 | Incremental marking built in | Incremental SATB marking is a **gated Stage 10** that changes no fast path, layout or JIT code | Estimated STW major cost at Patina's *measured* live sizes (≤50 MB) is ≈5–15 ms once sweep is lazy (§6.11 [I]); Racket CS abandoned true incremental marking, Larceny regional cost ~1.8× elapsed and 2× code [racket-larceny §5.6] |
| D5 | One contiguous minor heap | A **chunked nursery** (256 KiB chunks from the block pool) whose chunks are either evacuated or **promoted in place** | queue3/deeprec survive 100% at every nursery size, so copying costs 1.7–1.8× today's marking [demographics §5.2]; in-place promotion of dense or pinned regions is gen-ZGC/JEP 423 practice [hotspot §3.2] |
| D6 | Minor GC may run inside allocation | **Allocation never collects**; collection only at polls | As D2; Chez runs a moving generational GC under exactly this contract [chez §3] |

---

## 1. Thesis and key bets

Patina should adopt **one two-generation collector whose minor pauses are bounded by construction** — by the
nursery chunk budget, the remembered-set soft limit and a register-stack scan budget — over **a non-moving
Whippet-style mark-region old space with lazy, metadata-only sweeping**, and with **a 3-instruction armed-bit
field-logging barrier whose fast path is already SATB-sound** (the SATB arm is a branch in the inline slow path). The
collector sweeps nothing in the pause, never
visits dead objects, keeps Patina's best existing property (allocation never collects; collection only at polls where
every Scheme value is in VM memory), and therefore lets primitives keep holding raw values without handles even though
young objects move. Incremental old-space marking — the "bounded by construction" answer for major pauses — is
pre-wired (barrier slow-path mode byte, allocate-black promotion, a shared return watermark, darken-on-reinstate hook)
but built only when a measured gate says the STW major pause is too long, because the measured live heaps (≤50 MB in
the compact layout) put that pause at roughly 5–15 ms and the owner ranks throughput first.

Key bets (if any is wrong, the design needs rework):

1. **Chez discipline survives a moving nursery.** Allocation never collects; collection happens only at polls; heap-only
   primitives and all Rust code between polls hold raw `Word`s; re-entry boundaries use LIFO `RootScope`s. (Evidence:
   Chez [chez §3]; DIGEST resolution 2; prim-embed §11 estimates ~40–50 sites/structs need thought.)
2. **A small nursery with adaptive in-place promotion beats whole-heap marking on Patina's demographics**, or at least
   matches it while delivering bounded minor pauses. (Evidence for: nursery trace work 2–25× lower on 7/20 workloads,
   closures/cells/flonums survive ≤0.7% [demographics §5]. Against: Wingo's nboyer/splay [whippet §1.6]. The
   whole-heap configuration is a runtime switch of the same collector, so the M5 experiment decides, §16 K1.)
3. **The armed-bit barrier costs <1% in the interpreter and <3% in JIT code** on the store-heavy workloads
   (WriteCell = 93% of nboyer's barrier stores; barrier sites ≈1–2% of dispatches [jit-ready §2.3]).
4. **STW old-space marking stays ≤ ~15 ms at the measured live sizes** once sweep is lazy, objects are compact and
   static data stops being re-marked, so incremental marking can wait behind a gate.
5. **Every holder of VM-heap references can be made updatable** (slot visitor, heap cells, code space, continuation
   objects), while the tree-walker runs the same collector in its non-moving configuration.
6. **Representation dominates.** Headers, inline payloads, raw tagged addresses and self-tagged flonums deliver most of
   the measured wins (2.05× bytes [demographics §4.4], 72 B → 16 B cells/flonums, 178 ms → ms-scale post-load pause);
   the collector design must not delay them.

---

## 2. Value encoding

A value is one 64-bit word (`Word`, `#[repr(transparent)]`, `Copy`). Low 3 bits are the primary tag. All heap objects
are **16-byte aligned**, so a heap reference is the object's address plus its tag.

| Tag | Meaning | Decode | Notes |
|---|---|---|---|
| `000` | fixnum, 61-bit two's complement | `w >> 3` (arith) | kept so `adds`/`b.vs` work on tagged words [jit-ready R1] |
| `001` | immediate | bits 7:3 = subtag, 63:8 = payload | specials, chars, and GC-internal words |
| `010` | flonum, class "negative, \|x\| ∈ (2⁻⁵¹¹, 2)" | self-tagged | see below |
| `011` | **pair** (headerless, 16 B) | `addr = w − 3`; car `[w−3]`, cdr `[w+5]` | same tag as today |
| `100` | **headered object** | `addr = w − 4`; header `[w−4]` | all other kinds |
| `101` | **closure** (word 0 = code header) | `addr = w − 5`; free var *i* at `[w+3+8i]` | one-compare `procedure?` |
| `110` | flonum, class "positive, \|x\| ∈ [2⁻⁵¹¹, 2)" | self-tagged | |
| `111` | flonum, class "positive, \|x\| ∈ [2, 2⁵¹³)" | self-tagged | |

**Heap-reference test:** tag ∈ [3,5] → `and w9,w0,#7; sub w9,w9,#3; cmp w9,#2; b.ls` (one range compare).

**Immediate subtags (tag `001`):**

| Subtag | Low byte | Payload | Scheme-visible |
|---|---|---|---|
| 0 special | `0x01` | `#f`=0 (word `0x001`), `#t`=1 (`0x101`), `()`=2, eof=3, unspecified=4, default-object=5, unbound=6, broken-ephemeron=7 | yes (except unbound, which never escapes a cell read) |
| 1 char | `0x09` | Unicode scalar in bits 28:8 | yes |
| 2–26 | — | reserved | — |
| 27 POISON | `0xD9` | debug fill of evacuated memory | never |
| 28 FILLER | `0xE1` | length of an unused gap (heap parsability) | never |
| 29 FWD | `0xE9` | pair forwarding marker (car); cdr = new reference | never |
| 30 OBJECT HEADER | `0xF1` | §3 | never |
| 31 CLOSURE HEADER | `0xF9` | `word0 = code_descriptor_addr \| 0xF9` | never |

Because no Scheme value has tag `001` with subtag ≥ 27, a linear walk can always tell a header from a pair's car, and
`#f` is the single word `0x001` (`cmp x0,#1`).

**Flonums: self-tagging (Melançon–Serrano–Feeley, OOPSLA'25) [heap-repr §11].** Encode `w = rotl(bits, 3) + 5`;
the value is immediate iff `w & 7 ∈ {2, 6, 7}` (mask test `0xC4 >> tag & 1`), else it is boxed. Decode
`bits = rotr(w − 5, 3)`. Rotating left by 3 brings (sign, e₁₀, e₉) into the tag; adding 5 maps the three most common
classes — positive small (001→110), positive large (010→111), negative small (101→010) — onto the three float tags.
Boxed: ±0.0, negatives ≤ −2, |x| < 2⁻⁵¹¹, |x| ≥ 2⁵¹³, ±inf, NaN. ±0.0, ±inf and the canonical NaN are **immortal
canonical boxes**, so producing them never allocates (`eq?` on flonums is unspecified in R7RS; `eqv?` compares
payloads). Rationale: flonums are 19.8% of all allocations and 79–99.8% in float code and survive ≤0.7%
[demographics §4, §9.4]; immediates remove that nursery traffic entirely. The constant 5 (classes {+small, +large,
−small}) is chosen for mandelbrot-shaped code; Stage 5 measures the class histogram on fibfp/mbrot/nucleic and may
pick another cyclic shift before the tag ABI freezes (§16 K7).

**Heap reference form: raw tagged addresses inside a per-heap virtual reservation.** One load per field
(`ldr [v + off − tag]`, tag folded into the displacement as Chez does [chez §1]), word compare for `eq?`, globally
unique across the 318 heaps a test process can hold [demographics §8.1], and the side-metadata table is addressed by
`meta_bias + (addr >> 4)` (§4). I reject 32-bit offsets (an add per access, and compressed *fields* would break 61-bit
fixnums [DIGEST §3.1]).

**Forwarding.** Headered objects and closures: word 0 (a tag-`001` header) is overwritten by the new *tagged
reference*, so `forwarded ⇔ word0 is a heap reference`. Pairs: car := `FWD` (`0x…E9`), cdr := new reference.

---

## 3. Object model

**Header word** (word 0 of every tag-`100` object):

```
 63                                24 23      20 19      16 15       8 7        0
+------------------------------------+----------+----------+----------+----------+
|          length (40 bits)          | GC flags | mut flags|  type    |   0xF1   |
+------------------------------------+----------+----------+----------+----------+
 length : elements / chars / bytes / limbs / words, by type (up to 2^40)
 type   : u8 type code (table below)
 mut    : bit16 IMMUTABLE (literal), bits 17-19 type-specific; written only at initialisation
 GC     : bits 20-23 reserved for the collector (verifier SEEN bit in debug); the mutator never writes them
```

Mark, pin and field-log state live in **side metadata** (§4), not in headers: pairs have no header, and keeping GC
bits out of mutator-written words is threads-rec item 12. Identity hashing needs no header bits (pin-on-hash, §6.9).

**Closure word 0** is `code_descriptor_addr | 0xF9`; descriptors are 256-byte aligned in the non-moving code space.
The JIT reaches the entry with the tag folded into the displacement:
`ldur x10,[x0,#-5]; ldr x11,[x10,#(ENTRY−0xF9)]`. Closure size = 8 + 8·nfree, with `nfree` read from the
(non-moving) descriptor. A 1-free-variable closure is 16 B, not 32 B as a separately headered closure would be.

**Layouts** (words; sizes rounded to 16 B):

| Kind | Tag | Layout | Bytes | Pointer words | Notes |
|---|---|---|---|---|---|
| pair | 011 | car, cdr | 16 | both | headerless |
| closure | 101 | `desc\|0xF9`, fv₀…fvₙ₋₁ | 8+8n | fv | primitives, parameters and continuation procedures are closures over special descriptors |
| vector | 100 | hdr(VECTOR,n), e₀…eₙ₋₁ | 8+8n | elements | |
| string | 100 | hdr(STRING,n), n × u32 | 8+4n | none | UTF-32 inline, O(1) `string-set!` |
| bytevector | 100 | hdr(BYTEVECTOR,n), bytes | 8+n | none | large ones in LOS for FFI |
| flonum box | 100 | hdr(FLONUM), f64 | 16 | none | only for non-immediate classes |
| bignum | 100 | hdr(BIGNUM,limbs; sign flag), u64 limbs | 8+8L | none | inline limbs, no `num-bigint` payload |
| ratnum / compnum | 100 | hdr, num, den / re, im | 24→32 | 2 | |
| cell (box) | 100 | hdr(CELL), value | 16 | 1 | 72 B today |
| global cell | 100 | hdr(GCELL; WATCHED/CONST flags), value | 16 | 1 | allocated old and pinned (§8) |
| record | 100 | hdr(RECORD,n), rtd, f₀…fₙ₋₁ | 16+8n | rtd, fields | inline; RTD canonical |
| RTD | 100 | hdr(RTD), name, parent, field-names, uid, flags | 48 | 4 | allocated old |
| symbol | 100 | hdr(SYMBOL), name string, hash (raw u64) | 24→32 | 1 | interned, allocated old, pinned |
| identifier | 100 | hdr(IDENT), symbol, `scope_set_id:u32 \| source_id:u32` | 24→32 | 1 | Drop-free; scope sets interned [libload §6] |
| ephemeron | 100 | hdr(EPHEMERON; BROKEN flag), key, value | 24→32 | 2 (weak key) | nursery-only allocation (§6.6) |
| values | 100 | hdr(VALUES,n), v… | 8+8n | all | |
| promise | 100 | hdr(PROMISE), box | 16 | 1 | box = hdr(PBOX; done flag), value: SRFI 45 sharing without `Rc` |
| continuation | 100 | hdr(CONT,words), meta, frames…, registers…, dynamic state… | variable | custom tracer | §10 |
| port | 100 | hdr(PORT), `port_id:u32 \| flags` | 16 | none | port table + finalization registry (§6.7) |
| code descriptor | 100 | hdr(CODE), payload_id/arity, nregs/nfree, constants (Word), jit_entry, maps*, instrs* | 256-B slot | constants | non-moving code space |
| macro / library / env-spec / TW payload | 100 | hdr(kind), payload_id, [literal vector] | 16–32 | literal vector | Rust data in the host-payload table |

**Drop policy.** No object in the GC heap owns a Rust `Drop` value. Residual Rust-owned data (code instructions and
maps, JIT machine code, `CompiledMacro`, `Library`, Rust `Environment`s, port buffers, tree-walker CPS payloads, FFI
objects) lives in a per-heap **host-payload table** keyed by a `u32` id stored in the object. Each entry is registered
on a *young* or *old* list; minors examine only young entries, majors all (OCaml custom table, Wasmtime per-semispace
externref list [finalization §4.1]). Payload kinds that hold `Word`s expose them through the slot visitor (updatable)
or report them as pinning roots. Objects carrying payloads are allocated through the Rust slow path (never inline JIT)
so registration cannot be skipped [finalization §6.7]. This deletes sweep tombstoning, which today drops 19 payload
kinds and 47% of allocations' Drop glue [DIGEST §1.3].

One declarative **layout spec** (a `layout!` macro in `patina-gc`) generates, per type: size, pointer-word mask,
trace/update, verifier checks, debug printer, and the JIT field-offset constants (Chez `mkgc.ss` [racket-larceny
§5.8]). Misfiling a value-bearing field becomes a spec diff, not a silent use-after-free.

---

## 4. Heap organisation

**Per-heap reservation.** Each heap reserves one VA range (default 16 GiB data + 1 GiB side metadata + regions for
the remembered set and SATB buffers), committed lazily, 4 MiB-aligned by over-reserve-and-trim. 318 heaps ×
~18 GiB ≈ 5.6 TiB VA; 64 TiB of reservations measured fine on the dev machine [demographics §8.2]. Dropping the
heap unmaps it (requires breaking today's `Rc` teardown cycle, Stage 2).

**Spaces** (all inside the reservation, all sharing one block pool and one side table):

| Space | Contents | Moves? | Collected by |
|---|---|---|---|
| Nursery chunks | all small mutator allocation | evacuated, or promoted in place | minor |
| Old mark-region blocks | promoted objects, direct-old allocations | no (opt. evacuation in Stage 9) | major (lazy sweep) |
| LOS | objects > 8 KiB; buffers lent to FFI | never | major (eager page-run free) |
| Code space | code descriptors (256-B slots) | never | major only (code liveness) |
| Pinned-old | symbols, global cells, RTDs of builtins, canonical flonum boxes, standard ports | never | major |
| Immortal (Stage 9) | boot image after bootstrap freeze | never | not marked; dirty slots are major roots |

There is no separate pointer-free space: the tracer skips pointer-free types by header, and the barrier never fires on
them because their log bits are never armed.

**Sizes, from Patina's demographics:**

| Parameter | Value | Basis |
|---|---|---|
| Granule | 16 B | 56% of objects are exactly 16 B; mean 26.9 B [demographics §4.3] |
| Block | 64 KiB = 4,096 granules | Whippet nofl; one block's metadata is 4 KiB, swept in ~1–2 µs [I] |
| Nursery chunk (TLAB) | 256 KiB = 4 blocks | ≥ 30× the largest nursery object |
| Nursery default | 4 MiB = 16 chunks (adaptive 1–32 MiB) | 64 K–256 K-object interval ≈ 1.5–6 MB; bigger helps ≤1.5× [demographics §9.1]; OCaml 2 MiB, Chez/Racket CS 8 MiB |
| Medium-object threshold | 256 B | larger survivors go to an overflow block, not into small holes (Immix) |
| LOS threshold | 8 KiB | only 148 of 348 M objects exceed it [demographics §4.3] |
| Remembered-set soft limit | 64 K entries (512 KiB) per mutator; 8 M reserved | bounds the minor remset phase (§6.11) |
| Register-stack scan budget | 32 K slots (256 KiB) | bounds the minor stack phase (§9) |

**Side metadata: one byte per granule,** at `meta_bias + (addr >> 4)` (6.25% of *committed* memory):

```
 bit  7     6     5  4     3       2     1  0
    +-----+-----+--------+-------+-----+-------+
    |LOG1 |LOG0 |  kind  |PINNED | END | MARK  |
    +-----+-----+--------+-------+-----+-------+
 MARK   00 = free/unallocated; 01/10/11 = rotating mark colours (start granule only)
 END    last granule of an object (lazy sweep reads only metadata)
 PINNED object must not move (start granule); also set on young objects (nursery metadata is writable)
 kind   reserved: kind cache for the verifier and a possible MMTk binding (headerless pair/closure)
 LOG0/1 field-log "armed" bit for word 0 / word 1 of the granule (1 = first store this cycle must be logged)
```

Nursery metadata is never written by allocation (it reads as zero: unarmed, unmarked), so allocation stays a pure bump.
A **block table** (one byte per 64 KiB block: FREE / NURSERY(copy|in-place) / OLD / OLD_UNSWEPT / LOS / CODE / PINNED /
IMMORTAL, plus HAS_PINNED) answers "is this address young?" for the collector with one load; the mutator never asks
it.

**Decommit.** After each major, empty blocks beyond the free reserve (+25% hysteresis) get `MADV_FREE` (measured to
drop RSS at once [demographics §8.2]); nursery chunks above the adaptive size are decommitted when the nursery
shrinks; LOS runs are returned when freed. This fixes "arenas never shrink" (464 of 501 MiB retained after a library
load is empty capacity [libload §0.5]).

---

## 5. Allocation

**The contract (kept and tightened):** *allocation never collects.* An allocation slow path may refill, overflow,
raise an event, or fail hard; it never moves an object and never runs Scheme. Rationale: it is what lets ~1,300
primitive/front-end sites keep raw `Word`s [DIGEST resolution 2], what lets JIT allocation sites keep half-built
structures in SSA without stack maps [jit-ready R7], and what Chez proves compatible with a moving nursery.

**Interpreter / Rust fast path** (`Mutator` fields are `Cell`s, so allocation takes `&Cx`):

```rust
#[inline(always)]
fn alloc_small(&self, bytes: usize /* multiple of 16, ≤ 256 */) -> NonNull<Word> {
    let m = self.mutator();
    let p = m.alloc_ptr.get();
    let np = p + bytes;
    if np <= m.alloc_end.get() { m.alloc_ptr.set(np); unsafe { NonNull::new_unchecked(p as *mut Word) } }
    else { self.alloc_slow(bytes, AllocKind::Small) }   // #[cold], never collects
}
pub fn cons(&self, a: Word, d: Word) -> Word {
    let p = self.alloc_small(16);
    unsafe { p.as_ptr().write(a); p.as_ptr().add(1).write(d); }   // initializing stores: no barrier
    Word::from_addr(p.as_ptr() as usize | TAG_PAIR)
}
```

Rust compares against `alloc_end` (the real chunk end), never against `alloc_limit`, so a pending event never slows
Rust allocation.

**JIT fast path** (aarch64, `x21` = pinned `Mutator*`; `cons x1 x2`):

```
ldr   x9,  [x21, #ALLOC_PTR]
ldr   x10, [x21, #ALLOC_LIMIT]     ; = alloc_end, or 0 while an event is pending
add   x11, x9, #16
cmp   x11, x10
b.hi  .Lalloc_slow                 ; cold block
str   x11, [x21, #ALLOC_PTR]
stp   x1, x2, [x9]                 ; initializing stores, no barrier
orr   x0, x9, #3                   ; TAG_PAIR
```

Seven instructions plus the stores. A cell is the same with `stp hdr, v`; a closure stores `desc|0xF9` then its free
variables. The JIT combines a basic block's allocations into one bump (OCaml comballoc).

**Slow-path ladder** (`rt_alloc_slow(ctx, bytes, kind, site_flags)`):

1. Current TLAB chunk exhausted → take the next chunk from this mutator's nursery budget.
2. Budget exhausted → raise `EV_GC_MINOR`; grant an **overflow chunk** (the nursery may exceed its budget until the
   next poll services the event; under a `NoGcScope` this is how deferred regions keep allocating young). Cap:
   256 MiB of overflow.
3. Overflow cap reached (only a pathological deferred region) → allocate **direct-old** (bump into old holes, writing
   MARK = current colour and END), and **pre-log** the object on the remember-whole list (§7).
4. Old space beyond the soft heap maximum → raise `EV_GC_MAJOR | EV_HEAP_PRESSURE`; continue.
5. Reservation exhausted → release the 64 MiB **emergency reserve**, raise `EV_HEAP_EXHAUSTED`. The next poll runs a
   full collection; if the heap is still over its maximum it raises a catchable Scheme error
   (`heap-exhausted`, an `error-object`). If the emergency reserve also runs out before a poll, abort the process with
   a diagnostic (never unwind through Rust or JIT frames).

**Large objects** (> 8 KiB) go straight to the LOS (`alloc_large`), pre-logged if they may hold references.
**Payload-carrying and weak kinds** (ports, ephemerons, code, macros) allocate only through Rust paths that register
them.

**Poll folding (JIT only).** A JIT allocation site flagged `AT_SAFEPOINT` (all live values already in the register
file, e.g. the first allocation after a fragment entry) may service events in its slow path, so an allocating
function needs no separate entry poll. That is the one place OCaml's "allocation limit doubles as poll" survives.
Rust allocation and non-flagged JIT sites never service events.

---

## 6. Collection

### 6.1 Minor collection

Runs only at a poll (`EV_GC_MINOR`, `EV_REMSET_FULL`, `EV_STACK_BUDGET`), stop-the-world, single-threaded.

1. **Retire TLABs** (record each chunk's used end).
2. **Choose chunk modes.** A chunk is IN_PLACE if its block table says HAS_PINNED, or if the collector is in
   high-survival mode (previous minor's surviving bytes > 40%; leave the mode when an in-place minor measures < 20%).
   Otherwise COPY. If free blocks are below the nursery size (not enough to-space), all chunks go IN_PLACE — promotion
   can never fail.
3. **Pinning roots first** (tree-walker style by-value roots, FFI-lent objects, identity-hashed young objects): set
   PINNED and the chunk's HAS_PINNED, forcing IN_PLACE.
4. **Roots** (only the young-relevant parts): each green thread's register stack from its return watermark to its top
   (§9) with per-pc liveness maps, its frames' closure words, its small dynamic-state arrays; the mutator's root stack
   and the embedder handle table; the remembered set (slot entries and range entries); the remember-whole list
   (direct-old objects since the last minor); young host-payload registrations' slots; machine-wide mutable roots
   (`pending_escape`, scratch arguments).
5. **Process a young referent:** COPY chunk → if forwarded, update the slot; else copy into old space (bump into the
   promoter's current hole; > 256 B into an overflow block), set MARK = current colour and END, arm LOG bits for its
   pointer words, write the forwarding word, update the slot, push the copy on the grey stack. IN_PLACE chunk → if
   MARK = 0, set MARK/END, arm LOG bits, push. Promotion is on first survival (no ageing): "dead-old" is 65–100% and
   ageing would not change it [demographics §5.4].
6. **Drain the grey stack** with the generated tracer (depth-first, which keeps parents near children like OCaml's
   todo list). Bounded by survivors ≤ nursery size.
7. **Young epilogue** (order of §6.10, young lists only).
8. **Re-arm** every logged slot (set its LOG bit), reset the remembered-set cursor, arm and clear the remember-whole
   list.
9. COPY chunks return to the free pool (debug: poisoned with `POISON` and `PROT_NONE`); IN_PLACE chunks become OLD
   blocks with their holes already known (dead granules have MARK = 0).
10. Reset each thread that ran: `ret_wm = top`, `stack_limit = min(commit_end, top + BUDGET)`. Update survival, nursery
    size and the major trigger.

### 6.2 Major collection (v1: stop-the-world)

1. Run a minor first, so every object is old.
2. Finish sweeping any `OLD_UNSWEPT` blocks (metadata only), then rotate the current mark colour. Three colours plus
   "free" let marks persist across cycles with no clearing pass (OCaml 5 relabelling, Whippet MARK_0..2).
3. Mark from all roots: every register stack in full, root stack, handles, VM machine roots, code descriptors of
   running frames, Rust name tables (by-value pinned roots: their targets are pinned-old cells), tree-walker roots,
   immortal dirty slots (Stage 9). Marking a code descriptor marks its slot in the code space; tracing a closure or
   frame marks its descriptor.
4. Mark loop (§6.3). Count live bytes per block (input to Stage 9 defragmentation).
5. Weak fixpoint and epilogue (§6.6, §6.10). Release dead code.
6. Mark old blocks `OLD_UNSWEPT` (lazy sweep, §6.5). Sweep the LOS and code space eagerly (small).
7. Update the pacing model (§13) and decommit.

`(gc)` is always a full major: the ephemeron tests require a dead key of any age to break after one `(gc)`
[finalization §4.6]. A major is also forced after 256 consecutive minors, which bounds code-release, finalization and
ephemeron-breaking latency (Racket BC forces after 1000 [racket-larceny §5.9]; Larceny's `ephemeron` suite needs a
major within ~100 M pair allocations = ~400 minors at 4 MiB).

### 6.3 Marking order and the mark-stack bound

- Pairs: push the car if it is an unmarked reference, then *continue with the cdr in the same loop iteration*.
  cdr-linked lists need O(1) stack. Today's car-then-cdr order costs +55 MB on a 2 M-element list [DIGEST pitfall 19].
- Vectors, records and continuations: push `(object, next_index)` entries and process 128 words per pop, so one
  huge vector never floods the stack.
- The mark stack is a chain of 4,096-entry segments allocated outside the heap budget and freed after the cycle.
  Typical depth is the car-nesting depth; the pathological bound is O(live objects), the same as any explicit-stack
  marker.
- Marking writes only the side metadata byte. The heap pages are read, never dirtied, so a post-load major on a large
  clean heap does not fault in copy-on-write pages.

### 6.4 Evacuation and pinning policy

- **Young:** evacuated unless the chunk is IN_PLACE (pinned, high survival, or to-space short).
- **Old:** non-moving through Stage 8. Stage 9 adds Immix-style opportunistic evacuation at majors: blocks whose last
  measured live fraction is below 50%, containing no PINNED object and not targeted by a by-value root, are evacuated
  into a reserve of 2–5% of the heap until fragmentation (free granules in partially used blocks ÷ old bytes) falls
  under 10% (Whippet: start compacting above 10%, stop under 5% [whippet §1.4]).
- **Pinned:** objects reached by pinning roots, identity-hashed objects, symbols, global cells, code descriptors and
  LOS objects.

### 6.5 Sweep

Lazy and metadata-only. The promoter or the direct-old allocator takes the next `OLD_UNSWEPT` block and scans its
4,096 metadata bytes. Any start granule whose MARK is neither 0 nor the current colour is dead: clear its granules
through END, *including their LOG bits* (recycled memory must start unarmed). Collect the holes, then either return
the empty block to the pool or put it on the recyclable list. Sweep cost therefore lands on promotion inside minors,
at ~1–2 µs per block, never in the major pause. Dead objects are never read.

### 6.6 Weak references and ephemerons

- **Key-indexed pending resolution** (Whippet `gc-ephemeron.c` [whippet §1.9]). An ephemeron whose key is not yet
  marked or forwarded goes into a pending table keyed by the key's address. While that table is non-empty, every newly
  marked or copied object probes it; a hit traces the value and resolves the ephemeron. No round rescans: the
  16 K-chain that costs 289 ms today [finalization §0.7] becomes linear.
- **Generational rule: an ephemeron is never older than its key or value.** Ephemerons are always allocated in the
  nursery (never pretenured, never direct-old: their allocation path refuses the overflow ladder's step 3 and keeps
  growing the nursery), and promotion is age-monotone (first survival). So a minor examines only young ephemerons,
  treats an old or immediate key as live, and needs no remembered-set edges for ephemerons [finalization §4.6].
- **One fixpoint** covers ephemerons, host-payload tracing (a payload's slots are traced only once its handle object
  is marked) and, later, guardians. Commit `1d18c49` showed that sequencing them separately is a use-after-free.
- **Weak-key tables** (`WeakKeyTable<V>`, registered with the heap, with young and old entry lists) serve reader
  spans for pairs, SRFI 125 weak tables and a future weak symbol table. After a minor, young-key entries are rekeyed
  if forwarded and dropped if dead; old-key entries are processed at majors only.
- `reference-barrier` and SRFI 254 `ephemeron-ref` keys stay opaque uses in JIT code. Precise per-pc maps stay
  mandatory: the #423 tests at `ephemerons.rs:94,109,127,162` forbid conservative frame scanning [V].

### 6.7 Finalization and ports (R1–R6)

- **Port table:** a port is `{hdr, port_id}`. `PortData` (writers, readers, buffers) lives in a per-heap `PortTable`
  whose entries sit on young or old registration lists. A minor finalizes young entries whose object was neither
  forwarded nor marked; a major finalizes unmarked old entries. Finalizing = flush (errors ignored), close, free the
  slot. This meets **R2** (before Scheme resumes) and **R4** (Rust only: no allocation, no Scheme).
- **R1:** `end_process` flushes every live `PortTable` entry, garbage or not, keeping #346's error reporting.
  **R5:** dropping the heap finalizes all entries. **R6:** `current-*-port` parameters hold the canonical port
  object; standard ports are pinned-old singletons.
- **R3:** each file port charges 8 KiB of external bytes to the trigger, and opening min(128, RLIMIT_NOFILE/4) file
  ports since the last major raises `EV_GC_MAJOR`. On `EMFILE`/`ENFILE`, the open primitive (resumable) returns a new
  `Step::Collect { state }`. The machine services a full collection at its stub-frame safe point and resumes the
  primitive, which retries once (chibi `eval.c:1300-1323` [finalization §4.2]).
- GC-time flushing is observable, so port-finalization tests run outside the byte-identical differential lane.

### 6.8 Code liveness

Code descriptors live in the code space and are **marked, not counted**. The edges are frame→code, closure word 0→code,
continuation frame→code, and code→constants (which holds nested lambdas' descriptors). Only a major decides that code
is dead: it frees the descriptor slot and its host payload (instructions, maps, JIT code). This deletes
`live_closures`, `gc_freed_closure_code_ids` and the `RETIRED` sentinel [cont-repr §4]. Freeing machine code at a
safepoint is safe because under the S1 frame model no JIT native frame survives a safepoint (§8). The forced major
every 256 minors keeps release bounded, as `finished_forms_release_code.rs` requires.

### 6.9 Identity hash

**Pin-on-hash.** `identity-hash(x)`: immediates hash their bits; symbols return their stored name hash; any other
object gets `PINNED` set (in nursery or old metadata, plus HAS_PINNED on its chunk) and returns
`mix(addr ^ heap_salt)`. A pinned young object's chunk is promoted in place at the next minor, so the address never
changes. Cost: a handful of in-place chunks in eq-table workloads (eqtable makes 1.66 M calls on 500 K pair keys
[demographics §7.1]); the Stage 9 defragmenter skips pinned objects. I prefer this over Lilliput's hashed/moved header
bits because pairs and closures have no header, hashing is rare (the Larceny-derived workloads never call it), and one
mechanism covers all kinds. SRFI 254 transport cells are unnecessary.

### 6.10 Epilogue order (minor: young lists only; major: all)

1. Trace (copy or mark) to completion.
2. One fixpoint: newly reached host payloads → ephemeron resolution → (later) guardians.
3. Break the remaining pending ephemerons.
4. Weak-key tables: rekey forwarded keys, remove dead ones.
5. Host-payload table: promote survivors' registrations, drop the dead.
6. Finalization registry: run Rust finalizers (ports, `MemoryFs` commit).
7. Release code (majors only).
8. Re-arm the remembered set and reset watermarks; resume. Guardian queues (later) become visible to Scheme.

### 6.11 Pause budget (why "bounded by construction" holds for minors)

| Phase | Bound | Expected [I] |
|---|---|---|
| Minor copy / in-place mark | survivors ≤ nursery (4 MiB ≈ 156 K objects) | ≤1.5% survival on 10/20 workloads → 15–40 µs; nboyer 37% → ~0.4 ms; queue3/deeprec 100% → in-place ~0.5–1 ms |
| Minor remembered set | ≤ 64 K entries (soft limit raises a minor) | ≤ 0.1–0.2 ms |
| Minor stack | ≤ 32 K slots per thread that ran (scan budget) | ≤ 50 µs |
| Minor epilogue | ≤ young registrations (bounded by nursery) | µs |
| Major mark (STW) | O(live) | 3–6 ns/object → 5–15 ms at the measured 50 MB maximum (queue3, mperm); post-library-load live ~10–15 MB → 2–5 ms (today 178 ms) |
| Major roots | O(stack depth) | deeprec's 11.5 M registers ≈ 10 ms; majors are rare there |
| Sweep | 0 in pause (lazy) | today: sweep is most of queue3's 41 ms and all of libload's 176 ms [demographics §3] |

So after Stage 7 every pause is at most a few milliseconds except STW majors, which are proportional to live data
only (not to dead objects, high-water marks, session length or loaded code). Stage 10 makes majors bounded too.

---

## 7. Write barrier

**Kind:** pre-write **field logging on armed side-metadata bits**, LXR polarity (1 = armed; fresh memory 0)
[barrier-res §7]. **No dynamic value filter on the fast path.** The bit is tested first, and the slow path logs
whatever the value is, once per field per cycle. Consequences: (a) stores into young holders cost one byte load and
an untaken branch (nursery metadata is zero); (b) an immediate stored into an old field logs the slot once per cycle
— bounded by distinct fields, harmless; (c) the same fast path is **sound for SATB**, because SATB must log the *old*
value even when the new one is an immediate. Leaving the filter off costs a few extra slow-path entries per cycle on
fixnum-into-old-vector workloads (vecsort: up to one per element per cycle) and buys a fast path that never changes
when incremental marking arrives.

**Fast path, aarch64.** Static parity (fixed field offset: car = LOG0, cdr = LOG1, cell value = LOG1, record field *i*
= LOG0 + (i & 1)); `x2` = slot address, `x9` = hoisted `meta_bias`:

```
lsr   x10, x2, #4
ldrb  w11, [x9, x10]
tbnz  w11, #7, .Lbarrier_slow      ; LOG1 for word 1 of the granule
.Lstore:
str   x1, [x2]
```

**3 instructions.** Dynamic parity (`vector-set!`; slot = `v + i_tagged + 4`, since a fixnum index already equals 8i):
`lsr; ldrb; ubfx w12,w2,#3,#1; add w12,w12,#6; lsr w11,w11,w12; tbnz w11,#0` = **6**.

**Slow path: inline, cold block, no call** (a Cranelift call would be a safepoint and force spills under tier-2 stack
maps [barrier-res §6]):

```
.Lbarrier_slow:
  and   w11, w11, #0x7f            ; disarm (bit 7 here; the bit is per site)
  strb  w11, [x9, x10]
  ldr   x12, [x21, #REMSET_CUR]
  str   x2,  [x12], #8             ; log the slot address
  str   x12, [x21, #REMSET_CUR]
  ldrb  w13, [x21, #BARRIER_MODE]
  cbnz  w13, .Lsatb_arm            ; Stage 10 only: push old value [x2] to the SATB buffer
  ldr   x13, [x21, #REMSET_SOFT]
  cmp   x12, x13
  b.lo  .Lstore
  ; soft limit: request a minor (same sequence as every event request)
  ldr   w14, [x21, #EVENT]; orr w14, w14, #EV_REMSET_FULL; str w14, [x21, #EVENT]
  str   xzr, [x21, #FUEL]; str xzr, [x21, #ALLOC_LIMIT]
  b     .Lstore
```

The slow path never allocates, never collects and never runs Scheme. Between two polls only straight-line code runs
(every loop passes a poll), so the 8 M-entry reservation behind the 64 K soft limit cannot overflow from JIT code.
Rust bulk operations log range entries, and Rust's `write_slow` also checks the hard end and commits more.

**Arming:** promotion (copy or in place) arms the pointer words of each survivor. Direct-old and LOS allocations start
*unarmed* and go on the **remember-whole list**, scanned and then armed at the next minor (G1's
`on_slowpath_allocation_exit` compensation [hotspot §3.3]). Sweep clears LOG bits of freed granules. A minor re-arms
every slot it processed.

**Elision rules:**
- Initializing stores into an object allocated with no safepoint since (constructors take their contents:
  `cons`, `alloc_closure(desc, &fvs)`, `make_vector(n, fill)`, record constructors). In the JIT: same basic block, no
  intervening call, poll or slow path (JEP 475).
- Stores into the register file, frames and dynamic-state arrays (roots); strings and bytevectors (no references).
- Static immediates: only when `attrs.elide_immediate_stores` (true for generational-only configs, false once a heap
  is configured SATB-capable).
- Unboxing write-once `letrec*` defines (compiler item jit-ready R13) removes most `WriteCell` sites outright.

**The store funnel (Rust), the only way to mutate a heap object:**

```rust
impl Cx<'_> {
    pub fn set_car(&mut self, p: Pair, v: Word);
    pub fn set_cdr(&mut self, p: Pair, v: Word);
    pub fn vector_set(&mut self, vec: Vector, i: usize, v: Word) -> Result<(), RangeError>;
    pub fn record_set(&mut self, r: Record, i: usize, v: Word);
    pub fn cell_set(&mut self, c: Cell, v: Word);          // boxes and global cells
    pub fn write_range(&mut self, obj: Obj, start: usize, src: impl ExactSizeIterator<Item = Word>); // fill/copy: one range entry
    pub fn ephemeron_break(&mut self, e: Ephemeron);       // GC-internal, no barrier
}
#[inline(always)]
unsafe fn write_slot(m: &Mutator, slot: *mut Word, v: Word) {
    let meta = m.meta_bias.add(slot as usize >> 4);
    let bit = 6 + ((slot as usize >> 3) & 1) as u8;
    if (*meta >> bit) & 1 != 0 { write_slow(m, slot, meta, bit) }   // #[cold]: same semantics as the JIT slow path
    slot.write(v);
}
```

`Heap::vector_slice_mut` (one non-test caller, `vm_state.rs:2380` [V]) is deleted. Records, parameters, promises and
cells stop being `Rc<RefCell>` payloads, so the "stores through cloned `Rc` handles" channel disappears
[DIGEST §1.5]. Environment and global stores become cell stores through the funnel (§8).

---

## 8. Roots and rooting

**VM frames.** `CallFrame` becomes `#[repr(C)] Copy`, 32 B: `code: CodeRef` (raw pointer to a non-moving descriptor),
`pc: u32`, `base: u32` (slot index), `nregs: u16`, `ret: u16`, `closure: Word`. Per-pc maps (#423) shrink to
safepoint pcs only (call-return points and entries), as a sorted pc table plus bitmaps hanging off the descriptor
(today ~40 B/PC = 65% of the instruction stream [libload §2.2]). The GC scans a window with its frame's map and
overwrites dead slots with `UNSPECIFIED` (today's `retire_registers` rule, so a stale young pointer can never be
resurrected later). The register stack is a per-green-thread reservation (256 MiB VA, grown by copying *at a
safepoint* up to 8 GiB). Frames address windows by index and JIT code reloads `stack_base` after every safepoint, so
relocating then is safe and the 10 M-deep recursion that works today keeps working.

**JIT frames.**
- **Tier 1 (baseline, S1 fragments + `tail` calling convention):** Scheme values live in the register file at every
  safepoint; fragments may cache registers in SSA between safepoints and flush before any runtime call; **no Cranelift
  stack maps**. A tier-1 frame *is* a VM frame: same `CallFrame`, same maps [cranelift-gc §6.1]. Safepoints are fragment
  entry polls, may-GC runtime calls and `AT_SAFEPOINT` allocation slow paths. No derived pointers survive a
  safepoint; machine code embeds no heap addresses (constants come from the descriptor's constants vector).
- **Tier 2 (optimizing, later):** Cranelift user stack maps for values held in SSA across calls; frames walked by
  frame pointer, with maps keyed by return address in a per-heap code registry. Moving-safe by spill/reload. Barrier
  slow paths stay inline (no call). Allowed only for code that cannot capture continuations or that carries deopt
  metadata to rebuild VM frames.

**Rust code.** `Cx<'a>` (mutator + per-instance data) replaces `Rc<RefCell<Heap>>`. Reads and allocation take `&Cx`;
mutation takes `&mut Cx`; `collect` needs `&mut Heap`, which only the driver owns (DIGEST resolution 6). Between polls,
Rust holds raw `Word`s freely, including across allocation. Across a re-entry into the machine:

```rust
let scope = cx.root_scope();          // LIFO segment of the mutator's root stack
let h: Rooted<'_> = scope.root(v);    // slot index; the collector updates it in place
cx.run_nested(...)?;                  // may collect
let v = h.get(cx);                    // reload: v may have moved
```

`NoGcScope` (today's `GcDeferGuard`, renamed and narrowed) remains only for the tree-walker's trampolines and for
mid-form macro expansion (expansion point C) until syntax-case brings an `ExpansionContext` root provider with a
literal pool and epoch-checked memos [libload §7]. Inside a `NoGcScope` the nursery overflows (§5) and polls leave
events pending.

**Tree-walker policy (it may lag).** A tree-walker heap runs RegionGen in its **non-moving configuration**
(`young = Off`): Rust allocation bumps into old holes, writing metadata (the Whippet mmc path, about 10 instructions,
acceptable because the tree-walker has no JIT); collections are STW majors with lazy sweep; its Rust structures
(`StepResult`, `CpsContinuation`, `ContEnv`, `CpsExpr` literals, per-call `Rc<Environment>`) are visited **by value as
pinning roots**, which is legal because nothing in that configuration moves. Its `CpsLambda`/`CpsContinuation` payloads
move into the host-payload table, which replaces sweep-time `Drop`. It collects only at its outermost trampoline safe
points as today. It never becomes one of N carriers.

**Embedding API.** `eval_*` returns `OwnedRoot` (index + generation into a handle table the collector updates; `Drop`
frees the entry; Wasmtime `OwnedRooted`, JNI global references), plus a scoped form
`interp.with(|cx| …)` for bulk work. This fixes the measured embedder use-after-free [prim-embed §7.2]. Dropping an
`Interpreter` tears down its heap: no `Environment.heap` / `CompiledMacro.heap` back-references, so no `Rc` cycle.

**Fate of every off-heap holder:**

| Holder (today) | Fate |
|---|---|
| VM register file `Vec<TV>` | per-thread reservation, scanned with maps, watermarked for minors |
| `CallFrame.closure: HeapIndex`, `.code: Rc<CodeObject>` | `closure: Word` (updatable), `code: CodeRef` (non-moving, marked) |
| `CodeObject.constants` in `Rc` | heap vector referenced from the descriptor's traced `constants` slot; allocated direct-old, pre-logged |
| VM environments / globals (`Bindings`, `FORWARDED`, `Owner`) | **global cells, variant R**: each owned binding is a pinned-old `GCELL`; importer slots point at the exporter's cell (#406 by construction); Rust name tables hold cell refs by value (pinned) |
| `Library.exports: HashMap<String, TV>` | name → `GCELL` ref (pinned; by-value root) |
| `CompiledMacro` literals | literal vector in the heap, referenced from the macro's payload by a traced slot |
| VM continuation side tables (`Rc<VmContinuation>`) | **deleted**: continuations are heap objects (§10) |
| `WindRecord.handlers: Rc<[…]>` | heap data inside the continuation/dynamic-state objects |
| Symbol table `HashMap<String, HeapIndex>` | name → symbol ref; symbols pinned-old; one intern function (shardable later) |
| `syntax_sources` (316 MiB) / `SourceMap.locations` / child spans | identifiers carry `source_id` inline; reader spans for pairs move to a heap-registered `WeakKeyTable`; the two unread stores are deleted (−26 MiB peak, −355 MiB churn [libload §5]) |
| Parser/desugarer raw-bits sets and memos | valid within one `NoGcScope`; after syntax-case, keyed by GC epoch |
| `PENDING_ESCAPE`, `pending_escape`, scratch args | machine roots (updatable slots) |
| Tree-walker `Rc` graphs | by-value pinning roots in the non-moving tree-walker heap |
| `ParsedLibrary.body` held across a load | rooted heap list in the registry; collection allowed between body forms |
| Debugger hook storage, tracer snapshots | registered root providers (open registration, not a closed array) |

---

## 9. Safepoints and polling

**The event protocol** (fields of the `Mutator`, §11):
- `fuel: isize` is **the poll word**. Every poll decrements it; ≤ 0 takes the slow path.
- `event: AtomicU32` holds why: `GC_MINOR 1, GC_MAJOR 2, GC_FULL 4 ((gc)), REMSET_FULL 8, STACK_BUDGET 16,
  PREEMPT 32, INTERRUPT 64, DEBUGGER 128, TERMINATE 256, HANDSHAKE 512, HEAP_EXHAUSTED 1024, PROFILE 2048`.
- `request(bits)`: `event |= bits; fuel = 0; alloc_limit = 0`. Remote requesters (timer, other carriers later) use
  relaxed atomic stores. A racing owner decrement can overwrite the zero, but the request is then still seen within one
  quantum, because expiry always reads `event` (Chez's argument [threads-rec item 7]).
- `rt_poll(ctx)`: `ev = event.swap(0)`; services GC (if `collection_allowed()`), preemption, interrupts, debugger;
  refills `fuel = quantum` and `alloc_limit = alloc_end`; re-reads `event` after the reset so no request is lost (OCaml
  `caml_reset_young_limit`).

**Poll placement.** Compiled code has only forward jumps, so every loop passes through a call or tail call [jit-ready
§2.2]. Polls therefore sit at:
- the **interpreter**: the entry of every closure activation (`Call`, `TailCall`, `Apply`, `TailApply`, stub-frame
  thunk calls, nested-loop entry). This replaces today's per-instruction flag load, which costs 1.1–1.4%
  [DIGEST §1.9];
- the **JIT**: fragment entry for procedure entry (which is also the self-tail-call back edge), unless an
  `AT_SAFEPOINT` allocation folds it.

Return points need no poll. The JIT poll is 4 instructions (`ldr x9,[x21,#FUEL]; subs x9,x9,#1;
str x9,[x21,#FUEL]; b.le .Lpoll_slow`); ISMM'15 measured untaken conditional polls at 1.9% geomean [cranelift-gc §4].
Fuel quantum: 2⁴⁰ when no green threads run (the poll never fires spuriously), 10 000 under the green-thread scheduler,
and a fixed small value under `PATINA_DETERMINISTIC` for preemption tests.

**Two more limits fold into the same event:**
- **Stack-scan budget.** `stack_limit = min(commit_end, ret_wm + BUDGET)`. The call-site check the JIT needs anyway
  (`new_top > stack_limit → slow`) grows the commit or, past the budget, requests `STACK_BUDGET` (a minor) and raises
  the limit by another BUDGET. On `Return` into a caller whose base is below `ret_wm`:
  `ret_wm = base; stack_limit = min(commit_end, base + BUDGET)`, two inline stores in a cold arm. Every write into a
  frame below the top (value delivery, `ResumeWindJump`, raise stubs) goes through one `return_into(frame)` helper that
  does this first; reinstating a continuation or appending a delimited one sets `ret_wm` to the lowest restored frame.
  A minor therefore never scans more than BUDGET slots of new or resumed frames per thread, plus restored ones. (V8
  folds interrupts into its stack limit the same way.)
- **Remembered-set soft limit** (§7) and **nursery budget** (§5) raise their events the same way.

**Deterministic mode.** GC triggers depend only on allocated bytes, preemption only on fuel, timers are off. The
byte-identical differential lanes therefore stay byte-identical under every collector configuration.

**Nested Rust loops (what replaces `GcDeferGuard`):**

| Today's site [V] | Replacement |
|---|---|
| `vm_state.rs:1151` every nested dispatch loop | `RootScope` for the boundary values (re-entry argument slices, taken buffers); nested loops collect (Stage-5 Priority 2) |
| `vm_state.rs:343` `with_globals` | moot with cells: no saved `Rc<Environment>`; the remaining state is a rooted slot |
| `library_loader.rs:208` `ParsedLibrary` | body held as a rooted heap list in the registry; collect between body forms (rbtree: 211 → 118 MiB peak measured [libload §0.6]) |
| `desugarer/mod.rs:1841` `desugar_with_imports` | `NoGcScope` mid-form only; collect between forms |
| `cps_eval/mod.rs:219` tree-walker trampoline | unchanged (`NoGcScope`); the tree-walker lags |

**Safe regions.** `enter_safe_region` / `leave_safe_region` wrap blocking I/O (`read-char`, `read-line`, `open`,
`sleep`) and the future FFI. They are no-ops with one mutator and become Chez `Sdeactivate_thread` with N.

---

## 10. Continuations and stacks

**Representation: design A, a traced snapshot object** [cont-repr §3]. A `CONT` object holds: meta (deliver register,
flags `abort_landing`/`exit`, depth bookkeeping for delimited capture, re-entry ids inline), frames inline as `Copy`
records (32 B each), the register slice, and the wind/prompt/handler arrays (later: pointers to deep-bound persistent
heap lists, threads-rec item 9). A continuation *procedure* is a closure whose descriptor is the `CONT_INVOKE` stub and
whose fv₀ is the `CONT` object, so calling it is an ordinary closure call.

**GC interaction.** The object is byte-counted, so 80 K captures at depth 1000 can no longer reach 5.7 GB unseen
[DIGEST §1.9]. It is nursery-allocated when ≤ 8 KiB, otherwise LOS plus pre-logged. Capture copies with
initializing stores and keeps dead-slot clearing (the `deliver_reg` rule that fixed a 296 MB leak). Captured state is
immutable (re-entry copies out), so continuation objects never need a barrier. A custom tracer walks frames through
`frame.code → maps[pc]` (the descriptor cannot move, and it is marked before or with the frame). Both VM weak tables,
`VmContinuationRef`, and the weak-id half of today's fixpoint are deleted.

**JIT interaction.** Under S1 no Scheme value lives in a native frame at a safepoint, so capture is the same VM-level
copy for interpreted and compiled frames. Each JIT function provides resume entries for its return pcs (or tiers down
to the interpreter). Escapes return a new target to the trampoline; nothing unwinds native frames. Cranelift
`stack_switch` is not used (x64-only, one-shot) [cranelift-gc §2.6].

**Multi-shot `call/cc`, `dynamic-wind`, delimited control:** the algorithms are unchanged. Stub frames still carry
travel state; depths are integers; `append_delimited` still relocates three depths; abort keeps its in-place fast path.
The 64-row control-flow matrix (2 extent forms × 2 positions × 16 transfers [V]) and `escape_from_primitive.rs` gate
the change on both backends.

**Prepared for later (not built now):**
- **Darken on reinstatement** (Stage 10): reinstating a continuation while incremental marking is active first marks
  the `CONT` object's contents (OCaml `caml_continuation_use` [ocaml-gambit §2.9]).
- **C′ (freeze above a watermark)**, gated on a capture-at-depth benchmark: it reuses the same `ret_wm` word (one
  `min` on `Return`, folded into the GC and freeze consumers [cont-repr §6 Stage 4]). The toy measured 57 ns vs 5.0 µs
  capture at depth 1000. Owner decision 18.

---

## 11. Pluggability contract

**Principle:** pluggable *contract*, one production collector. Fast paths are data the JIT and interpreter read
(Whippet `gc-attrs.h` [whippet §1.8]); collectors are selected statically; runtime configuration is read only on slow
paths.

```rust
// crate patina-gc (new; patina-core depends on it)
pub struct GcAttrs {                         // const per collector; JIT reads at compile time
    pub inline_alloc: InlineAlloc,           // BumpChunk { ptr_off, limit_off } | OutOfLine
    pub granule: u32, pub max_inline_bytes: u32,          // 16, 256
    pub barrier: BarrierKind,                // None | FieldLog { bias_off, granule_shift: 4, log_bit0: 6 } | Card { .. }
    pub elide_immediate_stores: bool,        // false if the heap may run SATB marking
    pub poll: PollKind,                      // Fuel { fuel_off }
    pub alloc_limit_is_poll: bool,           // permits AT_SAFEPOINT poll folding
    pub stack_watermark: bool,               // Return must maintain ret_wm / stack_limit
    pub can_move: bool, pub can_pin: bool,
}

pub unsafe trait Collector: Sized + 'static {
    const ATTRS: GcAttrs;
    type Config: Default + Clone;            // young: Copying | InPlaceOnly | Off; evacuate_old; incremental; sizes
    fn new(res: HeapReservation, cfg: Self::Config) -> Self;
    fn bind_mutator(&self, m: &mut Mutator);
    fn alloc_slow(&self, m: &Mutator, bytes: usize, kind: AllocKind, site: SiteFlags) -> NonNull<Word>; // never collects
    fn alloc_old(&self, m: &Mutator, bytes: usize, kind: AllocKind) -> NonNull<Word>;                  // pre-logged
    fn alloc_large(&self, m: &Mutator, bytes: usize, kind: AllocKind) -> NonNull<Word>;
    fn write_slow(&self, m: &Mutator, slot: *mut Word, meta: *mut u8, bit: u8);                      // no alloc, no GC
    fn write_range_slow(&self, m: &Mutator, obj: Obj, first: *mut Word, len: usize);
    fn pin(&self, obj: Obj);
    fn collect(&mut self, req: GcRequest, roots: &mut dyn RootSource, weak: &mut WeakRegistry) -> GcReport;
}

pub trait RootSource { fn trace_roots(&mut self, scope: RootScopeKind, v: &mut dyn SlotVisitor); }
pub trait SlotVisitor {
    fn slot(&mut self, s: &mut Word);                        // updatable
    fn slots(&mut self, s: &mut [Word]);
    fn frame(&mut self, f: &mut CallFrame, window: &mut [Word]);   // uses per-pc maps, retires dead slots
    fn pinned(&mut self, v: Word);                           // non-updatable: pins the target for this GC
    fn code(&mut self, c: CodeRef);                          // code liveness
}
```

`dyn` appears only at collection entry, never on a fast path. Selection:
`#[cfg(not(feature = "gc-null"))] pub type ActiveCollector = RegionGen;` and
`#[cfg(feature = "gc-null")] pub type ActiveCollector = NullCollector;`.

**The JIT ABI object: `Mutator` (= VmCtx), `#[repr(C)]`, pinned for the mutator's life, pointer in x21 (aarch64) /
r15 (x64):**

| Off | Field | Type | Written by | Read by |
|---|---|---|---|---|
| 0 | `alloc_ptr` | usize | inline alloc, Rust | inline alloc |
| 8 | `alloc_limit` | usize | slow paths, requesters (0 = event) | JIT inline alloc |
| 16 | `alloc_end` | usize | slow path | Rust alloc |
| 24 | `fuel` | isize | polls, requesters | polls |
| 32 | `event` | AtomicU32 | requesters | poll slow path, barrier soft limit |
| 36 | `barrier_mode` | u8 | collector | barrier **slow** path only (0 generational, 1 SATB marking) |
| 40 | `meta_bias` | *const u8 | collector (constant) | barrier fast path (hoistable) |
| 48 | `remset_cur` | *mut usize | barrier slow path | barrier slow path, minor |
| 56 | `remset_soft` | *mut usize | collector | barrier slow path |
| 64 | `satb_cur` / 72 `satb_soft` | *mut usize | Stage 10 | barrier slow path |
| 80 | `stack_top` | *mut Word | call/return | JIT |
| 88 | `stack_limit` | *mut Word | return slow arm, collector | call check |
| 96 | `ret_wm` | *mut Word | return slow arm, collector | return check |
| 104 | `stack_base` | *mut Word | thread switch, stack growth | JIT (reload after safepoints) |
| 112 | `thread` | *mut GreenThread | scheduler | runtime |
| 120 | `heap` | *const HeapShared | — | runtime |

**Collectors shipped:**

| Collector / config | Selection | Purpose |
|---|---|---|
| **RegionGen, `young = Copying`** (adaptive in-place) | default | production, VM |
| RegionGen, `young = Off` | runtime config | tree-walker heaps; the **non-moving differential oracle**; the "whole-heap" arm of M5 |
| RegionGen, `young = InPlaceOnly` | runtime config | sticky-mark arm of M5; torture of the in-place path |
| RegionGen, **zeal / torture** | `PATINA_GC_ZEAL=minor:N,evacuate-all,poison,protect` | minor every N polls, always COPY, evacuate every old block at majors (Stage 9), poison and `PROT_NONE` from-space |
| RegionGen, collection disabled | `PATINA_GC=0` | the "off" lane, same binary, same barriers |
| **NullCollector (Epsilon)** | cargo `gc-null` | speed-of-light baseline: bump only, `BarrierKind::None`; isolates GC and barrier cost (JEP 318) |

Today's mark-sweep is **not** retained as an oracle. It is bound to the `Vec`-arena representation that Stage 5
deletes; its role (a simple non-moving reference) is filled by `young = Off`, which shares the generated tracer but none
of the moving, remembered-set or watermark machinery.

**How a future collector plugs in:**
- *Incremental major* (Stage 10) is a RegionGen config: `barrier_mode = 1` during cycles, SATB buffers already in the
  ABI, `attrs.elide_immediate_stores = false`.
- *Parallel marking* is RegionGen under a `threaded` feature (marks become `fetch_or` on the metadata byte,
  work-stealing grey deques).
- *MMTk-backed* is a `MmtkCollector: Collector` under `gc-mmtk`. The binding maps `SlotVisitor`→`Slot`, the layout
  spec → `ObjectModel`, the reserved `kind` metadata bits → identifying headerless pairs and closures, and
  `BumpChunk` → MMTk's `BumpPointer`. It stays blocked by MMTk's one-instance-per-process limit [immix-mmtk §10].

**Explicitly excluded:** load or read barriers of any kind; concurrent marking or relocation; conservative scanning of
VM or JIT frames (forbidden by the #423 tests); Java-style finalizers that run Scheme; per-object `Drop` in GC objects;
`dyn` dispatch on any fast path; patching machine code at GC.

---

## 12. Threading readiness

- **Mutator vs GreenThread.** `Mutator` is the carrier (one under M:1): TLAB, fuel/event, remembered set, SATB
  buffer, cached stack registers. `GreenThread` is an execution state: register stack, frames, dynamic environment
  (parameterization, current ports, handler stack — deep-bound heap data), `ret_wm`, and a ran-since-last-GC bit. A
  green-thread switch saves and restores `stack_*`/`ret_wm` in the `Mutator` (O(1)).
- **N-ready now at zero single-thread cost:** per-mutator TLAB and remembered set (no shared log); a block pool behind
  one uncontended lock; metadata bytes where the mutator only *clears* LOG bits (racing clears are idempotent —
  a lost clear means one duplicate log, never a lost one); the store funnel as the single publication point (a release
  fence goes there under `threaded`); a poll protocol written as request → acknowledge → collect → release; safe
  regions; per-thread root providers; continuation objects that reference no carrier state; code store and
  continuation data machine-wide; one intern function; a **two-mutator test mode** alternating two `Mutator`s on one OS
  thread in CI [threads-rec §3.1].
- **Deferred:** OS-thread carriers, `Slot` accesses as relaxed atomics, parallel marking and copying (CAS
  forwarding), handshakes, TSan/loom lanes; the tree-walker as a carrier (never).

---

## 13. Heap sizing, pacing and observability

**Triggers, all in bytes:**
- *Minor:* nursery budget consumed (default 4 MiB per mutator), remembered-set soft limit, stack budget.
- *Major:* `old_allocated_since_major ≥ H − L`, where L = live bytes after the last major and old allocation includes
  promotion, direct-old, LOS and **external bytes** (`charge_external`: 8 KiB per file port, host payload bytes such as
  code instructions and maps). H follows MemBalancer [whippet §1.10]:
  `H = L + clamp(c·√(L·g/s), 8 MiB, 3·L)`, with g = old-allocation rate, s = marking speed, EWMA α = 0.5, and c
  chosen so H ≈ 2L at L = 10 MB on the benchmark set. Also: the open-file-port count (§6.7) and the 256-minor backstop.
- *Initial:* first major at 32 MiB of old bytes. After bootstrap (≈4 MiB live), the post-library-load major becomes the
  first one, not a 178 ms sweep.

**Nursery sizing.**
- Start at 4 MiB; double (up to 32 MiB) when minors exceed 200/s with < 5% survival (fixed per-minor costs dominate);
  halve (down to 1 MiB) when p99 minor pause exceeds 2 ms.
- High survival switches mode (in-place promotion), not size, because a bigger nursery helps ≤ 1.5×
  [demographics §5.5].
- Allocation-site pretenuring is left to the JIT era (sites are natural there).

**Free reserve.** After every major, keep at least max(nursery size, 2% of heap) of empty blocks. If that cannot be
met, grow H. If growth hits the maximum, minors promote in place, which needs no reserve, so Wingo's
fragmentation livelock cannot spin.

**Decommit:** §4. **Hard limit:** §5.

**Observability (Stage 0 onward):**
- `GcStats` per collection: kind, mode, pause, phase times (roots, stack, remembered set, copy/mark, ephemerons, weak,
  finalize), bytes promoted, live bytes, survival, nursery size, H, fragmentation.
- `PATINA_GC_LOG=file` writes one CSV row per collection; `scripts/gc_bench.py` computes p50/p99/max pause and
  **MMU at 1/10/100 ms windows** (Larceny `gc_mmu_log.c` [racket-larceny §4.4]).
- A `(gc-stats)` association list for Scheme; `PATINA_GC_TRACE` JSON events for the debugger hook system;
  `last_pause_micros` finally read.

---

## 14. Testing and verification

- **Heap verifier** (`PATINA_GC_VERIFY=1`, on in debug lanes). After every collection: linear walk of all old blocks
  and the LOS through metadata (start/END consistency, header validity, MARK = current colour); every pointer field
  targets a live object start; no pointer into the nursery after a minor; the **remembered-set invariant** (every
  old→young edge is in the remembered set or its slot is disarmed and logged); all register stacks scanned with maps;
  code descriptors of live frames marked.
- **Torture:** zeal configs above. Copying lanes poison evacuated chunks and map them `PROT_NONE`, so a stale
  reference faults at the access. The in-place lane forces IN_PLACE every other minor. Stage 9 adds an
  evacuate-all-old lane.
- **Miri:** `patina-gc` puts its virtual-memory provider behind a trait with a `Vec`-backed implementation, so the
  allocator, the tracer over a mock layout, the remembered set, ephemeron resolution and the sweeper run under Miri.
- **Differential lanes** (`run_gc_differential.sh`, extended): {`PATINA_GC=0`, default, `young=Off`,
  `young=InPlaceOnly`, zeal} × {release, debug-poison+verify} × VM; tree-walker: {0, default(`young=Off`), zeal-major}.
  Outputs stay byte-identical to the `PATINA_GC=0` run; `EXPECTED_TOTAL` stays asserted. Port finalization tests (E1,
  E2 at `RLIMIT_NOFILE=256` in a subprocess, E4, E5) run outside it.
- **Scoreboards:** control-flow matrix (64 rows, both backends), hygiene matrix (139 shapes — gates the identifier
  representation change), `ephemerons.rs`, `finished_forms_release_code.rs`, Larceny `ephemeron` 6/6, both chibi
  suites, `patina-compat check-smoke`.
- **GC benchmark set:** demographics' 20 workloads [demographics §2]. The Larceny-derived programs are run from
  `~/Project/reference/larceny` like `run_larceny_tests.sh` (LGPL; not vendored); the Patina-authored extras (deeprec,
  libload, eqtable, empty) and new probes (capture-at-depth, ping-pong, abort100, nested-map churn, port churn) are
  vendored. Metrics: wall, GC CPU share, peak RSS, pause p50/p99/max, MMU(1/10/100 ms), allocations, promoted
  bytes. Every perf claim is an interleaved main/branch/main run, ≥5 rounds, on the dev Mac plus the CI Linux lane
  for barrier-sensitive changes.
- **Barrier experiments** (barrier-res §8): M2 (interpreter tax with everything armed), M3 (JIT barrier variants:
  static/dynamic parity, call vs inline slow path — check for tier-2 spills), M5 (generational vs barrier-only vs
  whole-heap) — all as runtime configs of RegionGen plus `gc-null`.
- **How each stage proves itself:** §15's acceptance column. A stage merges only with every existing lane green and its
  new lane added.

---

## 15. Migration plan

Ordered stages, each landable with both backends and every lane green. The new heap is built behind a cargo
feature while the old one ships (the strangler pattern), so Stage 4's many PRs never break `main`. Issues are filed
before every defect fix (the DIGEST §1.11 defects get issues in Stage 0).

| Stage | Scope | Crates / files | Acceptance and measurement (interleaved A/B) | Effort [I] | Standalone value |
|---|---|---|---|---|---|
| **0 Measure** | GC benchmark set + runner; per-collection log with phase times; MMU; issues for DIGEST §1.11 defects and doc drift; `ExecutionState::depth()` for the 37 `frames.len()` sites | `patina-core/src/heap/gc.rs`, `scripts/gc_bench.py`, `bench_programs/gc/` | reproduces demographics §3 within noise; lanes byte-identical; zero cost when logging is off | 1–2 wk | every later claim becomes measurable |
| **1 Quick wins on today's collector** | byte trigger incl. `Vec` payloads and continuation bytes; port pressure + `Step::Collect` EMFILE retry; delete the unread provenance stores and throwaway `SourceMap` | core `heap/`, `primitives/io/ports.rs`, `registry.rs` (`Step`), VM `resume_stub`, TW `ResumePrimitive`, `source_map.rs` | gcold RSS 628 → ≤120 MB; 500×`make-vector 100000` collects; 100 K opens at `ulimit -n 256` pass on both backends; libload peak −26 MiB; geomean wall within ±2% | 2–4 wk | fixes R3, trigger blindness, multi-GB plateaus now |
| **2 Context and rooting (C1, C10)** | `Cx` receivers (codemod, crate by crate); per-instance data off `Heap`; `RootScope` / `Rooted`; `NoGcScope`; nested loops collect; `OwnedRoot` embedder handles; heap teardown (break `Rc` cycles); fuel/event poll at call entry | all crates (~1,300 sites, ~95% mechanical [prim-embed §10]) | embedder-UAF repro fixed; E5 flushes; nested-map churn collects during the map; libload malloc peak ≤ 450 MiB; interpreter geomean ≥1% faster (fewer borrows, no per-instruction poll); no workload > +1% | 6–9 wk | embedding correctness, nested-loop memory bounds, faster interpreter |
| **3 Store funnel, slots, layout spec (C2, C11, C5, C6a)** | funnel API; delete `vector_slice_mut`; records/params/promises/cells as heap data; `SlotVisitor`; `layout!` spec generating trace/size/verify; `heap.view()` for `datum_writer`; port table + registry (R1–R6); canonical ports/RTDs/procedures | core, primitives, VM | M2 interpreter barrier tax (all armed, drained per GC) ≤1%; E4 `eq?` passes; R1–R6 tests; generated tracer = old tracer on all lanes | 5–8 wk | identity fixed (#R6), barrier-ready, exponential-tracer class of bugs closed |
| **4 Heap v2 behind `heap-v2` (C3, C4, C12)** | `patina-gc` crate; reservation, block pool, side metadata, LOS, code space; tagged addresses + headers; RegionGen in `young=Off`; lazy sweep; key-indexed ephemerons; host-payload table; pin-on-hash; `Mutator` ABI struct; per-thread register-stack reservation; MemBalancer; decommit; verifier, zeal (major-only), Miri | new `crates/patina-gc`, `patina-core` heap internals | v2 lane green on all suites and matrices; vs v1: RSS ≤0.6× geomean, allocation-heavy workloads faster, post-load pause 178 ms → ≤20 ms, queue3 max 41 → ≤15 ms; no pause term proportional to dead objects | 10–16 wk | the representation win (2.05× bytes, no sweep pause, memory returned) |
| **5 Flip + flonums** | make v2 default, delete arenas and `HeapObjectData`; self-tagged flonums with canonical boxes | core, numeric primitives, VM inline arithmetic | flip: all lanes; flonums: fibfp/mbrot/nucleic allocations −80%, wall −15% or better; others ±1%; class-coverage report fixes `C_ENC` | 3–5 wk | float code stops allocating; JIT tag ABI frozen |
| **6 Remaining holders (C7, C8, C13)** | continuations as `CONT` objects (design A), `Copy` frames; code space + traced code liveness; global cells variant R; inline `source_id`, weak-key reader spans; identifiers as ids | VM `runtime/`, `types/`; core `environment.rs`, `library.rs`; frontend/macros | matrix 64/64 both backends; ctak capture 2–4× faster, RSS bounded; code-release tests; fib/tak `LoadGlobal` faster; hygiene matrix 139/139; libload peak ≤ 250 MiB | 7–10 wk | cheap byte-counted continuations, JIT-ready globals, every VM holder updatable |
| **7 Generational** | nursery chunks, copying minor + in-place mode, armed-bit barrier live, remembered set and remember-whole list, `ret_wm` + stack budget, generational ephemeron/payload/port/weak-key rules; zeal copy lanes | `patina-gc`, VM call/return paths | **M5 gate:** gen ≥ whole-heap on geomean wall *and* p99 pause ≥2× better; p99 minor ≤2 ms, max minor ≤5 ms on all 20 workloads; differential and zeal lanes green | 6–10 wk | bounded minor pauses, free young garbage, 7-instruction allocation |
| **8 Threads-ready split** | `Mutator`/`GreenThread` split, deep-bound parameterization and ports, safe regions, two-mutator test mode | VM, primitives, `parameters.scm` | matrix + new transfer rows scored against Gambit/chibi/Gauche; ≤1% cost | 3–5 wk | SRFI 18 (M:1) can start |
| **9 Defrag + boot freeze** (gated: fragmentation >15% on any workload, or static data >30% of major time) | opportunistic old evacuation; evacuate-all zeal lane; immortal boot space with dirty-slot roots | `patina-gc` | fragmentation ≤10%; major time on short programs −50% | 4–6 wk | long-running REPL/embedding memory stability |
| **10 Incremental major** (gated, §16 K5) | SATB slow-path arm, mark slices paced by minors, incremental stack scanning via `ret_wm`, darken-on-reinstate, final remark | `patina-gc`, VM return slow arm | p99 major ≤3 ms at 50 MB live; throughput ≤3% geomean; matrix and ephemeron tests under the incremental lane | 8–12 wk | bounded major pauses |

**The first three PRs:**

1. **"GC benchmark set, per-collection log and MMU report."**
   - Changes: `heap/gc.rs` emits a CSV row per collection under `PATINA_GC_LOG` (kind, pause, mark, weak and sweep
     times, slots swept, live slots, allocations since the last collection).
   - New `scripts/gc_bench.py`: runs Larceny programs from the reference checkout plus vendored extras, interleaved
     main/branch/main, and reports wall, `/usr/bin/time -l` RSS, pause percentiles and MMU.
   - New `crates/patina-tests/bench_programs/gc/` (Patina-authored extras only).
   - Acceptance: reproduces demographics §3's table within run-to-run noise; differential lanes byte-identical;
     `PATINA_GC_LOG` unset costs nothing.
   - Value: the measurement baseline every later stage needs (no pause/MMU benchmark exists today).
2. **"Byte-accounted trigger with external-resource pressure"** (issues first: trigger blindness, EMFILE).
   - `note_alloc(bytes)` at every `alloc_*`, using slot size plus payload capacity (vectors, strings, bignums,
     continuation frames and registers). Threshold `max(8 MiB, 2 × live_bytes)`.
   - Ports charge 8 KiB each and count opens.
   - `Step::Collect` lets the open primitives collect and retry once on `EMFILE`, on both backends.
   - Acceptance: the Stage 1 numbers above; E2 test added on both backends (outside the differential lane);
     interleaved A/B geomean within ±2%.
3. **"`Cx` receiver for heap-only primitives."**
   - `HeapHandler` becomes `fn(&mut Cx, &[Word])`; the dispatcher borrows the heap once per call; instance metadata
     (features, command line, fs) moves to `Instance`.
   - Codemod the 280 heap-only primitives. Borrowck flags the 78 borrow-then-allocate functions, which get local
     scoping fixes.
   - Acceptance: every lane; interleaved A/B shows no regression above 1% (expect a small gain on primitive-heavy
     cases).
   - Value: removes ~406 `RefCell` borrow sites and creates the receiver that later carries the barrier and
     allocator.

Total to Stage 7 ≈ 40–64 engineer-weeks [I]. The representation work in Stages 2–6 is ~75% of it and is needed by
every candidate on the panel.

---

## 16. Risks, mitigations and kill criteria

| # | Risk | Mitigation | Kill criterion → change of course |
|---|---|---|---|
| K1 | Generational loses (Wingo's nboyer/splay) | `young=Off` and `InPlaceOnly` are runtime configs of the same collector; M5 runs all three | After Stage 7, if `young=Copying` is slower than `young=Off` by >1% geomean wall **and** does not cut p99 pause ≥2× → ship `young=Off` (with Stage 9 defrag) as the VM default, set `BarrierKind::None` in attrs, keep generational as a config. The design degrades to candidate A/B, losing no earlier stage. |
| K2 | Barrier cost | M2/M3 measured before Stage 7 merges; static elision; `letrec*` unboxing | Interpreter tax >1.5% geomean or JIT >3% on store-heavy workloads → reinstate a dynamic value filter on the fast path (forfeits free SATB; Stage 10 would then need JIT invalidation) or switch to card + young filter (`BarrierKind::Card`, a one-module change) |
| K3 | Fragmentation from in-place promotion and a non-moving old space | free reserve; fragmentation metric in `GcStats` | sustained fragmentation >30%, or RSS >1.5× the `young=Copying`-only run on any workload → pull Stage 9 forward |
| K4 | Minor pause bound violated by an unforeseen root set (big handle tables, tree-walker-like roots, dirty immortals) | phase timings per minor | p99 minor >5 ms on any benchmark → make that root set watermarked or generational before shipping Stage 7 |
| K5 | Incremental marking cost vs need | not built until gated | **Build** Stage 10 only if STW major p99 >25 ms on the benchmark set at default sizing, or the owner sets a lower latency target. **Abandon** it if throughput cost >3% geomean, or if the control-flow matrix or ephemeron tests need more than 2 weeks of fixes in the incremental lane |
| K6 | Stage 4 slips (largest work item) | strangler feature; per-PR v2 lane; common-core work first | >2× its estimate by mid-stage → cut scope: ship v2 with `young=Off` only (already the Stage 4 target) and defer Stage 7 |
| K7 | Self-tagged flonums regress or cover too little | class histogram; canonical boxes | coverage <70% of float values on fibfp/mbrot/nucleic, or fixnum/pair paths >1% slower → keep 16 B boxed flonums (cheap in a bump nursery) and free the three tags for later use |
| K8 | Major-only code release delays reclamation | forced major every 256 minors; `(gc)` = major | `finished_forms_release_code.rs` or a REPL churn probe shows unbounded code growth → add a code-only mark at the backstop |
| K9 | Return-watermark or virtual-depth bugs in control transfers (the area where defects cluster, #157–#163, #176) | matrix rows for watermark paths; `ret_wm` updated in one `return_into(frame)` helper | any matrix row failing under zeal for >1 week → disable the stack budget (`BUDGET = ∞`); minors stay correct, just unbounded on deep stacks |
| K10 | Tree-walker drifts (non-moving config bit-rots) | its own differential and zeal-major lanes in CI | n/a: required lanes |

---

## 17. Owner decisions (DIGEST §6) this design depends on

| # | Decision | My default | Consequence of the alternative |
|---|---|---|---|
| 1 | Tree-walker role | Answered: kept, may lag → runs RegionGen `young=Off`, by-value pinning roots, host-payload table | Making it a full moving citizen costs 6–12 weeks of heap-allocated frames [tree-walker §8] |
| 2 | Rebinding semantics | Variant **R** first (today's follow-the-name), C recommended later and recorded in `DIVERGENCES.tsv` | C: one-load globals, deletes per-site caches and shadow bits; flips six pinned tests |
| 3 | `define` timing / mid-program imports | Keep today's behaviour under R; settle with C | C's expansion-time rebinding matches chibi/Chez/Gauche |
| 4 | Value encoding | 61-bit fixnums, **self-tagged flonums** (3 tags), raw tagged addresses, closure tag | NaN-boxing would cost the fixnum tag-0 trick and 64-bit-clean pointers; keeping boxed flonums costs 19.8% of allocations (K7 is the escape hatch) |
| 5 | Strings / bignums | UTF-32 inline (O(1) `string-set!`); **inline bignum limbs** (Drop-free) | UTF-8 halves string bytes but needs an index for `string-ref`; keeping `num-bigint` payloads forces bignums through the host-payload table |
| 6 | Pause goal | Answered: throughput first, STW acceptable → minors bounded by construction; majors STW until K5 | A latency target <10 ms makes Stage 10 mandatory |
| 7 | Shared-memory parallelism | **Not assumed**: M:1 green threads; N-ready interfaces (§12) | If yes: the `threaded` feature, parallel marking, CAS forwarding — 4–9 months on top [threads-rec §4] |
| 8 | JIT frame model | **S1** fragments + tail calls; tier 2 later and only for non-capturing code | S2 needs native unwinding and a depth cap; S3 everywhere needs deopt for continuations |
| 9 | Async interrupts in JIT code | **Yes**, via the fuel poll at entries | Without them, non-allocating loops could skip polls (saves ≤1.9%) but Ctrl-C and preemption fail in tight loops |
| 10 | Identity hashing | **Pin-on-hash** | Lilliput header bits need headers on pairs/closures or a side table; GC-rehashed native eq-tables mean rewriting SRFI 69/125 in Rust |
| 11 | Weakness scope | SRFI 124 + SRFI 254 ephemerons now; SRFI 125 weak tables over ephemerons; guardians after Stage 7 (one fixpoint slot reserved) | Weak pairs or wills add epilogue kinds with no consumer |
| 12 | Port finalization | Yes: R2/R3 with collect-and-retry; excluded from the differential lane | Without it, descriptor exhaustion persists and Patina diverges from both oracles |
| 13 | Embedding API | Scoped `with` + `OwnedRoot`; mandatory teardown; many heaps per process | Scoped-only blocks long-lived host references; no teardown leaks a VA reservation per dropped interpreter |
| 14 | Dependencies (MMTk) | **Bespoke in-tree**; MMTk-shaped seams; no new crate in `patina-core` beyond `patina-gc` | MMTk: one instance per process (vs 318 heaps), worker threads in an `Rc` world, nondeterministic trace order, headers on pairs [immix-mmtk §10] |
| 15 | Footprint | 16 GiB reservation per heap; MemBalancer H≈2L at 10 MB; decommit on | A smaller reservation lowers the hard cap; fixed multipliers waste memory at large L |
| 16 | Front-end sequencing | Delete unread provenance stores in Stage 1; inline source ids and identifiers-as-ids before Stage 7; lazy scope propagation recommended in parallel (44% of load CPU [libload §0.4]) | Without them, library loading keeps a 300 MiB provenance cost and Drop-carrying identifiers block the nursery |
| 17 | syntax-case timing | After Stage 7: transformer calls become polls over a rooted `ExpansionContext`; minors at transformer entry are cheap | Before Stage 2 it would have to keep whole-form deferral |
| 18 | Continuation end state | **Design A** now; C′ gated on capture-at-depth benchmarks, reusing `ret_wm` | C′ now: O(1) capture, but medium-high risk on the matrix |
| 19 | Divergences | Record each fix or introduction (port `eq?`, rebinding under C) in `DIVERGENCES.tsv` | — |
| 20 | JIT code memory | Freed with its descriptor at majors; W^X toggling confined to one install function | Per-function freeing in `cranelift-jit` is unverified; worst case is an arena per code unit |
| 21 | Effort order | **Representation first** (Stages 2–6), collector second | Collector-first would build a nursery on 72-byte `Drop`-carrying slots, which cannot be copied |
