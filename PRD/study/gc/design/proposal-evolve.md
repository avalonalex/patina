# Proposal EVOLVE: a non-moving mark-region heap first, evacuation and generations as measured switches

Panel philosophy: **evolutionary.** Date 2026-10-01. Repo `main` at `28a94f8`. Inputs: `DIGEST.md` and every
report it cites, referred to by the digest's keys (`demographics`, `barrier-res`, `cont-repr`, …). Tags:
**[V]** verified in source, measurement or primary source (re-checked for this proposal where marked
**[V*]**); **[I]** my design judgement.

Re-checked for this proposal [V*]: `core:tagged_value.rs:76-110`, `core:heap/mod.rs:570-612`,
`vm:types/mod.rs:41-60`, `vm:types/code_object.rs:140-174`, `vm:runtime/execution_state.rs:17-23`,
`core:heap/gc.rs:177-213,485-500`, `vm:runtime/vm_state.rs:1195-1207`, `run_gc_differential.sh:67`,
`control_flow_matrix.rs:118`, and Whippet's `nofl-space.h`, `gc-allocate.h`, `gc-barrier.h`, `mmc-attrs.h`.

---

## 1. Thesis and key bets

The digest's second fact is the whole argument: Patina's costs come from its representation (72 B enum
slots, `Rc` payloads, `Vec` arenas whose bases move, `Rc<RefCell<Heap>>`, a sweep proportional to the
arena high-water mark), not from mark-sweep as an algorithm. The same allocations cost about 2.05× the
bytes of a headered layout, and the 178 ms worst pause is a sweep over 7.5 M dead slots (demographics
§3–§4). So I build the common core (DIGEST §4.0, C1–C14) in an order where **every stage ships a measured
win on its own**. The first production collector is candidate A in its plainest form: a **non-moving
mark-region space** (Whippet nofl-derived: 16 B granules, a side metadata byte per granule, 32 KiB
blocks, bump allocation into holes, lazy metadata-only sweep), with byte-based pacing, continuations as
traced objects and a non-moving cell space for globals. **Sticky-mark generations** and **Immix-style
opportunistic evacuation** (candidate B) are added later *on the same space, the same metadata byte, the
same barrier and the same JIT ABI*, each behind a per-heap switch that measurement turns on. I deviate
from candidate A in three places and say so:

- **No metadata writes at allocation.** Whippet writes begin and end bytes at allocation because it
  supports conservative roots (`gc-allocate.h:31-56`). Patina forbids conservative scanning (#423
  ephemeron tests), so the marker writes the END bit instead. The fast path becomes a pure bump (6
  instructions, 3 with the pointer and limit cached in registers), the same sequence as a copying
  nursery's. Candidate A's cited 8–10-instruction disadvantage on allocation disappears.
- **One log bit per granule, not per word**, plus a tag map in which "might be a heap reference" is
  **one bit**. The heap-value barrier path is 4 instructions on aarch64; the fixnum and flonum path is 1.
- **Closures are headerless** (code-descriptor word, then free variables, as in Chez). Closures are
  35.3% of allocations, and 68% have 1–4 free variables (demographics §4.1–§4.2).

**Key bets.** If any of these is wrong, the design needs to change course; §16 gives each one a measurable
kill criterion.

- **B1 Representation dominates.** Headers, inline payloads, bump allocation, lazy sweep and memory
  return capture most of the available win. What a copying nursery adds on top of a sticky-mark nursery is
  second order on Patina's bimodal survival: 10 of 20 workloads have ≤1.5% survival, but promoted data is
  65–100% "dead-old", and survival rises once the compiler stops making garbage closures (demographics
  §5.3–§5.5). Kill criteria: K4, K9.
- **B2 Allocation never collects.** Collection happens only at polls and at calls into the runtime, with
  no Rust temporaries live. This holds through the JIT, as it does in Chez (chez §3–§4; prim-embed §2).
- **B3 A granule field-logging barrier is cheap enough.** The target is under 1% in the interpreter and at
  most 2% in JIT code. It serves the sticky generations now, stays sound under evacuation, and supports
  incremental marking later. Whippet measured field logging at 1.05–1.5× faster than cards; LXR's version
  costs 1.6% geomean. Kill criterion: K2.
- **B4 Fragmentation can be contained without moving until stage 7.** The tools are a free-block reserve,
  medium-object overflow blocks and a recyclable threshold. Evacuation can be added later without changing
  the object model, the barrier or the ABI. Kill criterion: K3.
- **B5 Precise roots with no Cranelift stack maps in the baseline tier.** At every safepoint, every Scheme
  value is in the VM register stack. That is the S1 model in cranelift-gc §6.1.
- **B6 Generational collection is not assumed.** It is a per-heap switch. The M5 experiment (barrier-res
  §8) picks the default, and the tree-walker heaps stay whole-heap. Kill criterion: K1.

---

## 2. Value encoding

`TaggedValue` stays `#[repr(transparent)] u64` with a 3-bit low tag. Fixnums keep 61 bits and tag `000`,
so a JIT can add or subtract tagged values directly and test overflow (`adds`/`b.vs`, jit-ready R1).
Heap references become **raw tagged addresses**: objects are 16 B aligned, the tag folds into the load
displacement (`car` is `ldur x, [v, #-3]`), and decoding needs no base register (chez §1; rust-gcs §4.6;
hotspot §3.10). Flonums are **self-tagged immediates** (Melançon, Serrano and Feeley, OOPSLA'25, via
heap-repr §11). Each heap still owns one virtual-address reservation (§4), but values carry addresses,
not offsets: fields stay 64-bit because they hold 61-bit fixnums, so a compressed 32-bit offset would buy
only a base add on every access (js-engines L9).

**Final tag map** (low 3 bits):

| Tag | Meaning | Decode | Notes |
|---|---|---|---|
| `000` | fixnum, 61-bit two's complement | `v >> 3` (arithmetic) | unchanged from today |
| `001` | flonum, immediate (band A) | see below | |
| `010` | immediate constant: specials and chars | sub-tag in bits 3–7 | was `001`/`010` |
| `011` | pair (headerless) | address = `v − 3` | unchanged tag |
| `100` | flonum, immediate (band B) | see below | was vector |
| `101` | flonum, immediate (band C) | see below | was string |
| `110` | closure (headerless) | address = `v − 6` | the tag that is dead today, now used |
| `111` | headered object | address = `v − 7` | vectors and strings move here |

**One-bit "may be heap" test.** All three heap tags (`011`, `110`, `111`) have bit 1 set. Fixnums and all
three flonum tags have it clear. The barrier's value filter (§7) is therefore one `tbz v, #1` on aarch64.
Tag `010` (specials and chars) also passes the filter. That false positive is harmless: logging depends on
the *holder's* granule, and storing `#t` into an old cell costs one extra metadata load. barrier-res §4.1
asked for exactly this single-bit test.

**Flonum self-tagging, derived [I, arithmetic checked].**
- The double's bits `D` hold the sign in bit 63 and the 11-bit exponent in bits 62–52.
- Encode: `t = D + 2^60`, then `v = rotl(t, 4)`, which is `ror v, t, #60` on aarch64.
- The low 3 bits of `v` are then `(top3(exponent) + 1) mod 8`. The value is immediate iff `top3 ∈ {0, 3, 4}`,
  which gives tags `001`, `100` and `101`:
  - band A (`001`): ±0.0, subnormals and magnitudes below 2^-768;
  - band B (`100`): magnitudes in [2^-255, 2);
  - band C (`101`): magnitudes in [2, 2^257).
- Decode: `D = rotr(v, 4) − 2^60`.
- Every other band is boxed: `top3 ∈ {1, 2, 5, 6}` would land on the non-flonum tags `010/011/110/111`,
  and NaN, ±∞ and huge values (`top3 = 7`) would land on `000`. The encoder sees the tag and boxes.
- These are the three bands of the Gambit variant reported in heap-repr §11 (its tags `011/110/111`
  correspond to an offset of 3). I use offset 1 so that the flonum tags land on `001/100/101` and leave
  bit 1 free for the heap test.
- Encoding is canonical: a double is immediate whenever it can be, and boxed only when it cannot. So
  `eqv?` on flonums is decode-and-compare, and `eq?` on equal immediates is true.
- Immediate test: `(0x32 >> (v & 7)) & 1`. Encode cost: an add, a rotate, an and, a mask test and a
  branch.
- Flonums are 19.8% of all allocations, 79–99.8% in float code, and survive at ≤0.7% (demographics §9.4).

**Immediates (`010`)**:
- Layout: `payload << 8 | sub << 3 | 0b010`.
- `sub = 0`, specials: `#f = 0x002`, `#t = 0x102`, `() = 0x202`, `eof = 0x302`, `unspecified = 0x402`,
  `default-object = 0x502`. The rest are runtime-only and never handed to Scheme:
  - `UNBOUND = 0x602`, the global-cell placeholder (global-cells §3.1);
  - `BWP = 0x702`, a broken ephemeron;
  - `GC_POISON = 0x802`, debug only;
  - `PAIR_FWD = 0x902`, a debug check word in evacuated pairs.
- `sub = 1`, chars: the code point sits in bits 8–28.
- Truthiness is `cmp v, #2`.
- Today's `FORWARDED = 0xF1` import marker is deleted with global cells (stage 4b).

**Forwarding.** Forwarding state lives in the side metadata byte (state `FORWARDED`, §3), not in object
words. The evacuated object's word 0 holds the new **untagged** address. That works the same way for
headerless pairs, headerless closures and headered objects. In debug builds the evacuated object's word 1
is overwritten with `PAIR_FWD` or `GC_POISON`, so a stale read panics.

**Why.** It keeps what code and a JIT rely on (fixnum tag 0, `#f` as one constant, pair and closure tags
for one-instruction type tests; calls are 10–31% of dispatches, jit-ready §2.3), removes the per-type arena
base (cranelift-gc §6.4), and settles flonums before the tag-ABI freeze (jit-ready R1). From today's map
only the specials move (`001` → `010`; chars, `010` today, become its sub-tag); pairs keep `011`, closures
take the dead `110`, objects keep `111` (§15, stage 5).

---

## 3. Object model

**Invariant W (word-scannability).** Every word of every object that can hold pointers is either a
`TaggedValue` or a word whose low 3 bits are `000`, so it reads as a fixnum: headers, code-descriptor
pointers, packed integers. Pointer-free objects (strings, bytevectors, flonum boxes, bignums, symbols) are
never written through the barrier. W makes granule logging sound (a logged granule is re-read as two
values, §7), makes padding harmless (holes are zeroed, §5), and keeps the card-marking runner-up sound if
it is ever needed (barrier-res §3, conditions 1–3). It is Chez's "headers look like fixnums" trick
(chez §1).

**Header word** (every object except pairs and closures):

```
 63                                        16 15 13 12 11 10         3 2   0
[ length / size field (48 bits)             | flags | hash |  type (8)  | 000 ]
```
- `type`: up to 256 kinds.
- `hash` (2 bits): reserved for a later Lilliput-style "hashed / hashed-and-moved" upgrade (hotspot §3.10).
  It stays `00` until kill criterion K7 fires; identity hashing pins instead (§6.8).
- `flags` (3 bits): per type, written only by the constructor or by a single owner. Examples: the immutable
  literal bit, ephemeron broken, promise done, identifier written-in-source, bignum sign.
- `length`: the element count for vectors, strings, bytevectors, records and values; the limb count for
  bignums; the word count for continuations.
- **No GC state lives in the header.** Mark, END, pin, log and forwarding bits live in side metadata, so
  the mutator never read-modify-writes a GC-owned bit (threads-rec §2.12).

**Side metadata byte**, one per 16 B granule, in a flat table (§4):

| Bits | Field | Values |
|---|---|---|
| 0–2 | state | 0 = free or young-unmarked; 1, 2, 3 = rotating mark epochs; 4 = FORWARDED; 5 = BUSY (reserved for parallel evacuation); 6 reserved; 7 = PERMANENT (immortal space) |
| 3 | END | last granule of a marked object; written by the marker |
| 4 | PINNED | start granule only; never evacuated; does not keep the object alive |
| 5 | KEYHINT | start granule; "an ephemeron is pending on this key", which saves a hash probe (§6.6) |
| 6 | LOG | armed: the first store into this granule since it became old must be logged |
| 7 | reserved | ALLOC-BLACK for a future incremental mode, or a second log bit |

**Layouts** (sizes round up to 16 B; "pf" = pointer-free):

| Kind | Tag | Words | Bytes | Notes |
|---|---|---|---|---|
| pair | `011` | car, cdr | 16 | 27.9% of allocations; one granule |
| closure | `110` | `code` (CodeDesc address, `000` low bits), fv₀…fvₙ₋₁ | 8+8n | the size comes from `CodeDesc.nfree`, read by the GC through word 0. A JIT call is a tag test, then `ldur code, [f, #-6]` |
| vector | `111` | hdr(len n), e₀…eₙ₋₁ | 8+8n | inline; no `malloc` |
| string | `111` | hdr(len n), UTF-32 code units | 8+4n | pf. UTF-32 is kept: O(1) `string-set!`, and the 252 heap mentions in `strings.rs`/`characters.rs` stay as they are (owner decision #5) |
| bytevector | `111` | hdr(len n), bytes | 8+n | pf; buffers above 8 KiB land in the LOS, which never moves (FFI) |
| flonum box | `111` | hdr, f64 | 16 | NaN, ±∞ and out-of-band magnitudes only |
| bignum | `111` | hdr(limbs n, sign flag), u64 limbs | 8+8n | pf. Arithmetic builds `num-bigint` temporaries from the limbs; today it already clones a `BigInt` on every dispatch (`core:numeric.rs:418-435`). No `Drop` |
| ratnum / complex | `111` | hdr, num, den / hdr, re, im | 32 | a complex number is one object (parts are usually immediate flonums), where today it is three |
| cell (`MutableCell`) | `111` | hdr, value | 16 | today 72 B |
| record | `111` | hdr(nfields), rtd, f₀… | 16+8n | one allocation; today three plus a field vector (`prim:records.rs:138-182`) |
| record type | `111` | hdr, name, parent, uid, field names, flags | 48 | **canonical**: one object per type |
| symbol | `111` | hdr(len), hash, UTF-8 bytes | 16+len | pf, immortal, allocated in the cell/symbol space; the hash is stored, never derived from the address |
| identifier | `111` | hdr(written flag), symbol, scope-set id, source id | 32 | no `Drop`; today 72 B. 2.19 M of these during the R7RS-large load (libload §6) |
| ephemeron | `111` | hdr(broken flag), key, value | 32 | always allocated young (§6.6) |
| promise | `111` | hdr(done flag), content | 16 | content is a thunk, a value or a shared promise box (SRFI 45) |
| parameter | `111` | hdr, converter, global value | 32 | deep binding: a thread's parameterization maps parameter → cell (threads-rec item 9) |
| port | `111` | hdr, port id, flags | 32 | the `PortTable` owns the Rust `PortData`; the object has a finalization entry |
| primitive | `111` | hdr, registry index, name | 32 | **canonical**: one per primitive, created at install |
| condition | `111` | hdr(kind), message, irritants | 32 | |
| continuation | `111` | hdr(words), meta, frames, registers, dynamic stacks | variable | immutable after capture (§10); LOS above 8 KiB |
| code descriptor | `111` | hdr, body pointer, constants vector, JIT entry, packed `nfree\|nregs\|arity`, unit id | 48 | PINNED; has a finalization entry; frees the Rust body and the machine code |
| macro | `111` | hdr, literals vector, body pointer | 32 | literals are traced heap data; the Rust body is finalized |
| host payload (tree-walker) | `111` | hdr(proc or cont), host id | 16 | an id into the evaluator's host-payload table (§8) |
| global cell | raw `CellRef` | value, name symbol, meta (flags + namespace id), pad | 32 | immortal cell space; never a Scheme value |

**Rust `Drop` policy: no heap object owns a Rust `Drop` payload.** Today 47% of allocations carry one
(finalization §1.2), and lazy sweep never reads object memory. Rust-owned resources (`PortTable` entries,
code and macro bodies, tree-walker host payloads) live in side tables, are reached by an id or a
`000`-low-bits raw word, and are released by the **finalization registry** (§6.7). They are under 1% of
allocations (finalization §4.3).

**One layout specification** generates the size, trace, update, verify and debug-print code, plus the JIT
offset table: a `declare_layouts!` macro in `patina-gc`. This is the lesson of Chez's `mkgc.ss`
(racket-larceny §3.2) and fixes the hazard warned of at `heap/mod.rs:137-141`, where "misfiling a variant
is a use-after-free". `datum_writer.rs`'s 27-arm match is replaced by `cx.object_kind(v)`.

---

## 4. Heap organisation

**One virtual-address reservation per heap**, mapped read-write with `MAP_NORESERVE` (Linux) or as plain
lazily committed anonymous memory (macOS). Pages are touched on demand and decommitted with `MADV_FREE`
(`MADV_DONTNEED` where immediate return matters). A probe measured 4,096 reservations of 16 GiB on this
machine with RSS unchanged, and `MADV_FREE` plus `PROT_NONE` returning RSS at once (demographics §8.2). Read-write mapping
avoids the Linux `vm.max_map_count` problem that per-block `mprotect` commits would cause. No process-wide
state exists, so 318 live heaps in one test binary are fine; the heap drops its reservation on `Drop`.

```
reservation (default 16 GiB data, configurable HeapConfig::reserve):
  [ metadata: 1 byte / 16 B granule = data/16 ][ block descriptors: 16 B / block ][ data: blocks + LOS pages ]
separate small reservations: per-mutator SSB (256 MiB VA), per-thread register stack (8 GiB main, §8)
```

`meta_bias = meta_base − (data_base >> 4)`, so `meta(a) = *(meta_bias + (a >> 4))` for any address in the
data region. That is one shift and one load, with no alignment requirement beyond 16 B (only 1 in 64
returned regions was 4 GiB aligned, demographics §8.2).

**Spaces**, composed MMTk-plan style:

| Space | Holds | Allocation | Collection | Moves? |
|---|---|---|---|---|
| **Small-object space (SOS)** | objects ≤ 8 KiB | bump into holes; objects > 256 B ("medium") go to an overflow allocator that uses empty blocks only (Immix, immix-mmtk §2) | mark-region; lazy sweep | no (stages 5–6); opportunistic evacuation (stage 7) |
| **Large-object space (LOS)** | objects > 8 KiB | 16 KiB page-granular runs in the same reservation, first-fit by size class | mark; eager sweep and decommit at the end of the pause | never (the natural home for buffers lent to Rust or FFI) |
| **Cell/symbol space (immortal)** | global cells, interned symbols | append-only blocks flagged `IMMORTAL` | never swept; cells are scanned as a root region at majors | never; JIT code may embed cell and symbol addresses as immediates (global-cells §6) |
| **Code descriptors** | CodeDesc objects | SOS, allocated PINNED | marked from frames, closures, continuations and parent code; finalized | never |
| **Register stacks** | VM frames and registers | one reservation per green thread; no relocation (jit-ready R24) | a root region (§8) | n/a |

Deliberately absent: a small pointer-free space (headers already mark leaves; strings are ~0% of GC
workloads) and a boot-image static space (the ~1–2 MB bootstrap set becomes old at its first GC; a Chez
static generation can follow boot images).

**Sizes and why.**

| Parameter | Value | Basis |
|---|---|---|
| granule | 16 B | 56% of objects are exactly 16 B; pairs, cells and 1-free-variable closures are each one granule (demographics §4.3) |
| block | 32 KiB (2,048 granules; 2 macOS pages) | Immix's 32 KB block with an 8 KB LOS bounds block-level waste at 25% (immix-mmtk §2). Smaller than Whippet's 64 KiB because the bootstrap heap is about 1–2 MB and 318 heaps share a process |
| line | none: the metadata byte *is* the line table at granule size | Whippet `collector-mmc.md` |
| medium threshold | 256 B | Whippet `NOFL_MEDIUM_OBJECT_THRESHOLD`; 99.2% of objects are ≤ 128 B |
| LOS threshold | > 8 KiB | 148 of 348 M objects exceed it |
| recyclable block | ≥ 25% free granules after a sweep; otherwise "full" until the next major | Immix/nofl promoted-block idea (`nofl-space.h:620-650`) |
| free reserve | ≥ max(8 blocks, 5% of committed heap) empty blocks after every major | the fix for Wingo's 2025-05-22 livelock (whippet §1.10) |
| evacuation reserve (stage 7) | 2.5% of blocks | Immix paper; sensitivity is low between 1 and 3% |
| initial commit | 64 blocks (2 MiB) | bootstrap fits; per-heap minimum stays small |

**Overhead and bytes.** Granule metadata is 6.25% of the data region; block descriptors (live-granule
count, block state, `swept_epoch`, `has_pending_keys`, hole statistics) are 0.05%; the SSB is capped at
8 MiB. Objects cost the "hypothetical layout" (demographics §1.1) plus about 7% of 16 B rounding, which
headerless closures win back (a 1-free-variable closure is 16 B, not 32). Net: about 2× fewer bytes than
today [I, from demographics §4.4].

**Decommit.** After each major, empty blocks beyond `free_reserve + reserve_ratio × live` (ratio 1.0, as in
Chez `heap-reserve-ratio`, chez §2) are released, together with their metadata pages. Dead LOS runs are
released eagerly. This is what removes the 464 MiB of empty arena capacity retained after the 26-library
load (libload §0.5).

---

## 5. Allocation

**The contract stays exactly as today: allocation never collects** (`core:heap/mod.rs:570-584` [V*]).
- About 230–890 Rust functions hold unrooted values across allocation (offheap §5.2), and Chez runs a moving
  generational collector under the same contract (chez §3; DIGEST App. A #2).
- Allocation sites are therefore never safepoints. JIT allocation needs no stack maps, and initializing
  stores need no barrier because no safepoint separates allocation from initialization (JEP 475, hotspot
  §3.3).

**Mutator state** (`#[repr(C)]`, the JIT ABI object, §11): `alloc_ptr @0` and `alloc_limit @8`, both
`Cell<usize>`. `limit` is the end of the current hole, or 0, which forces the slow path.

**Rust fast path** (interpreter and primitives):
```rust
#[inline(always)]
fn alloc_small(&self, bytes: usize) -> *mut u64 {        // bytes: a multiple of 16, ≤ 256
    let p = self.alloc_ptr.get();
    let np = p + bytes;
    if np <= self.alloc_limit.get() { self.alloc_ptr.set(np); return p as *mut u64 }
    self.alloc_slow(bytes, Kind::Small)                   // #[cold]; never collects
}
pub fn cons(&self, a: Value, d: Value) -> Value {
    let p = self.alloc_small(16);
    unsafe { p.write(a.raw()); p.add(1).write(d.raw()); }
    Value::from_raw(p as u64 | TAG_PAIR)
}
```

**JIT fast path** (aarch64; `x21` is Cranelift's single pinned register holding `*Mutator`, cranelift-gc
§2.7):
```
ldr   x0, [x21, #0]        // alloc_ptr
add   x1, x0, #16
ldr   x2, [x21, #8]        // alloc_limit
cmp   x1, x2
b.hi  alloc_slow_cold
str   x1, [x21, #0]
stp   x_car, x_cdr, [x0]   // initializing stores: no barrier
orr   x3, x0, #3           // tagged pair
```
- That is 6 instructions plus the initializing stores and the tag; with `alloc_ptr` and `alloc_limit`
  cached in registers it is `add; cmp; b.hi`, a copying nursery's 3–4 (DIGEST §4.6).
- Whippet's 8–10 included begin and end metadata writes (§1 deviation).
- Within a fragment, `alloc_ptr` stays in an SSA variable and is written back before any call. All
  allocations in a basic block share one bump and one check (OCaml Comballoc, ocaml-gambit §4.2).
- A later optimizing tier (S3) reserves at its poll the maximum straight-line allocation of the region,
  so in-body allocations need no check and **no slow-path call is live across GC references**. This
  matters because a cold-block call still forces spills under Cranelift user stack maps (barrier-res §6).

**Slow path** `alloc_slow(m, bytes, kind)`. It never collects:
1. If `bytes > 8 KiB`, allocate in the LOS (fallible for user-sized requests, see below).
2. If `bytes > 256`, use the overflow allocator over empty blocks only.
3. Otherwise, find the next hole in the current block by **lazy sweep**:
   - scan metadata from the block's sweep cursor;
   - skip each live object (`state == current epoch`) to its END granule;
   - a hole is a run of granules with any other state;
   - **zero the hole's metadata and data** (bulk zeroing, which RC Immix found cheaper than per-object
     zeroing, immix-mmtk §4), so padding words read as fixnum 0 (invariant W);
   - return `[hole_start, hole_end)` as `(alloc_ptr, alloc_limit)`.
4. If the block has no hole left, take the next recyclable block, then a free block, then commit new
   blocks.
5. **Accounting happens at refill, not per object.** `bytes_since_gc += hole_bytes`, so the fast path
   carries no counter. Today's `note_alloc` is per object.
   - If the nursery budget (generational) or the heap target is crossed, set `event |= GC_MINOR` or
     `GC_MAJOR` and `tick = 0` (§9). The mutator keeps allocating.
   - Past the **soft limit** it keeps committing. At the **hard ceiling** (`HeapConfig::max_heap`, default
     the smaller of the reservation and 50% of physical RAM), small infallible allocations dip into a
     1 MiB emergency reserve, set `event |= HEAP_EXHAUSTED`, and the next poll aborts the interpreter with
     a reported error and a non-zero exit, after flushing ports (memory rule "exit status must report
     failure") [I].
6. **User-sized allocations are fallible**: `make-vector`, `make-string`, `make-bytevector`, string-port
   growth and `read-string`. They call `try_alloc(bytes) -> Result<_, Oom>`, and the primitive raises an
   ordinary Scheme error.

---

## 6. Collection

All collection is stop-the-world on the mutator thread (owner priority: throughput first, bounded pauses,
no concurrent marking). There are two kinds, plus evacuation at stage 7.

### 6.1 Major (full) collection
1. Retire allocation buffers (`alloc_ptr = alloc_limit = 0`).
2. **Flip** the mark epoch (1 → 2 → 3 → 1).
3. **Drop the SSB.** No re-arming pass is needed, because step 5 rewrites the log bits of every live
   object.
4. Trace roots (§8): register stacks with safepoint maps, root scopes and handles, the thread table,
   cells, host-payload and pinning roots. Pinning roots are traced **first**, so a later evacuating pass
   (stage 7) marks them in place, as Whippet does for conservative roots (whippet §1.4).
5. **Mark.** To mark an object, read its start byte. If the state is not the current epoch:
   - write `state = epoch | (byte & (PINNED | KEYHINT))`;
   - in generational mode, set LOG on every granule of the object (a `u64` store covers 8 granules);
   - set END on its last granule;
   - add its granules to `block.live_granules`;
   - push its fields.

   Sizes come from the tag (pair), word 0 (closure → CodeDesc) or the header.
6. Run the weak/ephemeron fixpoint (§6.6), then the epilogue (§6.9).
7. **Catch-up sweep.** Blocks whose `swept_epoch` lags by two majors get a metadata-only, word-parallel
   sweep in the pause. Lazily swept blocks can otherwise carry stale epoch values that alias the epoch
   after next. The cost is bounded by those blocks' metadata (1/16 of their bytes).
8. Blocks with `live_granules == 0` go straight to the free list (a memset of 2 KiB of metadata, no
   scan). Recyclable and full blocks are classified from the live count.

**Pause** ≈ roots + live objects marked + catch-up. **No term grows with the arena high-water mark**, and
that term is what produced today's 41 ms (queue3, mostly sweep) and 178 ms (post-load) pauses
(demographics §3).

### 6.2 Minor (sticky-mark) collection (generational heaps only)
Minors do not flip. "Old" means the start granule's state equals the current epoch; "young" means state
0. A young object becomes old when a minor marks it (promotion on first survival, which demographics §5.5
says wastes little).
1. Retire buffers.
2. Trace roots. Register stacks are scanned only **above the stack watermark** (§8.1).
3. For each SSB entry (a granule address), re-read its two words, treat each as a value, mark young
   referents, and **re-arm** the granule.
4. Mark young objects only; do not trace through old objects.
5. Run the weak fixpoint over young entries only (§6.6): finalization and host-payload young lists, and
   young LOS objects.

**Pause** ≈ roots above the watermark + SSB entries + survivors. Old dead objects remain until the next
major, which is sticky semantics. **Lazy sweep after a minor** re-sweeps only the blocks allocated into
since the last GC (the "young blocks" list).

**Why mark-region sticky marks rather than a copying nursery, and where it loses.**
- *Wins.* No copy reserve. No forwarding in minors. Pinning is free. Raw `TaggedValue`s held by
  tree-walker Rust structures, raw-bits memos in the expander and immediate addresses in JIT code all
  stay valid. Survivor marking costs per object; copying costs per byte.
- *Loss 1.* Dead young objects are not free: lazy sweep reads and zeroes 1 metadata byte per 16 B
  reclaimed, and zeroes the data. A copying nursery also zeroes memory but reads no metadata. Mitigations:
  word-parallel scanning, and `live_granules == 0` blocks freed without any scan (the common case in
  churn workloads).
- *Loss 2.* Locality: new objects interleave with old survivors in recycled holes. Wingo's own verdict is
  "not quite as good as semi-space nursery" (whippet §1.6).
- *Loss 3.* Survivors are never compacted until stage 7.
- *Loss 4.* Hole refills are more frequent than nursery chunk refills once old blocks fragment.
- Kill criterion K4 says when to add a copying nursery.

### 6.3 Marking order and a bounded mark stack
- The mark stack is a chunked stack: 4 KiB chunks recycled from a free list, with chunk bytes counted in
  GC statistics.
- **Push cdr, process car first.** Today's car-then-cdr order makes the stack O(list length) for lists
  with heap cars: +55 MB on 2 M two-vectors (gc-impl §3.2). With cdr pushed and car processed first, the
  stack is O(depth of the car structure).
- Vectors and records push one "range" entry, `(object, next index)`, rather than every field (Racket
  BC's segmented mark stack, racket-larceny §5.6).
- Green Tea-style span batching (whippet §2.1) is a later optimization behind the same interface.

### 6.4 Lazy sweep
Per block and on demand (§5, slow-path step 3): it reads only metadata, writes only the holes it returns,
runs at most once per block per cycle (`swept_epoch` drives the §6.1 catch-up), and never runs a
destructor; that obligation belongs to the finalization registry (§6.7; finalization §6.1).

### 6.5 Evacuation and pinning (stage 7; candidate B)
- **Trigger.** Evacuate at a major when the previous cycle's fragmentation exceeds 10%, and keep doing so
  until it falls below 5% (Whippet `mmc.c:656-715`). Fragmentation is free granules not in usable holes
  ≥ 256 B, divided by committed SOS granules.
- **Source blocks**: the lowest-occupancy blocks under 75% live, in Chez's in-place rule (chez §7).
- **Targets**: the 2.5% evacuation reserve, plus up to 50% of free blocks during compaction.
- **Mechanism.** A non-PINNED object in a source block is copied once, its start byte set to FORWARDED,
  word 0 set to the new address, and the referencing **slot is updated** (the slot visitor, §8). When the
  reserve runs out, remaining objects are marked in place.
- **Pins never move**: the PINNED bit, pinning roots traced first, and LOS objects.
- **Log bits**: the destination's granules are armed as marking writes them, and the source's are cleared
  by sweep.
- **Optional young-survivor evacuation** inside minors (GenImmix-like) is gated on M5 and K4.
- **Prerequisites**: slot-based roots, `CallFrame.closure` as a value and tree-walker pinning roots
  (stage 2); no persistent raw-bits maps on movable keys (stage 4c); pin-on-hash (stage 5). The
  `TortureGc` lane (§14) proves them.

### 6.6 Weak references and ephemerons, with no O(n²) rescans
- **Key-indexed pending resolution** (Whippet `gc-ephemeron.c`):
  1. When an ephemeron is reached and its key is unmarked (and not an immediate), put it in `pending`, a
     map from key address to a chain of ephemerons, and set KEYHINT on the key's start granule.
  2. When the mark stack first drains, scan `pending` once and trace the values of entries whose key is
     now marked.
  3. From then on, **each newly marked object with KEYHINT set** probes `pending` and traces the values
     waiting on it. The hint bit avoids a hash probe on every mark.
  4. Repeat until no progress. Then break the remaining entries (key and value become `BWP`; the broken
     state stays distinct from `#f`).

  Each ephemeron resolves in amortized O(1). The 16 k-element chain that costs 289 ms in the bad order
  today (finalization §0.7) becomes linear.
- **Generations.**
  - Ephemerons are always allocated young; they are 32 B, never in the LOS, never in immortal space, and
    never pretenured. Promotion is age-monotone.
  - So "E is never older than its key or value" holds (whippet §1.6; finalization §4.6). Minors skip old
    ephemerons, and no remembered-set edges are needed for them.
- **`(gc)` is always a full major** (`ephemerons.rs:23,52,213,226`).
- **One fixpoint** covers ephemerons, host-payload weak ids and (later) guardians (commit `1d18c49`).

### 6.7 Finalization and ports (R1–R6)
- **Registry.** `FinalRegistry { young: Vec<Entry>, old: Vec<Entry> }`, with
  `Entry { obj, kind: Port | Code | Macro | HostPayload | Foreign, id }`. Entries are created only by
  slow-path constructors. Inline JIT allocation never makes finalizable kinds (finalization §6.7).
- **After a minor**: a young entry whose object is marked moves to `old`; one that is unmarked is queued.
  **After a major**, unmarked old entries are queued as well. This is OCaml's custom table and Chez's
  per-generation guardians (finalization §4.1).
- **Running finalizers.** Queued finalizers run **after the pause and before Scheme resumes** (poll slow
  path), so a blocking pipe flush is not pause time; they run no Scheme and allocate nothing (R4). Ports
  flush (errors ignored), close and free their `PortTable` slot (R2; `MemoryFs` writers commit); code
  frees its Rust body and machine code.
- **R1.** `end_process` and `emergency-exit` flush every live `PortTable` entry (replacing `OUTPUT_FILES`).
- **R3.** Every file-port open adds 8 KiB of external bytes. After N = min(128, RLIMIT_NOFILE/4) file opens
  since the last GC, the open path sets `event |= GC_MAJOR`. On `EMFILE`/`ENFILE` the open primitive
  returns a distinguished failure; a resumable `Step` requests `(gc)`, passes a safepoint and retries once
  (chibi's behaviour, `eval.c:1300-1323`).
- **R5.** Dropping the heap finalizes every entry. The interpreter-drop leak goes away with
  `VmClosure.globals` and `Environment.heap` (stage 4).
- **R6.** `current-*-port` parameters hold the canonical port object.
- GC-time flushing is observable, so its tests live **outside** the GC differential lane, and the lane's
  documentation says so (finalization §6.10).

### 6.8 Code liveness and identity hashing
- **Code liveness is decided by marking, not counting.** Edges into a CodeDesc come from frames
  (`visit_code`), closure word 0, frames inside continuations, and parent descriptors' constants (nested
  lambdas). This deletes `live_closures`, `gc_freed_closure_code_ids` and `RETIRED_VM_CLOSURE_CODE`
  (`vm_state.rs:521-575`). Minors are precise for young objects, so a young CodeDesc that dies in a minor
  is dead, and `eval`'s one-shot code is freed then (#353's case). Machine code goes with its descriptor;
  under S1 no native frame survives a safepoint, so no epochs are needed (cont-repr §4).
- **Identity hash** is `mix(address)`, stored by SRFI 69 in buckets (`srfi-69-impl.scm:118`). The first
  identity hash of an object **sets PINNED** (a load and test, then one byte write; Guile chose the same,
  whippet §1.11). That costs nothing while the heap is non-moving, and keeps every stored hash valid once
  evacuation exists. Pairs need no header word. `equal-hash`'s heap-index fallback becomes identity hash.
  K7 says when to upgrade headered objects to Lilliput hash words.

### 6.9 Epilogue order (finalization §4.9, adapted)
1. Trace.
2. Fixpoint: young and old host-payload ids → key-indexed ephemerons → (future) guardians.
3. Break the remaining ephemerons.
4. Weak-key tables: rekey or prune. Only `syntax_sources` remains until inline provenance lands; the
   generational rule applies (young-key entries in minors).
5. Prune the host-payload tables.
6. Classify finalization registry entries; queue dead ones.
7. Sweep and release the LOS.
8. Classify blocks; catch-up sweep; decommit (majors only).
9. Statistics and pacing update.
10. Release the world. At the poll, after the pause, run queued finalizers.

---

## 7. Write barrier

**Kind: pre-write field logging at granule granularity, with an armed-bit polarity.**
- Fresh, zeroed memory is unarmed, so stores into young holders fall through.
- Logging happens when a store hits an *armed* granule of an old object.
- Logged entries are **exact granule addresses** in a per-mutator sequential store buffer (SSB),
  deduplicated by disarming.
- This is barrier-res's recommendation (§7.1), with three refinements:
  - one log bit per granule instead of per word, so the fast path needs no parity computation even for
    variable-index `vector-set!`. Two fields in one granule share one log entry;
  - the one-bit value filter (§2);
  - log bits live in the same metadata byte as mark state, so they cost no extra metadata.
- No heap parsability is needed. Invariant W makes the "re-read both words" rule safe (header and code
  words read as fixnums).

**aarch64 fast path**, holder in `x0` (tagged), value in `x1`, field offset `off` from the object start
(`x20` = `meta_bias`, loaded once per fragment from `Mutator+24` and marked `readonly`):
```
tbz   x1, #1, 1f               // value filter: fixnums and flonums skip     (1)
add   x2, x0, #(off - TAG)     // slot address (often already needed for the store)
lsr   x3, x2, #4               // absolute granule index                     (1)
ldrb  w4, [x20, x3]            // metadata byte                              (1)
tbnz  w4, #6, log_cold         // LOG armed → slow path                      (1)
1: str x1, [x0, #(off - TAG)]
```
- Heap-value path: **4 instructions** beyond the store and its address computation.
- Fixnum and flonum path: **1**.
- For comparison: barrier-res estimated 6 for per-granule static parity and 8–9 dense; an unconditional
  card is 2 (5 with a filter); G1 after JEP 522 is 12 x64 instructions (hotspot §3.3).
- Whether Cranelift selects `tbz`/`tbnz` for a single-bit test is unverified. If it does not, each test is
  `tst` plus a branch: 6 on the heap-value path, 2 on the immediate path.

**Inline slow path. It has no call** (a Cranelift call is a safepoint; barrier-res §6):
```
log_cold:                               // cold block
  and   w4, w4, #~0x40 ; strb w4, [x20, x3]     // disarm (plain store under M:1; `ldclrb` under `threaded`)
  ldr   x5, [x21, #32]                          // remset_cur
  and   x6, x2, #~15 ; str x6, [x5], #8         // append the granule address
  str   x5, [x21, #32]
  ldr   x7, [x21, #40] ; cmp x5, x7 ; b.lo 1b   // below the soft limit → done
  ldr   w8, [x21, #16] ; orr w8, w8, #EV_GC_MINOR ; str w8, [x21, #16]
  str   wzr, [x21, #20]                          // tick = 0: the next poll takes the slow path
  b     1b
```
- The SSB lives in reserved VA: 256 MiB per mutator, lazily committed, with a soft limit of 1 M entries
  (8 MiB). One entry is pushed per distinct granule per cycle, and polls bound straight-line code, so the
  hard end is unreachable (barrier-res §6).
- Measured scale on the barrier workloads: nboyer 5 slow paths per run, hashtab 131 K, vecsort 51 K,
  queue 96, tree 46 K (barrier-res §2.3). With granule dedup these counts can only fall.

**Filters, in order.**
1. **Static elision.** Literal immediates; results of `Not`, `NullP`, `PairP`, `VectorP`, `Eq`, `Lt`,
   `NumEq` and the `*Imm` comparisons; never arithmetic results, which can overflow to bignums (barrier-res
   §5). Also **initializing stores** with no safepoint between allocation and store: constructors take
   their contents, and the JIT groups a block's allocations.
2. **Dynamic value filter**: bit 1 of the value. It removes 64% of heap stores in aggregate (demographics
   §6.2).
3. **Holder young**: free, because the granule is unarmed. The median workload sends 99.4% of its stores
   to young holders.
4. **Already logged this cycle**: free, because the granule was disarmed.

**When bits are armed.**
- Marking arms every granule of every object it marks, in generational mode only.
- Minor SSB processing re-arms the granules it processes.
- Sweep clears the bits of freed granules.
- Cells are born old in immortal space, so their constructor **pre-logs** the cell (G1's
  `on_slowpath_allocation_exit` compensation, hotspot §3.3).
- LOS objects are born young, like everything else in sticky mode. They need no pre-logging, unlike in a
  copying nursery with direct-old large allocation.

**Rust store funnel.** This is the only way to write a value into a heap object. `vector_slice_mut` is
removed: its single non-test caller is `vm_state.rs:2380` (DIGEST App. A #8).
```rust
impl<'gc> Mutation<'gc> {
    #[inline(always)]
    pub fn write(&self, slot: FieldSlot<'gc>, v: Value<'gc>) {      // FieldSlot = address of a field word
        if v.raw() & 0b10 != 0 {
            let m = unsafe { *self.meta_bias.add(slot.addr() >> 4) };
            if m & LOG != 0 { self.log_granule_cold(slot.addr()) }
        }
        slot.store(v)                                                // plain; relaxed atomic under `threaded`
    }
    pub fn write_range(&self, holder: Value<'gc>, first: usize, len: usize); // vector-fill!, vector-copy!:
                                                                     // logs every armed granule in the range
    pub(crate) unsafe fn init(&self, slot: FieldSlot<'gc>, v: Value<'gc>);   // constructors only; debug-asserts
                                                                     // that the holder is young or pre-logged
}
// typed façade: set_car, set_cdr, vector_set, cell_set, record_set, global_set (cell),
// parameter_set, promise_resolve, ephemeron_break (GC-internal, never barriered)
```

**No barrier** on strings, bytevectors, register-stack and frame stores (roots, covered by the watermark,
§8.1) or continuation objects (fresh and immutable). Environment stores stop being a channel: globals go
to cells (stage 4b) and tree-walker frames live in whole-heap heaps (§8.4). A non-generational heap
reports `BarrierKind::None`: its JIT code emits nothing, and the Rust funnel's branch is never taken.

---

## 8. Roots and rooting

### 8.1 VM frames
- **The register stack does not relocate.** It is one reservation per green thread, committed on demand,
  with a limit check on push (jit-ready R24). The main thread reserves 8 GiB, which covers today's
  10 M-frame recursion at 1.37 GB (vm-runtime §0).
- **Frames are `Copy` and interleaved** with their registers:
  `[code: *const CodeDesc][pc:u32|nregs:u16|ret:u16 (fixnum-packed)][closure: Value][r₀ … rₙ₋₁]`.
  - Every word satisfies invariant W, so a frozen chunk is one word array (cont-repr stage 3).
  - `CallFrame.closure: Option<HeapIndex>` becomes a `Value`, and `code: Rc<CodeObject>` becomes a raw
    descriptor pointer. That removes four `Rc` operations per call/return pair (vm-runtime §2.1).
- **Liveness maps at safepoints only.** Frames below the top are always suspended at a call's return pc,
  and the top frame is at a poll. Today's per-pc `Vec<Vec<u64>>` (40 B per pc, 65% of the instruction
  stream, libload §2.2) becomes a sparse table, keyed by pc, of bitsets interned per code object.
- **Scan rule.** For each frame, `map = code.safepoint_map(pc)`, then `visit_slot(&mut r_i)` for each live
  `i`.
  - Stub frames have no map, so all of their slots are visited.
  - Dead slots are not visited, so `retire_registers`' overwrite moves to capture time only (§10), which
    keeps the #423 tests (`ephemerons.rs:94,109,127,162`).
- **Stack watermark (generational heaps).** `low_water = min(low_water, index of the frame that becomes
  active)` on every `Return`, unwind and abort, and on delimited append (to the append base).
  Continuation reinstatement sets it to 0. A minor scans frames `[low_water, top]`, which always includes
  the executing frame, and then sets `low_water` to the executing frame's index (not past it: that frame
  keeps writing its registers).
  - Sound because suspended frames are immutable until resumed, and after a GC they reference only old
    objects (hotspot §3.12; jit-ready R26).
  - It removes deeprec's 11.5 M-register scan per minor (demographics §7.2). Kill criterion K8 covers its
    cost.

### 8.2 JIT frames by tier
- **Baseline (S1: fragments plus the tail convention).**
  - JIT code keeps Scheme values in SSA only *between* safepoints. Before any runtime call or poll it
    stores dirty values into register-stack slots and the frame's `pc`; afterwards it reloads values and
    the bases.
  - No native frame holding Scheme values survives a safepoint. **No Cranelift stack maps, no frame
    walker, no unwinder** (cranelift-gc §6.1; Guile and Sparkplug).
  - JIT code may embed as immediates the addresses of objects that never move: cells, symbols, CodeDescs,
    the Mutator. Movable constants load from `CodeDesc.constants`: two loads, hoistable. Code is never
    patched at GC (DIGEST pitfall 10).
- **A later optimizing tier (S3).** Values live across calls use `declare_value_needs_stack_map`; maps
  come from `MachBuffer::user_stack_maps()` (`JITModule` does not surface them, cranelift-gc §2.1); frames
  are walked by frame pointer; no derived pointer lives across a safepoint (pitfall 9). Allocation
  reserved at the poll (§5) and the call-free barrier slow path (§7) add no safepoints. Capture first
  deoptimizes S3 frames into VM frames.

### 8.3 Rust code
- `Mutation<'gc>` is a shared context with `Cell`-typed allocator and barrier state (DIGEST App. A #6).
  Primitives are `fn(&Cx<'gc>, &[Value<'gc>]) -> Result<Value<'gc>, Error>`, where `Cx` carries
  `&Mutation` and `&Instance` (features, command line, fs). Collection needs `&mut Heap`, which only the
  driver owns. The `'gc` brand is carried by the types and can be enforced later (rust-gcs §4.1).
- **Within a primitive, values are plain `Copy` locals across any number of allocations.** The ~280
  heap-only handlers, the reader, the desugarer and the expander need nothing more.
- **Across a may-GC boundary**: `Step` state (resumable primitives, already exists); a LIFO `RootScope`
  for re-entry, library loading and transformer calls (a root-slot bump, rust-gcs §2.10); an owned
  `Handle` (an index into a GC-updated table) for long-lived Rust holders.

### 8.4 Tree-walker policy: it may lag
- Tree-walker heaps are built with `HeapConfig { generational: false, evacuation: false }`. That is
  candidate A's whole-heap mode. Each backend builds its own heap (tree-walker §4e).
- It collects only at its existing outermost trampoline safe point. Nested trampolines keep `NoGcScope`,
  the renamed `GcDeferGuard`.
- Its `Rc<Environment>` frames, `ContValue`s and `StepResult` fields are reported through
  `visit_pinned(v)`: traced, never updated, about 140 ns per suspended frame (tree-walker §2.4).
- `CpsLambda` and `CpsContinuation` become host-payload ids, which removes drop-at-sweep. Globals go
  through cells.
- Its off-heap allocation is charged to the trigger by counting `Environment` creations (240 B each), one
  increment per creation.
- Green threads on the tree-walker are a `StepResult` per thread (threads-rec §4). It never runs as one of
  N carriers.

### 8.5 Embedding API
- `Interpreter::eval_*` returns `Handle` (an owned root with a generation check; `Drop` frees the slot).
  This fixes the measured use-after-free (prim-embed §7.2).
- `interp.with(|cx| …)` gives scoped access without per-value handles.
- `display_tagged` accepts `impl AsValue`, so most of the ~200 test call sites compile unchanged.
- Dropping the interpreter tears the heap down: it runs finalizers and releases the reservation.
- Reading without a VM still works: `patina-compat` builds a heap with `NullGc` (§11).
- Plugins never see `HeapIndex` or the encoding (prim-embed §8).

### 8.6 Fate of every off-heap holder

| Holder (today) | Fate | When |
|---|---|---|
| Environments and globals (`Rc<Environment>` slot tables, `FORWARDED` + `Owner` links) | Namespaces hold only `CellRef`s; values live in immortal cells. Cells are a root region at majors and reach minors through the SSB. Imports share the exporter's cell (#406 by construction). Variant **R** first, so semantics do not change | 4b |
| `VmClosure.globals: Rc<Environment>` | deleted; code carries its namespace link | 4b |
| `CodeObject.constants: Vec<TV>` in `Rc` | a constants vector (heap object) owned by the CodeDesc; immutable, traced through the CodeDesc | 4d |
| `CompiledMacro` literals and `CompiledMacro.heap` | a literals vector owned by the MACRO object; the body is finalized; the `heap` field is removed | 4f |
| `Library.exports: HashMap<String, TV>` | `HashMap<String, CellRef>`; the stale value copies go away | 4b |
| `syntax_sources` (raw bits → `Rc<SyntaxSource>`) | kept as a weak-key table processed in the epilogue while non-moving. Inline source ids (libload option a) delete it before stage 7. `SourceMap.locations` and child spans are **deleted now** (no production reader) | 4c / front end |
| `quoted`, `OpenNodes`, scope-edit memos, `PrimitiveCallMap.by_value` | stay transient. Under a non-moving heap, raw-bits keys remain valid across a rooted safepoint, which makes syntax-case transformer calls cheap (libload §7.7) | — |
| VM continuation side tables (`FxHashMap<u64, Rc<VmContinuation>>`) | deleted; continuations are heap objects (§10) | 4d |
| `WindRecord.handlers: Rc<[H]>` | a heap vector (stage 4d), later a persistent heap list (C′) | 4d |
| `CallFrame.closure` (bare index), `code: Rc` | `Value` and raw CodeDesc pointer | 2 / 4d |
| Symbol table `HashMap<String, HeapIndex>` | name → immortal symbol address; never traced | 5 |
| `Parameter{values: Rc<RefCell<Vec>>}` (process-wide shallow stack) | deep-bound: a per-thread parameterization (heap) maps parameter → cell | 4f |
| Record `Rc<RefCell<Vec>>`, promise `Rc<RefCell>` | inline heap fields through the funnel | 4f |
| Current ports (`thread_local!`) | the per-thread dynamic environment, a heap port object | 4a |
| Tree-walker `CpsExpr` literals | a per-form constants vector (heap), referenced by the host payload | 4e |
| `PENDING_ESCAPE`, `scratch_args`, `pending_escape` | root slots | 2 |
| Values held by the embedder | `Handle`s | 2 |
| Tracer snapshots, debugger hook storage | registered `RootProvider`s (open registration) | 2 |

---

## 9. Safepoints and polling

**The event word.** The Mutator holds `event: Cell<u32>` at offset 16 (`AtomicU32` under the `threaded`
feature) and a private `tick: Cell<i32>` at offset 20. Event bits:

| Bit | Event |
|---|---|
| 0 | `GC_MINOR` |
| 1 | `GC_MAJOR` |
| 2 | `PREEMPT` (green-thread quantum) |
| 3 | `SIGNAL` (Ctrl-C) |
| 4 | `DEBUGGER` |
| 5 | `TERMINATE` |
| 6 | `HANDSHAKE` (reserved for N mutators) |
| 7 | `FINALIZERS_PENDING` |
| 8 | `HEAP_EXHAUSTED` |

Anything that raises an event also sets `tick = 0`.

**The poll** decrements the tick and branches when it reaches ≤ 0:
```
ldr  w9, [x21, #20] ; subs w9, w9, #1 ; str w9, [x21, #20] ; b.le poll_cold
```
- That is 4 instructions, or 2 when the tick stays in a register within a fragment.
- The slow path resets `tick = quantum` (default 1024), then reads `event` and dispatches.
- This is Chez's `%trap` plus `something-pending` (chez §4), and the shape threads-rec item 7 asks for.

**Placement.**
- *Interpreter*: at `Call`, `TailCall`, `Apply` and `Return`, and at runtime-stub entry. Today there is a
  load before every instruction, which costs +1.1–1.4% (GC_DESIGN §6.1). Codegen emits forward jumps
  only, so every loop passes through a call or a tail call (jit-ready §2.2), and straight-line code is
  bounded by code size.
- *JIT*: at fragment entry and at self-tail-call loop headers. The allocation slow path and the barrier
  slow path only *raise* events.
- Loops that neither allocate nor call still get a poll, because the owner wants interrupts (owner
  decision #9). Without that requirement they would need none (cranelift-gc §4).

**Deterministic mode.** The tick quantum is the only source of preemption; no timer writes `PREEMPT` in
test lanes. So the GC differential lanes stay byte-identical with green threads present: the quantum is
counted in polls, not time. Ctrl-C and profiler signals set `event |= SIGNAL` and `tick = 0` with plain
stores (relaxed atomics under `threaded`).

**Nested Rust loops (what replaces `GcDeferGuard`).**
- The poll slow path may collect iff `mutator.unrooted_depth == 0`, where the count is incremented only
  by `NoGcScope`.
- Every other Rust frame that can be on the stack below a nested driver loop **registers what it holds
  in a `RootScope` first**. That covers:
  - library loading points A, B, D and E (libload §4.2): the import form, the import-set `Vec`,
    `ParsedLibrary.body` as a heap list held by the registry's `Loading` entry, and `with_globals`'
    `saved` on `VmState.globals_stack`;
  - `across_reentry`;
  - `%parameterize-swap!`, until it is made resumable.
- `NoGcScope` remains only at point C (after a mid-form import inside `desugar_with_imports`) and for
  nested tree-walker trampolines; an unserved event stays raised and the heap overshoots to the soft
  limit, then the hard ceiling (§5). This is JEP 423's lesson (hotspot §3.6). Measured payoff:
  between-form collection on `(nieper rbtree)` cut the `malloc` peak from 211 to 118 MiB (libload §0.6).

---

## 10. Continuations and stacks

**Representation: design A first** (cont-repr §3.1), the low-risk end state.
- A continuation is **one immutable, variable-size heap object**:
  - a header (word count);
  - meta words as fixnums: kind (full, delimited or abort landing), `deliver_reg`, the
    `*_depth_at_capture` fields, `base_at_capture`, `exit_status`;
  - the captured interleaved frame and register words;
  - the wind, handler and prompt arrays (`Value`s plus depths as fixnums);
  - the re-entry boundary ids.
- Capture uses `memcpy` and **clears dead slots in the copy** using the safepoint maps, plus the call/cc
  `dst` hole. That clearing is what fixed the 296 MB leak and what the #423 tests require.
- The GC then scans the object as plain words: every word is a value or reads as a fixnum (invariant W),
  and dead slots are already `UNSPECIFIED`.
- Captures above 8 KiB go to the LOS. At depth 1000 that is roughly 88 KB, against 171 KB today.

**What this deletes and what it costs.**
- Deleted: `VmContinuationRef`, `VmDelimitedContinuationRef`, both weak side tables, the VM's
  `trace_weak_ids`/`sweep_weak`, the "store touched within one dispatch" soundness rule
  (`gc_roots.rs:21-24`), and the orphan wind-step windows (the stub's TARGET becomes a strong edge).
- Captures become byte-accounted, so the 3.4–5.7 GB plateaus become the trigger's problem (cont-repr §2).
- Capture remains O(depth). The toy measured about 5.8× cheaper capture overhead (cont-repr §2.2).

**GC interaction.** Continuations are born young (SOS or LOS), written only by initializing stores and
never mutated (re-entry copies out), so they need no barrier; once promoted their armed bits are inert.
Reinstatement resets the stack watermark. Under evacuation SOS continuations move like vectors. Their
frames' CodeDesc pointers keep code alive (§6.8).

**JIT interaction.** Capture is a may-GC runtime call; under S1 every value is already in the register
stack, so capture is the same `memcpy` for interpreted and JIT frames. Each JIT function has a resume entry
per return pc, so reinstated `(code, pc)` frames resume in JIT code or, failing that, in the interpreter.
Escapes return a status without writing `dst` (`control.rs:56-84`).

**Control semantics are unchanged.**
- **Multi-shot `call/cc`**: immutable captured state, copied out on every invoke.
- **`dynamic-wind`**: travel, one thunk per step, through stub frames, which remain ordinary captured
  frames (property P1).
- **Delimited**: capture `[depth_at_capture..]`. `append_delimited` relocates by the same integer
  arithmetic (`execution_state.rs:319-424`).
- The 64-row `control_flow_matrix.rs` and `escape_from_primitive.rs` are the gate for stage 4d.

**Later, optional: design C′.** C′ is freeze-above-watermark plus immutable chunks plus lazy thaw: about
57 ns per capture independent of depth in the toy, against 5.0 µs today. Its frozen watermark is the
**same `low_water` the minor GC uses** (§8.1): one compare on `Return` serves both. With winds, handlers
and prompts as persistent heap lists, C′ also gives green threads segmented stacks with freeze-and-slide
overflow. It is scheduled after the baseline JIT and only if capture-at-depth matters (owner decision
#18). Its risk is medium-high: virtual depths have reproduced the #162/#163/#176 family before.

---

## 11. Pluggability contract

This follows the owner's answer: yes to pluggability for the contract, no to a catalogue of collectors.
Fast paths are data the JIT inlines, and the collector is selected statically.

**The contract** (crate `patina-gc`, no dependency on the VM, so Miri can run it):
```rust
/// Implemented by Patina core for its object model; generated from `declare_layouts!`.
pub unsafe trait ObjectModel {
    fn size(obj: Address, tag: Tag) -> usize;                       // pair: tag; closure: word 0; else header
    fn trace<V: FieldVisitor>(obj: Address, tag: Tag, v: &mut V);   // V::field(&mut Value) — slot-based
    fn is_pointer_free(obj: Address, tag: Tag) -> bool;
    const LAYOUT: &'static LayoutTable;                              // per-kind field offsets for the JIT
}

/// What roots look like to every collector. Open registration on the heap (`RootSet::register`).
pub trait RootProvider {
    fn trace(&mut self, v: &mut dyn RootVisitor);
}
pub trait RootVisitor {
    fn slot(&mut self, s: &mut Value);              // precise and updatable
    fn slots(&mut self, s: &mut [Value]);           // register windows, handle tables
    fn frames(&mut self, stack: &mut RegisterStack, from_depth: usize);  // map-driven
    fn pinned(&mut self, v: Value);                 // precise, never updated (tree-walker, debugger payloads)
    fn code(&mut self, d: *const CodeDesc);
    fn host_payload(&mut self, id: HostId);         // weak-id host tables
}

/// One production collector plus test collectors implement this.
pub unsafe trait Collector: Sized + 'static {
    const ATTRS: StaticAttrs;                        // allocator kind, granule, thresholds, can_move, can_pin
    fn new(cfg: &HeapConfig) -> Result<Self, HeapError>;
    fn runtime_attrs(&self) -> RuntimeAttrs;         // per heap: meta_bias, barrier kind on/off, offsets
    fn alloc_slow(&self, m: &Mutator, bytes: usize, kind: AllocKind) -> Option<NonNull<u8>>; // never collects
    fn try_alloc_large(&self, m: &Mutator, bytes: usize, kind: AllocKind) -> Result<NonNull<u8>, Oom>;
    fn log_slow(&self, m: &Mutator, granule_addr: usize);           // Rust twin of the inline JIT slow path
    fn pin(&self, obj: Address);                     // permanent pin (identity hash, FFI)
    fn is_live(&self, obj: Address) -> bool;         // for weak processing and verification
    fn requested(&self, m: &Mutator) -> Option<CollectionKind>;      // pacing decision, read at the poll
    fn collect(&mut self, kind: CollectionKind, cx: &mut CollectCx<'_>) -> GcStats;  // &mut: driver only
}
```

**The JIT ABI.**
- `Mutator` is `#[repr(C)]` with frozen offsets:

  | Offset | Field |
  |---|---|
  | 0 | `alloc_ptr` |
  | 8 | `alloc_limit` |
  | 16 | `event` |
  | 20 | `tick` |
  | 24 | `meta_bias` |
  | 32 | `remset_cur` |
  | 40 | `remset_soft` |
  | 48 | `reg_top` |
  | 56 | `reg_limit` |
  | 64 | `thread` |
  | 72 | `heap` |
  | 80 | `safepoint_state` (u8) |
  | 81 | `barrier_mode` (u8) |

- `RuntimeAttrs` is plain data the JIT reads **once per heap** at compile time:
  ```rust
  pub struct RuntimeAttrs {
      pub alloc: AllocAttrs { kind: BumpInHole, ptr_off: 0, limit_off: 8, granule: 16,
                              small_max: 256, large_threshold: 8192, writes_alloc_table: false },
      pub barrier: BarrierAttrs { kind: None | GranuleLog | Card, value_filter: Some(bit 1),
                                  meta_bias_off: 24, meta_shift: 4, log_bit: 6,
                                  remset_cur_off: 32, remset_soft_off: 40, inline_slow: true },
      pub poll: PollAttrs { kind: TickThenEvent, tick_off: 20, event_off: 16 },
      pub can_move: bool, pub can_pin: bool, pub layout: &'static LayoutTable,
  }
  ```
- This is Whippet's `gc-attrs.h` turned into a value (whippet §1.8). JIT code is per heap anyway, because
  constants are per heap.
- The interpreter's Rust fast paths are `#[inline(always)]` code over the same fields. JEP 475's lesson
  applies: one barrier definition, expanded late (hotspot §3.13).

**Static selection.**
- `pub type ActiveGc = …` in `patina-gc`, chosen by cargo feature: `gc-markregion` (default), `gc-null`
  or `gc-torture`.
- Downstream crates name `Heap`, never `Heap<C>`, so the ~1,300 call sites never become generic.
- `patina-gc`'s own tests are generic and instantiate every collector.
- There is no `dyn` on any fast path. Per-heap generational or evacuation choice is a runtime field read
  by slow paths and collectors only.

**Collectors shipped.**

| Collector | Role | Moves |
|---|---|---|
| `MarkRegion` | production. Modes: whole-heap; sticky-generational (stage 6); evacuating (stage 7) | stage 7 only |
| `NullGc` (Epsilon) | bump allocation only; collects only on explicit `(gc)` by doing nothing. It is the lower bound for performance A/B and the reference run for differential lanes (today's `PATINA_GC=0`); also the standalone reader heap | no |
| `TortureGc` | copies **every** non-pinned object on every collection into fresh blocks; `mprotect`s and poisons from-space in debug builds; zeal hooks (collect at every poll, or every N refills) | yes |

Today's mark-sweep is not ported. Its oracle role passes to `NullGc` plus the verifier (§14). The ZGC,
Shenandoah and Lua lesson applies: maintain one production collector (DIGEST pitfall 21).

**How a future collector plugs in.**
- **Incremental marking** (if owner decision #6 changes): `barrier_mode = Incremental`, read by the slow
  path only; an incremental-update remark re-scans logged granules (the value filter stays valid; SATB
  would need `value_filter: None` and recompiled JIT code); allocate-black by pre-marking holes in
  `alloc_slow`; marking slices from the poll slow path; darken continuations on reinstatement
  (ocaml-gambit §4.5). No fast-path shape changes.
- **Parallel tracing**: marking becomes `fetch_or` on the metadata byte, work-stealing chunk deques, and
  the reserved `BUSY` state for parallel evacuation (Whippet's live race is a warning, whippet §1.4).
- **A copying nursery** (K4): a new `NurserySpace` (a contiguous bump region) composed with `MarkRegion`.
  Same barrier, since nursery memory is never armed (Whippet pcc uses the same field barrier). Minors copy
  survivors into SOS blocks and arm them. It needs only the stage 7 prerequisites.
- **MMTk-backed**: implement `Collector` over an MMTk plan. Its untagged `ObjectReference` forces pair
  headers, which `LayoutTable` can express (pair car at offset 8 instead of 0), at +50% pair bytes. Its
  one-instance-per-process limit conflicts with 318 heaps (immix-mmtk §10). The seam keeps a spike
  possible; I do not recommend it.

**Explicitly excluded.** Load and read barriers (pitfall 11). Conservative stack scanning (#423).
Collection inside allocation. `dyn` dispatch on fast paths. Patching code at GC. Process-wide singletons.
Java-style finalizers (JEP 421).

---

## 12. Threading readiness

Following threads-rec (M:1 green threads now, interfaces for N mutators):

- **`Mutator` (carrier) and `GreenThread` are separate.**
  - `Mutator` holds the allocation buffer, SSB, event word, tick, root stack, safepoint state and a pointer
    to the current thread.
  - `GreenThread` holds its register stack and frames, re-entry and escape fields, its dynamic environment
    (parameterization, the three current ports, handler stack, winds), a `ran_since_gc` bit and scheduler
    links.
  - Under M:1 there is exactly one `Mutator`.
  - `allocs_since_gc`, `gc_threshold`, `gc_pending` and `gc_defer_depth` leave `Heap`
    (`heap/mod.rs:379-417`). No runtime state goes in `thread_local!`.
- **N-ready now, at zero single-thread cost:** per-mutator SSBs (never a shared log); a block pool behind
  an uncontended `Mutex`; a safepoint protocol written as request → acknowledge (immediate with N = 1) →
  collect → release; no-op `enter_safe_region`/`leave_safe_region` around blocking I/O and the future FFI;
  one store funnel (where a threaded build adds the publication fence); a `Slot` accessor that becomes
  relaxed `AtomicU64`, and a disarm that becomes `ldclrb`, under `threaded`; one interning function;
  one-word inline caches; and a **two-mutator deterministic test mode** on one OS thread (§14).
- **Green threads on top** (SRFI 18, VM first; threads-rec §6): thread, mutex and condition-variable
  objects are heap objects and the thread table is a root; each thread's register stack is a root scanned
  from its own watermark, and minors skip threads that have not run since the last GC; parameters are
  deep-bound (stage 4f). Blocking inside a Rust re-entry raises an error until those paths become machine
  frames (threads-rec §1.5).
- **Deferred:** real N carriers, atomic marking, handshakes, concurrency lanes (TSan, loom, Miri), and
  freeze-and-slide green-thread stacks (C′). Until C′, a green thread's stack reservation is 256 MiB, which
  bounds deep recursion inside green threads; that limit will be documented.

---

## 13. Heap sizing, pacing and observability

**Triggers are in bytes**, counted at refill (§5) and for LOS allocations, plus **external bytes**: ports
(8 KiB each), code and macro bodies (their Rust size), tree-walker environments (240 B each) and
string-port buffers. This follows Chez phantom bytes and OCaml `caml_alloc_custom_mem` (DIGEST pitfall 4).

**Nursery (generational heaps).**
- The minor budget starts at **4 MiB**, which is about 150 K objects at the measured 26.9 B mean. That is
  inside the 64 K–256 K-object sweet spot (demographics §9.1); OCaml uses 2 MiB and Chez 8 MiB.
- It is then sized to keep the minor interval at least 2 ms of mutator time:
  `budget = clamp(rate × 2 ms, 2 MiB, 32 MiB)`. A JIT allocating 0.3–4 GB/s then gets 8–32 MiB
  automatically.
- **Adaptive bypass** (Lua 5.5-style hysteresis, whippet §4.1):
  - if minor survival (promoted bytes ÷ budget) exceeds 30% for 3 consecutive minors, switch the heap to
    *major-only* until a major frees at least 50% of the bytes allocated since the previous major. That
    covers queue3 and deeprec, which have 100% survival while they build (demographics §5.1);
  - in major-only mode the barrier stays armed, and an SSB overflow simply forces the next major.
- **Forced major**: after 256 consecutive minors, or when allocation since the last major exceeds 64× the
  last live size. Larceny's `ephemeron` suite needs a major within about 100 M pair allocations; 1.6 GB at
  a 4 MiB budget is 400 minors, so the 256-minor rule fires (finalization §4.6).

**Major target (MemBalancer-style, applied only at majors).**
- With L = live bytes after the major, g = the smoothed promotion-plus-direct-old allocation rate, and
  s = the smoothed marking speed (bytes/s):
  `E = c·√(L·g/s)` and `target = L + clamp(E, 0.25 L, 3 L)`, never less than `L + free_reserve`.
- Whippet's adaptive sizer uses the same form (whippet §1.10). MemBalancer reported 16% less memory at
  equal GC time (Kirisame et al., OOPSLA 2022).
- Smoothing is an EWMA with α = 0.5. The target changes only with more than 10% hysteresis, because of
  Wingo's "hyperactive squirrel" warning.
- `c` is calibrated in stage 5 so that the GC benchmark set lands near today's effective 2× live; the
  owner's footprint goal (decision #15) can move it.

**Free reserve and the anti-livelock rule.**
- After a major the heap must hold `max(8 blocks, 5%)` empty blocks, or it grows.
- Two consecutive majors that each free under 1% of the heap raise the target ×1.5. This is Wingo's
  livelock fix.
- Fragmentation is computed every major and reported. Once stage 7 exists it drives evacuation.

**Decommit.** See §4. Blocks beyond `free_reserve + 1.0 × L` are released at majors.

**Observability (stage 0 onwards).** Per collection, `GcStats` records the kind; phase timings (roots,
mark, weak fixpoint, LOS sweep, catch-up, finalize); bytes allocated, promoted and freed; the live
estimate; free/recyclable/full blocks and fragmentation; SSB entries, pinned objects, mark-stack peak and
watermark depth. Cumulatively it keeps the GC-time fraction, the pause histogram (p50/p95/p99/max) and an
**MMU log** of `(start, end)` per pause, reported at 1, 10 and 100 ms windows (Larceny `gc_mmu_log.c`).
`PATINA_GC_LOG=path` writes CSV, `(gc-stats)` gains bytes, pauses and MMU, and tracepoints sit behind a
feature. Today `last_pause_micros` is computed and never read (gc-impl §3.5).

---

## 14. Testing and verification

**Collectors as tests.** `NullGc` is the reference run. `TortureGc`:
- moves every unpinned object on every GC;
- `mprotect`s from-space and fills it with poison;
- zeal: collect at every poll, or every N refills.

`MarkRegion` has zeal knobs: `PATINA_GC_STRESS=n` collects every n refills;
`PATINA_GC_ZEAL=minor|major|alternate`.

**Heap verifier** (debug builds, after every GC under zeal, and on demand). It walks every live object
from the roots and checks:
- every field value points at a granule whose state is the current epoch (or FORWARDED in torture
  from-space, which must not be reachable);
- END bits match the computed sizes;
- invariant W holds for pointer-bearing kinds;
- every old-object granule that holds a young reference has LOG clear and appears in the SSB;
- no frame below the watermark holds a young reference.
- **Debug allocation bitmap**: debug builds write a start bit at allocation (the Whippet scheme), so the
  verifier can also walk dead young objects. Release builds skip it (K5).

**Poison.** Debug builds fill freed holes with a poison pattern at sweep, instead of zeros, and do
debug-only checks in accessors. A stale reference then panics instead of silently reading reused memory
(today's release run printed `(45294)`).

**Miri** runs on `patina-gc` (`mmap` shim, strict provenance); the Rust twins of the inline fast paths
are the reference for testing JIT emissions later (rust-gcs §4.7).

**Differential lanes**, extending `scripts/run_gc_differential.sh` (chibi suite, `EXPECTED_TOTAL=1226`):

| Mode | Builds | Backends |
|---|---|---|
| null | release, debug-poison | both |
| default | release, debug-poison | both |
| stress (every 16 refills) | release, debug-poison | both |
| sticky-gen zeal | release, debug-poison | VM |
| torture (stage 7 onward) | release, debug-poison | VM, with tree-walker pins honoured |

The reclamation proofs are kept. Port-finalization tests run outside the lane.

**Fuzzing** generates Scheme programs over mutation, `call/cc`, `dynamic-wind`, ephemerons and parameters,
run under zeal against `NullGc` (Wasmtime's `gc_ops` model). **Two-mutator mode** alternates two
`Mutator`s on one OS thread, exercising buffer refill, per-mutator SSB flushes and N-way acknowledgements.

**GC benchmark set (GBS-28)** plus metrics:
- the 20 workloads from demographics §2 (reproduction: `PRD/study/gc/probes/workload-demographics/instrumented/REPRODUCE.sh`). The Larceny-derived
  programs are LGPL and **not vendored**; they run from `~/Project/reference/larceny`, as
  `run_larceny_tests.sh` does. Its three Patina-authored extras (`deeprec`, `libload`, `eqtable`) are
  vendored in `bench_programs/gc/`;
- 8 new vendored probes: `samedepth1000`, `escape1000`, `pingpong1000`, `ctakdeep`, `abort100`
  (cont-repr §2.1), `frag-mix` (16 B and 48 B objects with interleaved lifetimes, Wingo's livelock
  shape), `ephem-chain-16k` and `port-churn` (100 k unclosed opens under `ulimit -n 1024`);
- metrics: wall time, peak RSS, GC-time fraction, pause p50/p95/p99/max, MMU at 1/10/100 ms;
- method: **interleaved A/B, main/branch/main**, at least 5 rounds, reporting the median and spread (the
  GC_STAGE5 preamble). `NullGc` serves as the lower bound for "GC cost" (Epsilon/LBO methodology, hotspot
  §4).

**How each stage proves itself**: §15 lists, per stage, the lanes, matrices and GBS deltas required.
Every stage keeps both chibi suites green, the 64-row control-flow matrix, the hygiene matrix (139
shapes), the suite oracles, both Larceny lanes and `patina-compat check-smoke`.

---

## 15. Migration plan

Rules applied to every stage:
- **an issue first** (with the defect narrative), and the PR closes it;
- `docs/GC_DESIGN.md` is rewritten in place (new markdown files need owner approval);
- behaviour changes are measured against chibi and Gauche and recorded in `DIVERGENCES.tsv`;
- every performance claim is interleaved A/B on GBS.

Effort is in focused engineer-weeks [I].

| # | Stage | Scope; crates and files | Acceptance | Effort | Value on its own |
|---|---|---|---|---|---|
| 0 | **Ground truth** | Benchmark harness, GC log, issues for present-day defects (DIGEST §1.11: embedder use-after-free, interpreter-drop leak, port `eq?`, `EMFILE`, rebinding divergence, docs drift). `core:heap/gc.rs`, `scripts/`, `bench_programs/gc/` | No behaviour change; all lanes green; harness reproduces the demographics baseline within noise | 2–3 | A pause and MMU baseline; the issue queue exists |
| 1 | **Byte-aware pacing in today's collector** | `note_alloc(bytes)`, payload bytes for vectors, strings, bytevectors, bignums and continuation snapshots; external-bytes API; descriptor pressure plus `EMFILE` retry. `core:heap/{mod,gc}.rs`, `prim:io/ports.rs`, `vm_state.rs` | 500 × `(make-vector 100000)` peaks under 100 MB (today 414 MB, 0 collections); gcold RSS ≤ 3× its 12 MB live (today 628 MB); `samedepth1000` bounded; port-churn completes (today fails at 1,021); GBS geomean within ±1% | 2–3 | Fixes the measured blow-ups now; R3 |
| 2 | **Root and store contract** | Slot-based `RootVisitor` plus open `RootSet`; `RootScope`/`Handle`; `CallFrame.closure` as a value; rooted loading boundaries A, B, D and E (`ParsedLibrary` body in the registry, `globals_stack`); tree-walker roots via `pinned`; store funnel for records, parameters and promises; `vector_slice_mut` deleted. Crates: core, vm, runtime, interpreter, tree-walker | Embedder probe passes in debug and release; rbtree load peak ≤ 130 MiB (today 211); differential lanes, matrices and ephemeron tests green; GC microbenchmarks ±1% | 5–7 | Library loading collects between forms; the use-after-free is fixed; every mutation is visible |
| 3 | **`Cx` context API** | `Mutation`/`Cx` receivers replace `&SharedHeap` (codemod over ~1,300 sites, crate by crate: primitives, frontend, macros); a heap-owning machine; `Instance` split; `Environment`, `CompiledMacro` and `Parser` stop owning the heap | Mechanical; per-crate PRs; GBS geomean ≥ 0% (the `RefCell` borrow disappears); all lanes | 6–8 | Removes a borrow per heap operation and the borrow-discipline rule; prerequisite for raw access |
| 4 | **Drop-free, canonical object model** (still on today's arenas) | 4a canonical RTDs, primitives, records and ports; port table plus finalization registry (R1–R6). 4b **global cells, variant R** (immortal `CellSpace`; delete `FORWARDED`/`Owner`/`links`; `Library.exports` → `CellRef`; code carries its namespace; `VmClosure.globals` deleted). 4c identifiers with symbol ids and interned scope-set ids; delete `SourceMap.locations`, child spans and the throwaway per-expansion `SourceMap`. 4d **continuations design A**, `Copy` frames, non-relocating register stack, CodeDescs with traced liveness, sparse safepoint maps. 4e tree-walker host-payload tables. 4f inline payloads for records, parameters (deep-bound), promises and cells; macro literal vectors | 4a: E4 → `(#t #t #t #t)`, E1/E5 tests added; 4b: all 6 rebinding tests unchanged under R, `import_modifiers.rs` (190 runs) green; 4c: big load peak −26 MiB or more; 4d: 64-row matrix and `escape_from_primitive.rs` green on both backends, capture at d=1000 ≥ 2× cheaper, `finished_forms_release_code.rs` green; 4e/4f: Drop-carrying allocations ≤ 1% (census); heap freed on interpreter drop | 10–14 | `eq?` becomes one compare; port semantics match the oracles; the teardown leak is gone; continuations are byte-visible and cheaper; the post-load sweep stops dropping millions of `Rc`s |
| 5 | **New heap, non-moving, non-generational** | New crate `patina-gc`: reservation, metadata, SOS, LOS, cell/symbol space, `Mutator`, `NullGc`, `MarkRegion` (whole-heap), verifier, `declare_layouts!`. **Kind-by-kind migration**, each PR green: 5a crate only (unit tests plus Miri); 5b pairs into blocks (tag `011` becomes an address; the composite marker handles arenas and blocks); 5c closures (tag `110`, headerless) and all headered fixed-size kinds (tag `111`, told apart from arena indices by magnitude: indices are below 2^35, reservation addresses above); 5d vectors, strings, bytevectors and bignums inline, plus the LOS (tags `100`/`101` retired); 5e delete the arenas and old collector, add pacing and decommit; 5f re-tag immediates and add **self-tagged flonums** (pin-on-hash is active from 5b) | Per PR: all lanes in null, default and stress, both backends. Stage exit: GBS geomean ≥ 5% faster than the stage-4 baseline; peak RSS ≥ 30% lower on libload, gcold and queue3; max pause on queue3 under 15 ms (today 41) and the post-load major under 10 ms (today 178); float workloads ≥ 1.3× after 5f (K6) | 12–16 | The representation win: about 2× fewer bytes; bump allocation; no `malloc` per vector or closure; pauses proportional to live data; memory returned |
| 6 | **Sticky generations (switch)** | 6a barrier armed with minors off (M2 and M5's "barrier only"); 6b sticky minors, SSB, watermark, young lists for weak data and the registry; 6c adaptive bypass; **M5 decides the default** | M2 interpreter barrier tax under 1% (K2); M5: generational must win geomean throughput or cut p95 pause ≥ 30% at ≤ 1% throughput cost (K1); sticky-zeal lane green; two-mutator mode green | 5–7 | Minor pauses proportional to survivors; old data traced at majors only |
| 7 | **Opportunistic evacuation (candidate B)** | `TortureGc` lane on the VM; fragmentation-triggered evacuation; optional young-survivor evacuation (gated) | Torture lane green on the VM; `frag-mix` steady-state committed ÷ live ≤ 1.4; K3 and K7 measured | 5–8 | Fragmentation cured; moving proven; the copying-nursery option opens |
| 8 | **JIT ABI freeze** | `RuntimeAttrs` and `Mutator` offsets documented in `docs/GC_DESIGN.md`; interpreter polls at calls; conformance tests that read the attrs and emit the Rust twins | Interpreter poll cost ≤ today's; attrs conformance tests | 2–3 | A Cranelift tier can start without touching the GC |
| 9 | *(optional)* **C′ continuations** | Frozen watermark, chunks, lazy thaw, persistent dynamic-state lists | 64-row matrix on both backends; same-depth capture ≥ 20× cheaper at d=1000 | 6–10 | O(1) capture; segmented green-thread stacks |

Stages 0–8 total **51–75 engineer-weeks (≈ 12–17 months)**. Most of it is the common core, which every
candidate pays (immix-mmtk §10: "the representation rewrite is the dominant cost either way"). The
collector proper is about 4–6 kLOC through stage 6, plus about 2–3 kLOC for stage 7.

**The first three PRs.**
1. **PR-1: "GC benchmark set, pause/MMU logging, and the measurement doc"** (stage 0; issue: "No pause or
   MMU measurement exists for the collector").
   - Adds `bench_programs/gc/` with the Patina-authored workloads above (no Larceny files) and
     `scripts/gc_bench.py`: interleaved main/branch/main, medians over N rounds, RSS via
     `/usr/bin/time -l`, GC CSV ingestion.
   - Adds phase timings (mark, weak, sweep) to `GcStats` in `core:heap/gc.rs`, the `PATINA_GC_LOG` CSV
     writer, and pause fields in `(gc-stats)`.
   - Fixes the stale `gc.rs:25` comment.
   - *Accept*: no behaviour change (differential lanes, chibi suites and matrices green); the script
     reproduces demographics §3's on/off table within ±5% for the vendored workloads.
2. **PR-2: "Count bytes, not objects, in the collection trigger"** (stage 1; issue: "500 ×
   (make-vector 100000) never collects; gcold reaches 628 MB with 12 MB live").
   - `Heap::note_alloc(bytes)`. Every `alloc_*` charges slot plus payload bytes.
   - `MarkSweepCollector::auto_threshold` uses marked **bytes** (`max(4 MiB, 2 × live_bytes)`).
   - A continuation snapshot charges its register and frame bytes when created.
   - *Accept*: the PR-1 harness shows the make-vector loop under 100 MB and gcold RSS ≤ 3× live; GBS
     geomean ±1%; all lanes.
3. **PR-3: "Slot-based root visitor and open root registration"** (stage 2a; issue: "`GcVisitor::visit`
   takes values by copy, so no root can be updated, and roots are a closed array").
   - `RootVisitor` with `slot`, `slots`, `pinned`, `code`, `host_payload`. Every `GcRoots` impl ported:
     `VmState`, `LibraryRegistry`, `StepTracer`, and the tree-walker's `Evaluator`, `EscapeRoots` and
     `StepRoots` (tree-walker structures report `pinned`).
   - `CallFrame.closure: Value`. `RootSet::register` replaces the closed array (`cps_eval/mod.rs:130`).
   - The collector is unchanged in behaviour.
   - *Accept*: no behaviour change; GC microbenchmarks ±1%; all lanes. Unit tests prove registered roots
     are traced and that `slot` visits can rewrite the slot (a test visitor rewrites and restores).

PR-4 roots the loading boundaries, which delivers the load-memory win. PR-5 adds the embedder `Handle`.
PR-6 is the store funnel.

---

## 16. Risks, mitigations, and kill criteria

| # | Risk | Mitigation | Kill criterion (measured on GBS, interleaved A/B) → change of course |
|---|---|---|---|
| K1 | Generational loses, as in Wingo's nboyer and splay results and Hudson's Go result | A per-heap switch; adaptive bypass; watermark | If sticky-gen vs whole-heap is worse than −1% geomean throughput **and** p95 pause is not ≥ 30% better → whole-heap becomes the default; generational stays opt-in for one release, then is **deleted** unless a GBS workload gains ≥ 5% |
| K2 | Barrier tax in the interpreter or JIT | One-bit filter; static elision; `letrec*` unboxing (jit-ready R13) removes most `WriteCell`s | M2 above 1.5% → profile; above 3% → switch to the card + young-filter runner-up, which invariant W already makes sound (one module plus the `BARRIER_KIND` attr) |
| K3 | Fragmentation before stage 7 | Free reserve; medium overflow; recyclable threshold; livelock rule | Committed ÷ live above 1.6 at steady state on any GBS workload, or any livelock on `frag-mix` → **move stage 7 ahead of stage 6** |
| K4 | Sticky-mark nursery loses to copying | Empty-block fast path; word-parallel sweep | After stage 6, if lazy sweep plus refill exceeds 4% of mutator time on any churn workload (deriv, destruc, fibfp, mbrot, generator, ctak, fibc, libload), or a prototype bump nursery reclaims ≥ 2× more bytes per GC-ms → build the `NurserySpace` (§11). Same barrier and ABI; needs the stage 7 prerequisites |
| K5 | END-at-mark-time (no allocation-time metadata) proves bug-prone | Verifier; debug allocation bitmap; torture | Two verifier escapes reaching `main`, or mark time +3% → revert to Whippet's allocation-time begin/end writes (+2 stores on the fast path) |
| K6 | Self-tagged flonums cost more than they save | Canonical encoding; one numeric dispatch | Non-float GBS geomean −1% or worse, or float workloads gain under 1.3× → revert to 16 B boxes (only possible before the stage 8 freeze) |
| K7 | Pin-on-hash blocks evacuation | — | Pinned bytes above 10% of live on eqtable-like workloads and under 50% of candidate blocks reclaimed → Lilliput hash words for headered objects (header bits 11–12); pairs keep pin-on-hash |
| K8 | Watermark cost on `Return` | Compare only, with the store on a cold branch | Above 0.5% on fib, tak and nqueens → enable only for heaps whose last-GC stack depth exceeded 10 K frames |
| K9 | B1 is wrong: representation does not pay | Stage 5 is kind-by-kind and can stop | At stage 5 exit, under 5% geomean gain **and** under 30% peak-RSS gain on the memory workloads → stop; re-plan around candidate C (copying nursery over a mark-in-place old space) |

Risks without a course-changing threshold, and their mitigations:
- **The migration is too large to keep the tree green.** Every stage lands on its own, and `NoGcScope`
  plus transitional tag decoding make each step reversible.
- **Single-mutator shortcuts creep in.** The threads-rec rules go into `GC_DESIGN.md`, the two-mutator
  mode runs in CI, and new `thread_local!`s and heap payloads are reviewed.
- **GC-time port finalization makes differential lanes flap.** Its tests live outside the lane.
- **Raw addresses turn rooting bugs into undefined behaviour.** Debug poison, the verifier, torture
  `mprotect` and Miri.
- **Reservations run out with many heaps on Linux CI.** `MAP_NORESERVE`, a configurable reserve, and a
  many-heaps test in CI.

---

## 17. Owner decisions

These are the DIGEST §6 items this design depends on. For each: the default I propose, and what happens if
the owner chooses the alternative.

| # | Decision | Proposed default | Consequence of the alternative |
|---|---|---|---|
| 1 | Tree-walker | *(answered)* keep it, lagging: whole-heap, non-moving, pinning roots, host-payload tables | — |
| 2/3 | Rebinding semantics; when `define` rebinds | **R first** (no semantic change, stage 4b); recommend C afterwards (chibi, Chez, Racket) with `DIVERGENCES.tsv` rows | C deletes `GlobalCacheEntry`, shadow bits and per-site caches, and lets JIT globals be one load; it flips 6 pinned tests. The GC is unaffected either way, since cells are immortal under both |
| 4 | Value encoding | **61-bit fixnums; self-tagged flonums (offset 1, tags 001/100/101); raw tagged addresses** | NaN-boxing breaks 61-bit fixnums; boxed-only flonums keep 19.8% of allocations as 16 B boxes; offsets add a base add per access |
| 5 | Strings and bignums | UTF-32 inline; bignums as inline limbs with `num-bigint` temporaries | UTF-8 halves string bytes but changes `strings.rs`/`characters.rs` (252 mentions); `num-bigint` payloads would need finalization |
| 6 | Pauses | *(answered)* throughput first; STW; targets: minor p99 ≤ 5 ms, major ≈ 3 ns per live object + 1 ms | An MMU target would add incremental marking through `barrier_mode` (§11) |
| 7 | Shared-memory parallelism | **Not a goal now**: M:1, with N-ready interfaces (§12) | M:N becomes a separately approved program (estimated 4–9 months after §12); nothing in this design needs undoing |
| 8 | JIT frame model | **S1 baseline** (VM frames, no stack maps); S3 later | S2 needs unwinding and a depth cap; S3 first needs stack maps and deoptimization on capture |
| 9 | Async interrupts | **Yes**: the tick poll at fragment entry and loop headers | No → non-allocating, non-calling loops need no poll (cranelift-gc §4) |
| 10 | Identity hashing | **Address hash, pin on first hash** | Lilliput words (K7) cost header bits and a growth-on-move path; SRFI 254 transport cells move the burden to Scheme |
| 11 | Weakness scope | SRFI 124 + 254 ephemerons; SRFI 125 weak tables via ephemerons; guardians after stage 6 | Guardians earlier add a fixpoint step and a per-generation list (cheap) |
| 12 | Port finalization | **Flush and close at GC, `EMFILE` collect-and-retry**, GC timing declared observable | Without it, R2 and R3 stay divergent from chibi and Gauche |
| 13 | Embedding API | Owned `Handle` plus scoped `with`; mandatory teardown; many heaps | Scoped-only is simpler but blocks long-lived host references |
| 14 | Dependencies | **No MMTk**; `patina-gc` depends only on `libc` | An MMTk spike needs a one-interpreter-per-process test mode and crate downloads |
| 15 | Footprint | Target ≈ 2× live (calibrated `c`); 16 GiB reservation; decommit beyond 1× live plus reserve | Tighter targets cost GC time along the MemBalancer curve |
| 16 | Front-end sequencing | Stage 4c deletes unread provenance stores and makes identifiers Drop-free; lazy scope propagation is separate | Doing it first removes most of the 96.7% copy share of load-time allocation |
| 17 | syntax-case | Independent; with `ExpansionContext` roots, transformer calls become safepoints and, because nothing moves, raw-bits memos stay valid | — |
| 18 | Continuation end state | **Design A** (stage 4d); C′ optional after the JIT | C′ earlier: O(1) capture sooner, at medium-high risk to the matrix |
| 19 | Divergences | Record the port `eq?` fix (it removes a divergence) and GC-observable port flushing | — |
| 20 | JIT code memory | CodeDesc finalization frees machine code, via a per-unit code allocator (`JITModule` cannot free single functions) | Never freeing it grows long REPL sessions |
| 21 | Budget order | **Representation first**, in landable stages | Collector-first builds an algorithm on 72 B slots and `Rc` payloads, and still has to do the rewrite |
