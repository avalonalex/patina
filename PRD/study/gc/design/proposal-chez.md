# Proposal C: a Chez-faithful hybrid collector for Patina

Panel submission, 2026-10-01. Repo `main` at `28a94f8`, read-only. Philosophy assigned: **Chez-faithful hybrid**
(DIGEST candidate C). Citations use the DIGEST keys (`[demographics §5.1]`, `[chez §7]`, …); repo paths are
crate-relative as in the DIGEST (`core:`, `vm:`, `prim:`, `tw:`, `fe:`, `rt:`). **[V]** = verified in source or by a
cited measurement; **[I]** = my inference or design choice. Where I depart from Chez I say so and give the evidence.

---

## 1. Thesis and key bets

**Thesis.** Patina should adopt the storage-manager *discipline* of Chez Scheme and Racket CS, the most heavily
exercised collector for a Scheme with multi-shot continuations: allocation never collects; collection happens only at
cheap polled safepoints where every live value sits in VM-owned memory; all types are bump-allocated into one young
space, where a dead object costs nothing; promotion sorts survivors into typed old spaces; old space is marked in place
and only blocks that a previous mark found under ¾ live are evacuated (Chez/Racket CS `use_marks`); the boot image goes
to an immortal static space; code never moves; one declarative layout spec generates every traversal (`mkgc.ss`). The
proposal adapts that discipline to Rust and Cranelift where Patina's measurements demand it:

- raw tagged addresses inside one virtual-address reservation per heap;
- a **field-logging armed-bit barrier** in place of Chez's store buffer and cards, because of Patina's measured store mix;
- **immediate (self-tagged) flonums** in place of boxes, because a baseline JIT that keeps values in VM registers cannot
  unbox;
- continuations as **immutable heap chunks frozen out of a fixed live register stack** in place of sealed heap stack
  segments, because the JIT wants a stack that never relocates;
- **two generations plus static** in place of five, because promote-on-first-survival wastes little.

The payoff is concentrated where Patina allocates. Closures, cells and flonums are 60% of allocations and survive at
≤0.7% [demographics §5.1]; 99.7% of library-load allocation is garbage [libload §0]. A copying nursery reclaims all of
that for free, and it removes the per-slot sweep, the free lists and the 72 B slots, which cost 1.1–3.9 swept slots per
allocation today [demographics §5.2].

**Key bets.** If any of these is wrong, the design is in trouble.

| # | Bet | If wrong |
|---|---|---|
| B1 | **"Allocation never collects; collect only at polls" carries from the interpreter to a Cranelift JIT unchanged** (Chez proves it with a moving generational GC [chez §4]) | Every Rust site holding a value across allocation (230–890 functions) needs rooting; the design collapses into the OCaml/V8 model every report rejects |
| B2 | **The whole moving-prerequisite set (§15 stages 1–3) can be finished**: every off-heap `TaggedValue` holder becomes updatable, pinned, or a heap object; nursery objects become Drop-free | No copying is possible; fall back to a non-moving configuration of the same heap (kill criterion K8) |
| B3 | **The young generation pays on Patina's workloads after the compiler improves** (bimodal survival today [demographics §5.5]) | Generational loses (Wingo's nboyer/splay result [whippet §1.6]); ship the major-only configuration (K1), which this design builds first anyway |
| B4 | **A field-logging barrier with static parity costs ≤4 aarch64 instructions on the heap-value path and <1% in the interpreter** [barrier-res §7.2] | Switch to card marking with a young filter. Promoted old blocks are already type-segregated and parseable; in-place blocks would need a 0.78% object-start bitmap (K2) |
| B5 | **The frozen tag ABI is right**: 61-bit fixnums, self-tagged flonums, 16-B-aligned raw addresses with 4-bit heap tags | It is expensive to change once JIT code exists; K5 reverts flonums to 16 B boxes before the freeze |
| B6 | **Mark-in-place with sparse evacuation bounds fragmentation without hole-reuse allocation** (Chez keeps holes until a block empties or is evacuated [chez §7]) | Add Immix line reuse to old blocks: the metadata already allows it |

---

## 2. Value encoding

One 64-bit word. Every heap object is **16-byte aligned**, so a heap address has four zero low bits, and the fourth bit
becomes a second heap-tag bit (Chez's alignment rule, used here for one extra tag bit).

```
 bits 63 ............................................. 4  3  2  1  0
 fixnum      [ signed 61-bit n                              ][ 0  0  0 ]      n<<3
 immediate   [ payload (56)              ][ kind (5)        ][ 0  1  0 ]      specials, chars, headers
 flonum      [ rotl(bits(d) + K, 4)                         ][ 1  0  0 ]      self-tagged double
 (reserved)  [ never produced                               ][ 1  1  0 ]
 heap ref    [ address bits 63..4 (16-B aligned)  ][ k  k  k  1 ]             8 heap kinds
```

**Heap kinds** (low 4 bits; bit 0 = 1 means "heap reference"):

| Low 4 bits | Kind | Word 0 of the object | Notes |
|---|---|---|---|
| `0001` | pair | car (no header) | Chez pair; 16 B |
| `0011` | procedure | raw pointer to a code descriptor | Chez closure: VM closures, primitives, parameters, continuations, tree-walker lambdas |
| `0101` | vector | header | `vector-ref` reads the length from the header |
| `0111` | typed object | header | everything else |
| `1001` | string | header | pointer-free |
| `1011` | symbol | header | lives in the non-moving symbol space |
| `1101` | bytevector | header | pointer-free |
| `1111` | record | raw pointer to its RTD (descriptor space) | Chez record; no header |

**Immediate kinds** (`010`, bits 3–7): 0 = special constants, 1 = char (`(cp << 8) | 0x0A`), 31 = object header (never
a Scheme value), 2–30 reserved. Special constants are `(code << 8) | 0x02`:

| Code | Constant | Code | Constant |
|---|---|---|---|
| 0 | `#f` = `0x002` | 6 | `UNBOUND` (global cell not yet defined; never handed to Scheme) |
| 1 | `#t` | 7 | `BWP` (broken weak pointer or ephemeron) |
| 2 | `()` | 8 | **`FORWARD`**: forwarding sentinel in a forwarded pair's car |
| 3 | eof | 9 | `POISON` (debug fill for freed and from-space memory) |
| 4 | unspecified | 10 | `HOLE` (uninitialised register or delivery hole) |
| 5 | default-object (SRFI 89) | 11–255 | reserved |

**Flonums (deviation from Chez, which boxes).** A double `d` with bits `u` is encoded `w = rotl(u + K, 4)` with
`K = 128 << 52`. The low 3 bits of `w` are then the top 3 bits of the biased exponent plus 128, and the sign moves to
bit 3. `w & 7 == 0b100` exactly when `|d| ∈ [2^-127, 2^129)`, for both signs. I checked the arithmetic: 1.0, −1.0,
2^128 and 2^-127 encode to tag `100`; 2^129 and 2^-128 do not. Decoding is `ror(w, 4) − K`.

Values outside that range use a 16-B `FLOBOX` typed object: 0.0, −0.0, ±inf, the canonical NaN, subnormals, extreme
exponents. The five canonical values are preallocated in static space, so producing 0.0 never allocates.

Costs on aarch64: encode is `add; ror; and; cmp; b.ne` (5 instructions); decode is `ror; sub` (2). This follows
Melançon, Serrano and Feeley (OOPSLA'25), who report 2.3× on float-heavy Scheme benchmarks [heap-repr §11]; their
baselines already bump-allocate boxes [I]. One tag covers a smaller range than their three-tag variant. That is the price of making
"is a heap reference" a single bit. The range covers float32 magnitudes in both signs, and zeros are static boxes.

**Heap reference form: raw addresses (Chez), not per-heap offsets.** Reasons:
- a field load is one instruction, `ldr x, [v, #(8i − tag)]`; `car` is `ldr x, [v, #-1]`;
- `eq?` is one compare;
- non-moving objects (cells, descriptors) can appear as immediates in machine code;
- each heap owns one reservation (§4), so a debug build can bounds-check any reference.

Wasmtime-style 32-bit offsets buy sandboxing, which Patina does not need, and density, which 61-bit fixnum fields
forbid [rust-gcs §4.6].

**Why this layout:**
- **Fixnum tag `000`.** Tagged `adds`/`b.vs` does 61-bit overflow detection directly [jit-ready R1].
- **`#f` is a single compare** (`cmp x, #2`).
- **"Is heap" is bit 0.** The barrier value filter is one `tbz` [barrier-res §4.1].
- **The forwarding sentinel cannot be a value.**
  - Pairs: car ← `FORWARD`, cdr ← new reference.
  - Every other kind: word 0 ← new tagged reference. A live word 0 is a header (low bits `010`) or a 16-B-aligned raw
    descriptor or RTD pointer (low bits `0000`), so bit 0 = 1 is unambiguous.
- **Headers decode as immediates.** A stray header read as a value is a harmless non-reference.
- **Wider fixnums are not worth it.** 4-bit heap tags with 61-bit fixnums cost 16-B alignment. Section 3 shows the
  padding is paid back, because closures and records lose their headers.

---

## 3. Object model

### 3.1 Header word (vectors, strings, symbols, bytevectors, typed objects)

```
 63                                         16 15 14 13         8 7        3 2    0
 ┌─────────────────────────────────────────────┬──┬──┬────────────┬──────────┬──────┐
 │ length / subtype payload (48 bits)          │ R│IM│ subtype (6)│ 11111    │ 010  │
 └─────────────────────────────────────────────┴──┴──┴────────────┴──────────┴──────┘
```

- `IM` (bit 14): immutable literal; mutation raises.
- `R` (bit 15): reserved, e.g. "RTD carries an unboxed-field mask".

**No GC state lives in the header.** Mark, log, hash and pin bits are all side metadata (§4). The mutator therefore
never does a read-modify-write on GC bits (threads-rec §2.12), and the header is immutable after initialisation except
for the collector's forwarding overwrite.

### 3.2 Layouts

All sizes are rounded up to 16 B. "Today" is from [heap-repr §3].

| Object | Ref tag | Layout (words) | Bytes | Today |
|---|---|---|---|---|
| pair | `0001` | car, cdr | 16 | 16 (arena) |
| flonum | imm `100` | — | 0 | 72 |
| flonum box | `0111` FLOBOX | hdr, f64 | 16 | 72 |
| bignum | `0111` BIGPOS/BIGNEG | hdr(len = limbs), u64 limbs | 8+8n | 72 + `Vec` |
| ratnum | `0111` RATNUM | hdr, num, den | 32 | 72 + 2 `Vec` |
| complex | `0111` COMPLEX | hdr, re, im (parts usually immediate) | 32 | 3 × 72 |
| vector | `0101` | hdr(len), elements | 8+8n | 24 + malloc |
| string | `1001` | hdr(len), UTF-32 units | 8+4n | 24 + malloc |
| bytevector | `1101` | hdr(len), bytes | 8+n | 72 + malloc |
| procedure | `0011` | desc*, fv₀ … fv₍ₙ₋₁₎, with n = `desc.nfree` | 8+8n | 72 + malloc + `Rc<Environment>` |
| cell (`MutableCell`, SRFI 111 box) | `0111` CELL | hdr, value | 16 | 72 |
| record | `1111` | rtd*, f₀ … f₍ₙ₋₁₎, with n = `rtd.nfields` | 8+8n | 72 + `Rc<RTD>` + `Rc<RefCell<Vec>>` |
| RTD (descriptor space) | `0111` RTD | hdr, name, parent, nfields, field names, uid, ptr-mask, sealed/opaque | 64 | re-wrapped per call |
| symbol (symbol space) | `1011` | hdr(len = 48-bit name hash), name string | 16 | 72 + `String` key |
| identifier | `0111` IDENT | hdr(payload = scope-set id:24 \| src id:24; the all-ones id escapes to a side table), symbol | 16 | 72 |
| ephemeron | `0111` EPHEMERON | hdr, key, value, gc-link | 32 | 72 |
| promise | `0111` PROMISE | hdr, box → [hdr PBOX, done?, value-or-thunk] (R7RS iterative forcing by sharing the box) | 16+32 | 72 + `Rc` |
| parameter | `0011` | desc(PARAM), converter, global binding | 32 | 72 + `Rc` + `Vec` |
| continuation (procedure) | `0011` | desc(CONT), record | 16 | id into side table |
| continuation record | `0111` CONT | hdr, chain, view depth, winders, handlers, prompts, parameterization, re-entry ids, deliver/flags | 80 | 152 + clones |
| stack chunk | `0111` CHUNK | hdr(len = words), parent chunk, flags, frames… | 24 + frames | — |
| host handle | `0111` HOST | hdr(kind:16 \| id:32), optional slot (e.g. macro literal vector) | 16–32 | 72 + `Rc` payload |
| values | `0111` VALUES | hdr(n), v₀ … | 8+8n | 72 + `Vec` |
| code descriptor (not a value) | code space | hdr, entry\*, code_index:32\|arity:16\|nregs:16, nfree:16\|kind:8\|flags:8, maps\*, unit:32\|gen:32, nested, constants… | 56+8k | `CodeObject` 160 B + `Rc` |
| global binding cell (not a value) | cell space | value, name, meta (DIRTY, WATCHED, defined) | 32 | env slot + `FORWARDED` + `Owner` |
| frame header (register stack or chunk) | — | closure, desc\*, pc:32\|nregs:16\|ret:16, prev-offset:32\|flags:32 | 32 | `CallFrame` 40 B + `Rc` |

**Chez features kept.**
- Headerless pairs.
- The closure's first word is code. In Chez it is the code entry; here it is the descriptor, so a call is a tag test,
  a load and an indirect jump.
- The record's first word is the RTD, with a pointer mask that allows unboxed fields later.
- Continuations are procedures.

**Where size comes from.** The collector reads a procedure's or record's size from its non-moving descriptor or RTD,
which costs two dependent loads during a copy. RTDs and code descriptors are allocated in non-moving descriptor/code
space, so word 0 is a stable raw pointer and never needs updating. This mirrors Chez's static code.

**Byte projection [I].** Applied to the 348 M-object census [demographics §4]:
- closures shrink about 8% against the hypothetical 16 + 8·fv layout (8 + 8·fv, rounded to 16);
- flonums leave the heap (−8.3% of hypothetical bytes);
- 16-B rounding adds 0–8 B to vectors and records.

Net: about 0.9× the hypothetical layout, so about **2.2× smaller than today**. Identifiers go from 72 B to 16 B, or
150 → 35 MiB on the R7RS-large load [libload §6.2].

### 3.3 Policy for Rust `Drop` payloads

**No heap object owns Rust memory.** Rust-owned state lives in per-heap `HostTables`, slabs indexed by a `u32` id:
- `PortTable`: buffered readers and writers, `MemoryFs` writers;
- `TwProcTable` and `TwContTable`: tree-walker `CpsLambda`, `CpsContinuation`;
- `MacroTable`: `CompiledMacro` without its literals and without its `heap` field;
- `LibraryTable`;
- `ForeignTable`: FFI.

The heap side is a 16–32 B `HOST` handle. Each table entry is registered on a **young or old registration list**
(OCaml custom table [finalization §4.1]). The epilogue finalises entries whose handle died (§6.7). Since a dead handle
owns nothing, any kind can live in the nursery, including identifiers (the 92k-per-load case) and closures, which
today hold `Rc<Environment>`.

**Bignums** keep limbs inline in the heap. `num-bigint` remains the arithmetic engine through temporary values. Today
`extract_num_data` already clones on every dispatch [heap-repr §4], so this is no regression.

---

## 4. Heap organisation

### 4.1 Spaces

| Space | Contents | Moves? | Collected by | Per-object metadata |
|---|---|---|---|---|
| **Nursery** (gen 0) | every kind ≤ 8 KiB, mixed (Chez `space_new`) | copied at a minor, or promoted in place if its block is pinned or in sticky mode (§6.11) | minor | granule byte (zero) |
| **Old pair space** | promoted pairs; uniform 16 B, parseable | marked in place; evacuated if under ¾ live at the previous major | major | mark bit, granule byte |
| **Old object space** | promoted pointerful objects (headered, or descriptor-first); parseable, since word 0 is a header or a 16-aligned raw pointer | same | major | same |
| **Old data space** | strings, bytevectors, bignums, flonum boxes; never scanned | same | major | mark bit |
| **LOS** | objects > 8 KiB, in page runs | never | young-LOS list at minor; major | mark bit, granule byte |
| **Static** | everything live after the post-bootstrap compaction (Chez static generation 7) | never | never; persistent boot remembered set | granule byte |
| **Code/descriptor** | code descriptors, RTDs | never | marked at majors; unmarked ones released | mark bit |
| **Symbol** | symbols and their name strings | never | weak symbol table pruned at majors (Chez collectable bare symbols [chez §10]) | mark bit |
| **Cell** | global binding cells, append-only | never; immortal | major root region; minor via remembered set | granule byte |

Chez has 19 spaces × 8 generations; this design has 9 spaces and 3 ages (young, old, static), as the Chez report itself
advises [chez §15 "Avoid"].

### 4.2 Sizes, from Patina's demographics

| Parameter | Value | Justification |
|---|---|---|
| Granule and alignment | **16 B** | 56% of objects are exactly 16 B; a forwarded pair needs two words (Chez); it is the source of the 4th tag bit |
| Block | **32 KiB** | two 16 KiB macOS arm64 pages; Immix block size; a quarter of it is the LOS threshold |
| LOS threshold | **8 KiB** | 148 of 348 M objects exceed it [demographics §4.3]; bounds block-level waste to 25% (Immix) |
| Medium objects (256 B–8 KiB) | separate overflow block when the current block's remainder is under the object's size | Immix overflow allocator; gcold's 816 B vectors |
| Lines | **none** | Chez does not reuse holes: a block is reused when empty or after evacuation. 128 B lines remain the documented fallback (B6), since 99.2% of objects are ≤ 128 B |
| Commit unit | 1 MiB (32 blocks) | Chez 2 MiB chunks; fewer `mprotect` calls |
| LOS page | 16 KiB | the macOS page; waste ≤ 50% only for 8–16 KiB objects |

### 4.3 Metadata layout

These are flat tables at fixed offsets from the heap base, with no Chez-style 3-level radix:
- **Granule byte**: 1 B per 16 B, 6.25% of the block area. Address: `meta_bias + (a >> 4)`.

  | Bit | 0 | 1 | 2 | 3 | 4 | 5 | 6–7 |
  |---|---|---|---|---|---|---|---|
  | Meaning | `LOG0` (word 0 of the granule armed) | `LOG1` | `HASHED` | `HASH_MOVED` | reserved | `EPH_PENDING` | reserved (future incremental marking) |

- **Mark bitmap**: 1 bit per granule (0.78%), separate so it can be `memset` per condemned block.
- **Block info**: 32 B per block (0.1%): space, age, flags (`TO_SPACE`, `EVAC_CANDIDATE`, `IN_PLACE`), saturating
  `pin_count` (255 = ∞, Chez `must_mark`), `live_bytes` (u32), allocation frontier, owning mutator (u16), list link.

That is 7.1% of committed block memory in total. The granule byte is chosen over a dense 1-bit log table (1.56%) for
throughput: it gives the 4-instruction barrier of §7.

### 4.4 Address-space reservation and decommit

- **One reservation per heap.** It is aligned to 1 GiB by over-reserving and trimming, because only 1 of 64 returned
  regions was 4 GiB-aligned [demographics §8.2]. Defaults: 16 GiB of blocks, plus 1 GiB of granule metadata,
  128 MiB of mark bits and 16 MiB of block info, all `PROT_NONE` until committed. 318 live heaps in one test process
  would take about 5.4 TiB of address space. Reserving 64 TiB was measured to work. The block area size is a
  construction-time option, up to 1 TiB.
- **Per mutator:** a 256 MiB remembered-set buffer reservation.
- **Per green thread:** a 16 MiB register-stack reservation with a 1 MiB soft limit. Overflow freezes frames into the
  heap (§10).
- **Heap teardown unmaps all of it.** This depends on the `Rc`-cycle fix in stage 1.
- **Decommit.** After each major, fully free 1 MiB chunks beyond `reserve + nursery target` are released with
  `MADV_FREE`, which returned RSS immediately in the measurement [demographics §8.2]. Here
  `reserve = max(64 blocks, 3% of heap)`.

---

## 5. Allocation

**The contract is kept: allocation never collects** [V `core:heap/mod.rs:581-584`; chez §3].
- The slow path may refill, overflow into new blocks past any soft limit, and raise an event.
- It never runs the collector and never runs Scheme.

This is what lets about 1,300 Rust call sites hold raw `TaggedValue`s across allocation with no rooting
[prim-embed §2]. It also means allocation sites are never JIT safepoints, so every constructor's stores are
initialising stores, which need no barrier.

**Rust fast path (interpreter and primitives):**

```rust
#[inline(always)]
fn alloc_small(&self, bytes: usize /* multiple of 16, ≤ 256 */) -> NonNull<u8> {
    let p = self.ctx.alloc_ptr.get();
    let np = p + bytes;
    if np <= self.ctx.alloc_limit.get() { self.ctx.alloc_ptr.set(np); unsafe { NonNull::new_unchecked(p as *mut u8) } }
    else { self.alloc_slow(bytes, AllocKind::Small) }          // #[cold], never collects
}
```

**JIT fast path, aarch64, `cons`.** `x21` is the pinned `VmCtx` register (Cranelift `enable_pinned_reg`
[cranelift-gc §2.7]).

```
ldr  x9,  [x21, #ALLOC_PTR]
ldr  x10, [x21, #ALLOC_LIMIT]
add  x11, x9, #16
cmp  x11, x10
b.hi Lslow_cold                ; cold block: call rt_alloc_slow(ctx, 16, PAIR), a leaf helper, not a safepoint
str  x11, [x21, #ALLOC_PTR]
stp  x_car, x_cdr, [x9]        ; initialising stores: no barrier
orr  x_res, x9, #1             ; pair tag
```

- That is 6 instructions plus the stores and the tag `orr`, against Chez's 4 with `%ap` in a register [chez §3].
- Between calls, a fragment can keep `alloc_ptr` in an SSA register and write it back before any call (Chez writes
  `eap` through the same way). Each allocation then costs `add; cmp; b.hi`.
- Allocations in one basic block share a single bump (OCaml Comballoc).
- A closure with n free variables bumps `round16(8 + 8n)` and stores the descriptor pointer first.
- `limit` is always the end of the current 32 KiB block, so the fast path needs no carry check. Refills happen about
  once per 1,000 allocations.

**Slow path** (`rt_alloc_slow`, `#[cold]`):
1. `size > 8 KiB`: allocate a LOS page run and push it on the young-LOS list. This is a fallible variant for
   user-sized requests: `make-vector 1e9` returns `&out-of-memory` from the primitive instead of aborting.
2. `256 B < size` and it does not fit: bump in the medium overflow block.
3. Otherwise:
   - retire the current block and record its frontier;
   - take the next nursery block, zeroing its 2 KiB of granule bytes.
   - If the nursery target is exhausted, take a block anyway, set the `GC_MINOR` event and zero `ticks` (§9).
     The overshoot lasts until the next poll.
4. If the block pool is empty, commit the next 1 MiB chunk.
5. If committed bytes exceed the user's maximum heap, set the `OOM` event. At the next poll a major collection runs;
   if the heap is still over the limit, the non-continuable condition `heap-exhausted` is raised there, where raising
   is legal.
6. If the reservation itself is exhausted, abort with a diagnostic, the analogue of `handle_alloc_error`.

**Kinds that never take the inline path:** ports, ephemerons, host handles and continuation records. Their allocation
must register them (tables, ephemeron rules), so they go through runtime helpers [finalization §6.7].

---

## 6. Collection

Collection is stop-the-world on the mutator thread, with no worker threads and a deterministic trace order. Both
collectors are one generic function, monomorphised as `collect::<Minor>` and `collect::<Major>` so that the young/old
tests constant-fold (Chez `gc-011` covers about ¾ of collections [racket-larceny §3.4]).

### 6.1 Minor collection

1. **Rendezvous.** Every mutator is at a poll or in a safe region; with one mutator this is immediate. Retire each
   mutator's allocation block (record frontiers) and flush its remembered-set buffer cursor.
2. **Pinning roots first** (tree-walker heaps, `PinToken`s, debugger payloads). Mark the referent in place and set its
   block `IN_PLACE`. This must happen before anything is copied (Whippet's order [whippet §1.4]).
3. **Precise roots, each slot updated in place:**
   - live register stacks of green threads with `ran_since_gc` set (§8);
   - the control-state heads of each thread (winders, handlers, prompts, parameterization);
   - `pending_escape` and scratch arguments;
   - `RootScope` stacks;
   - the young-handle list;
   - descriptors and cells allocated since the last minor (pre-logged whole objects);
   - young-LOS objects reached;
   - the boot remembered set;
   - **remembered-set entries**: each is a slot address; read it, evacuate a young referent, write back.
4. **Copy (Cheney).** Survivors are sorted by kind into three old to-spaces: pair, object, data (Chez's sort-on-promotion
   [chez §2]). The Cheney scan walks the pair and object to-spaces, which are parseable. Objects in `IN_PLACE` blocks
   are marked and pushed on the mark stack instead of being copied. The loop alternates Cheney scan and mark stack
   until both are empty (Chez's `sweep_generation_pass` [chez §6]).
5. **Arm.** OR `LOG0|LOG1` into the granule bytes of every newly filled range of pointerful to-space, and of every
   `IN_PLACE` block, holes included. Holes are never stored into, so arming them is harmless. Static-space slots taken
   from the remembered set move to the persistent boot remembered set instead of being re-armed.
6. **Young weak processing.** Ephemerons with young keys; young host registrations; the young part of the
   identity-hash moved table (§6.9).
7. **Reset.** Copied nursery blocks return to the pool. `IN_PLACE` blocks become old blocks whose `live_bytes` is the
   sum of marked sizes. Young LOS objects that were not reached are freed.
8. **Policy update.** Survival in bytes, nursery size, sticky-mode switch, major trigger (§6.11, §13).

After a minor no young object exists, so every remembered-set entry can be re-armed and the buffer reset.

### 6.2 Major collection (`(gc)` is always a major)

1. Rendezvous and flush, as in §6.1.
2. **Choose evacuation candidates.** This is the Chez/Racket CS `use_marks` rule [chez §7; racket-larceny §3.3]. An old
   pair, object or data block is a candidate if all of the following hold:
   - its `live_bytes` from the previous major is under ¾ of the block (24 KiB);
   - its `pin_count` is 0;
   - it is not a to-space block of this cycle.

   Blocks never marked before are marked in place first, as in Chez.
3. Clear the mark bitmap of every non-candidate old block, with one 256 B `memset` per block. Nursery blocks are copied
   as in a minor.
4. Pinning roots first (which can veto candidates), then precise roots, then **trace**:
   - a referent in a nursery or candidate block is copied (forwarded) into to-space, while the evacuation budget lasts;
   - a referent in any other old block gets its mark bit set and its size added to `live_bytes`;
   - descriptor, RTD and symbol edges set the mark bits of their non-moving spaces. Descriptor constants are traced as
     ordinary slots.
   - When free blocks drop to the reserve, remaining candidates are marked in place instead (Immix [immix-mmtk §2]).
5. The weak and finalisation epilogue (§6.10).
6. **Block accounting.**
   - Old blocks with `live_bytes == 0` and evacuated candidates are freed.
   - Blocks under ¾ live are flagged for the next major.
   - The LOS list is walked and unmarked objects are freed.
   - Unmarked descriptors are released (§6.8).
   - Free chunks are decommitted (§4.4).
7. **Re-derive the barrier state.** Discard the remembered set and OR `LOG0|LOG1` over every pointerful old block
   (2 KiB per block). This is the metadata pass [barrier-res §7.1 item 7].

### 6.3 Marking order and a bounded mark stack

- The mark stack is segmented in 64 KiB segments, outside the heap budget. Copied objects need no stack, since Cheney
  is breadth-first.
- **Pairs are traced by looping on the cdr and pushing only the car** (deviation from today's car-then-cdr order). The
  stack is then bounded by car-nesting depth, not list length. This fixes the +55 MB measured on a 2 M-element list
  [DIGEST §1.4].
- Vectors over 512 slots push a resumable `(object, next_index)` entry in 512-slot slices.
- Chunks are traced frame by frame through each frame's descriptor and safepoint map.
- Worst-case stack size is O(car nesting + #large vectors in flight), not O(heap).

### 6.4 Evacuation and pinning policy

| Object location | Minor | Major |
|---|---|---|
| Nursery block, no pins, normal mode | copy | copy |
| Nursery block with a pinned object, or sticky mode | mark in place; block becomes old (gen-ZGC/JEP 423 in-place promotion [hotspot §3.2]) | same |
| Old block, previous mark under ¾, unpinned | — | evacuate while the reserve lasts |
| Old block, otherwise | — | mark in place |
| LOS, static, code, symbol, cell | never moves | never moves |

**Pins.**
- `PinToken` (RAII) bumps the block's `pin_count`. It is used for FFI buffers and debugger event payloads.
- Pinning roots pin the block for one cycle.
- Large objects are implicitly pinned.

**Torture mode (`MOVE_ALL`).** Every collection is a major, and every unpinned non-LOS block is a candidate.

### 6.5 Sweep

There is **no object-level sweep**. Dead young objects are never visited, and old holes are never threaded onto free
lists (Chez). Post-collection work is O(#blocks + #large objects + #host registrations). That removes today's
2.4 ns-per-slot sweep over the high-water mark, which made the 176 ms post-load pause [demographics §3]. A lazy sweep is
unnecessary because nothing is ever swept.

### 6.6 Weak references and ephemerons (no O(n²) rescans)

- **Key-indexed resolution (Whippet [whippet §1.9]).**
  - When the trace reaches ephemeron E with an unmarked, unforwarded heap key K:
    - E goes into `pending[K]`, a hash table outside the heap;
    - K's granule byte gets `EPH_PENDING`.
  - When any object is marked or copied, its granule byte is checked (already loaded for the mark). If
    `EPH_PENDING` is set, its waiters are popped and their values traced. A moved key also rewrites E's key slot.
  - The cost is O(1) per key, which replaces the round fixpoint (289 ms on a 16k chain [finalization §0.7]).
- **Generational rule.**
  - SRFI 124/254 ephemerons are immutable apart from breaking. They are always allocated through the runtime into the
    nursery, never pretenured, and promotion is age-monotone. **E is therefore never older than K or V**
    [finalization §4.6].
  - Minors ignore old ephemerons and resolve young ones against young keys.
  - `(gc)` is a major and breaks dead keys of every age (`ephemerons.rs:23,52`).
- **One fixpoint** covers host-payload tracing (which replaces weak ids), ephemerons and, later, guardians (commit
  `1d18c49` [DIGEST §1.4]).
- **No raw-bits weak tables remain.** Provenance moves into the objects (stage 3). Pruning the symbol table, the
  scope-set intern table and the moved-hash table are the only weak-key passes.

### 6.7 Finalization and ports (R1–R6)

- **Representation (R6).** A port is a canonical `HOST(PORT, id)` object. `PortTable` owns its `PortData`. The current
  ports live in the thread's parameterization as heap values, so `(eq? (current-output-port) (current-output-port))`
  is `#t`, which fixes E4.
- **Registration lists, young and old.** After a minor, a forwarded young registration is updated and moved to the old
  list; an unforwarded one is finalised. After a major, unmarked old registrations are finalised.
- **Finalisers (R4)** are Rust-only: flush ignoring errors, close the descriptor, `MemoryWriter::finalize`, drop the
  tree-walker payload. They do no allocation, run no Scheme, and run in the epilogue before Scheme resumes (**R2**).
- **R1.** `end_process`, including `emergency-exit`, flushes every live `PortTable` entry, replacing `OUTPUT_FILES`.
- **R5.** Dropping the heap finalises every entry.
- **R3, descriptor pressure.**
  - Each file port charges 8 KiB of external bytes.
  - Opening `min(128, RLIMIT_NOFILE/4)` ports since the last GC raises `GC_MINOR`.
  - `open-*-file` returns a distinguished `EMFILE` failure, and its Scheme wrapper runs `(gc)` and retries once
    (chibi's behaviour [finalization §2.1]).
- GC-time flushing is observable, so its tests live outside the GC-differential lane [finalization §6.10].

### 6.8 Code liveness

- Descriptors are non-moving heap objects.
- Edges into them: closure word 0, frame word 1 (register stack and chunks), the `nested` vector (parent → lambdas),
  RTD → parent RTD.
- **Only a complete major decides liveness.** After the epilogue, unmarked descriptors release their code-table entry
  (bytecode, maps) and their JIT code.
  - Release is safe at a safepoint. Under the S1 fragment model (§10) no native activation of released code exists: the
    poll's own caller is live by construction.
- This deletes `live_closures`, `gc_freed_closure_code_ids`, `RETIRED_VM_CLOSURE_CODE` and the four `Rc` operations per
  call and return [cont-repr §4].
- Code-space bytes count toward the major trigger, so code release stays bounded
  (`finished_forms_release_code.rs` forces `(gc)`, which is a major).

### 6.9 Identity hash (Lilliput-style, with side metadata instead of header bits)

`identity-hash(v)`:
- immediates: `mix(bits)`;
- symbols: the name hash in the header;
- other heap objects, by granule byte state:

  | State | Result |
  |---|---|
  | not `HASHED` | set `HASHED`, return `mix(addr ^ heap_salt)` |
  | `HASHED`, not `HASH_MOVED` | return `mix(addr ^ heap_salt)` |
  | `HASH_MOVED` | look up `MovedHashTable[addr]` |

When the collector moves a `HASHED` object, it inserts or rekeys `(new_addr → hash)` and sets `HASH_MOVED` at the
destination. Dead entries are dropped in weak processing: the young part at minors, all of it at majors.
- This works for headerless pairs and closures alike.
- Its cost is proportional to hashed objects that moved: eqtable's 500 K keys, each moved once at promotion
  [demographics §7.1].
- A young object hashed and then dead never touches the table.
- SRFI 69 buckets store these hashes, so `hash-by-identity` stays correct under motion.
- `equal-hash` falls back to identity hash instead of the heap index.

### 6.10 Epilogue order (both collectors; minors restrict each step to young entries)

1. Trace (copy or mark) to the fixpoint, including host-payload tracing.
2. Ephemeron resolution, interleaved with step 1 by key, then guardians (later, SRFI 254) when nothing else progresses,
   then back to step 1.
3. Break remaining pending ephemerons (key ← `BWP`, value ← `BWP`).
4. Transport cells (SRFI 254, deferred).
5. Weak-key tables: symbol table, scope-set intern table, moved-hash table.
6. Host registrations: promote survivors, finalise the dead (ports flush and close).
7. Code release (majors only).
8. Block accounting, LOS free, decommit, re-arm.
9. Resume.

This is the order in [finalization §4.9].

### 6.11 High-survival adaptation

- **Sticky mode** handles queue3 and deeprec, where survival is 100% and copying does 1.7–1.8× more work than today's
  marking [demographics §5.2]. If byte survival exceeds 50% in two consecutive minors, the next minors mark the whole
  nursery in place: every block is `IN_PLACE`, promoted wholesale, and nothing is copied. Sticky mode exits when
  survival falls below 20% for two minors.
- **Sparse in-place blocks** are evacuated at the next major (§6.2).
- **Allocation-site pretenuring** is deferred to the JIT, where site counters are natural. It would pretenure *into
  in-place blocks*, never directly into old space, so initialising stores stay barrier-free.

---

## 7. Write barrier

**Kind: field logging on an armed bit, pre-write, with a value filter and static field parity.** This departs from
Chez's store buffer plus cards. On Patina's measured mix, Chez's buffer logs 0.5–1.0 entries per barrier store, and
cards need value-only parseable spaces [barrier-res §0, §3]. Polarity is LXR's: fresh memory, which includes all nursery
metadata, is 0 = unarmed. Stores into young holders therefore fall through with **no young test**. That covers 99.4% of
heap stores in the median workload [demographics §6.2].

**Fast path, aarch64.** Holder in `x0`, value in `x1`; the slot is at byte offset `OFF` from the object start; `p` is
the parity of word `OFF/8`, which is static for fixed fields of a 16-B-aligned object. `x20` holds `meta_bias`, loaded
from `VmCtx` once per function and hoisted.

```
        add   x9,  x0, #(OFF - TAG)      ; slot address, shared with the store
        tbz   x1,  #0, Lstore            ; [1] value is not a heap reference → no barrier
        lsr   x10, x9, #4                ; [2]
        ldrb  w11, [x20, x10]            ; [3] granule byte
        tbnz  w11, #p, Llog              ; [4] armed → log (cold block)
Lstore: str   x1,  [x9]
        ...
Llog:   ; inline, call-free (Cranelift: a call would be a safepoint [cranelift-gc §2.1])
        and   w11, w11, #~(1 << p)
        strb  w11, [x20, x10]            ; disarm; a lost race only duplicates an entry, which is harmless
        ldr   x12, [x21, #REMSET_CUR]
        str   x9,  [x12], #8             ; append the slot address
        str   x12, [x21, #REMSET_CUR]
        ldr   x13, [x21, #REMSET_SOFT]
        cmp   x12, x13
        b.lo  Lstore
        str   wzr, [x21, #TICKS]         ; request a minor at the next poll
        ldr   w13, [x21, #EVENTS]
        orr   w13, w13, #EV_MINOR
        str   w13, [x21, #EVENTS]
        b     Lstore
```

- **Cost.** 1 instruction on the immediate path, 4 on the heap-value path. A variable vector index makes the parity
  dynamic: `ubfx; lsr` adds 2, since element i sits at word `i + 1`.
- **Hard end unreachable.** The soft limit sits 64 MiB below the reservation's committed end, and JIT code between
  polls is straight-line [barrier-res §6].

**What the remembered set records and why.** Field logging needs no heap parsability. Its slow path runs at most once
per field per cycle: 5 times on nboyer against 24,504 OCaml ref-table entries; 96 on the queue workload; 131 K on
hashtab [barrier-res §2.3]. Minors re-read each slot and re-arm it; majors re-derive (§6.2).

**Barrier sites.** `WriteCell` (93% of nboyer's barrier stores), `VectorSet`, `set-car!`/`set-cdr!`, `list-set!`,
record set, global cell stores (`StoreGlobal`/`Define`), promise-box update, parameter set, bulk `vector-fill!` and
`vector-copy!` [barrier-res §5].

**Elision rules.**
1. Statically immediate values: literals, results of `Not`/`NullP`/`PairP`/`Eq`/`Lt`/`NumEq`/char ops, fixnum-only ops.
   Generic `+` is excluded because of bignums (Chez's `$fixmediate` rule).
2. **Initialising stores.** These are stores into an object allocated in the same straight-line region with no poll
   and no may-GC call in between. Because allocation never collects, this covers every constructor, `AllocCell` → first
   `WriteCell`, and `cons` chains.
3. Register-file and frame stores, which are roots.
4. Chunks, which are immutable.
5. Pointer-free kinds, which have no barrier at all.

**Pre-logged allocations.**
- Objects born old (descriptors, cells, symbols, static) are created unarmed and appended to a "remember whole" list;
  the next minor scans them and then arms them [barrier-res §7.1 item 8].
- Young LOS objects are unarmed until promoted.

**The single Rust store funnel.** Every mutation in Rust goes through it.

```rust
impl Mutator {
    /// Every heap store from Rust. Same filter, metadata test and slow path as the JIT.
    #[inline(always)]
    pub fn store<S: SlotOf>(&self, slot: S, v: TaggedValue) {
        let a = slot.addr();                                  // typed: PairRef::car(), VectorRef::slot(i), CellRef::value(), RecordRef::field(i), GlobalCell::value()
        if v.is_heap() {
            let m: &Cell<u8> = unsafe { &*(self.meta_bias.add(a as usize >> 4) as *const Cell<u8>) };
            let bit = ((a as usize >> 3) & 1) as u8;
            if m.get() & (1 << bit) != 0 { self.log_slot(a, m, bit) }   // #[cold]
        }
        unsafe { a.write(v) }
    }
    pub fn store_range(&self, v: VectorRef, start: usize, src: &[TaggedValue]); // bulk: scans armed bytes once
    // There is no API that hands out `&mut [TaggedValue]`: `vector_slice_mut` is deleted.
}
```

- Initialising stores are available only inside constructors (`alloc_pair(car, cdr)`, `alloc_closure(desc, &fvs)`).
- Reads return values, or `&[TaggedValue]` borrowed from `&self` for non-allocating bulk reads.
- The unbarriered `Rc` store channels go away with their payloads (§3.3): `prim:records.rs:262`,
  `parameters.rs:168,267`, `lazy.rs:134`.

---

## 8. Roots and rooting

### 8.1 VM frames

- Frames live in each green thread's **non-relocating register stack**: reserved address space, 32 B frame headers
  interleaved with register windows (§3.2), walked through `prev-offset` with no side vector [jit-ready R31].
- Every slot named by the frame's **safepoint map** at its `pc` is a precise, updatable root.
- Maps are compressed to safepoint pcs (calls and poll sites) and stored packed beside the descriptor. Today
  `register_roots` costs about 40 B per pc, 65% of the instruction stream [libload §2.2].
- Windows are zero-filled on push (fixnum 0, a plain `memset`) while maps still treat locals conservatively.
- `CallFrame.closure` becomes a `TaggedValue` slot (PR-2).
- **Minor cost is bounded by the live buffer.** It holds at most 1 MiB, and anything deeper has been frozen into
  immutable chunks (§10), which are old after one minor. deeprec's 11.5 M registers therefore stop being rescanned on
  every minor without a separate return-path watermark [demographics §7.2].

### 8.2 JIT frames

- **Tier 1 (baseline, S1 fragments [cranelift-gc §6.1]).** JIT code reads and writes the same VM frames.
  - Between safepoints it may cache values in SSA registers.
  - Before any **may-GC helper** (poll slow path, call into a may-run-Scheme primitive, `call/cc`, raise, wind
    operations) it publishes dirty values and the `pc`, then reloads after.
  - **It declares no Cranelift stack maps.** Leaf helpers (allocation slow path, barrier, bignum overflow) are not GC
    points because they never collect.
  - Helper classes are explicit in the helper table (`Leaf | MayGc | MayTransfer`) [jit-ready R17].
- **Tier 2 (possible optimising tier).** The same rule holds: at every may-GC helper and every non-inlined Scheme call
  the frame is materialised in VM slots, so Cranelift spills nothing on its own.
  - Inlining Scheme callees means deoptimising on capture: VM frames are built from Cranelift debug tags before the
    capture or GC proceeds.
  - **This GC never needs Cranelift user stack maps.** The cost is forgoing callee-saved registers across may-GC calls,
    values the user-stack-map spiller would spill anyway [cranelift-gc §2.1].
- Interior pointers never live across a safepoint.

### 8.3 Rust code

- **Context.** `Mutator` (carrier state, §12) and `Cx { heap: &Mutator, inst: &Instance }` replace
  `Rc<RefCell<Heap>>`. Mutator state is `Cell`-typed, so `&self` allocates, the DIGEST's contradiction resolution 6.
- **Only the driver can collect.** `collect` takes `&mut HeapCore`, which only the driver owns.
- **Primitives.** `fn(&mut Cx, &[TaggedValue]) -> Result<TaggedValue, EvalError>`. They hold any number of values in
  locals across allocation, which needs no rooting (B1).
- **Across a may-GC boundary** (nested driver loops, library loading, embedder calls):
  - a `RootScope` LIFO stack on the `Mutator`: `let r = scope.root(v); …; scope.get(r)` (Wasmtime `RootScope`
    [libload §4.3]);
  - `OwnedRoot`, an index plus generation into a handle table the collector updates, for long-lived Rust holders.
- A branded `Value<'gc>` lifetime is optional and can be added later, because signatures already carry `Cx`.

### 8.4 Tree-walker policy (it may lag)

A tree-walker heap is constructed with `HeapConfig { move_policy: PinRoots, generational: false }`:

- Collection happens only at the outermost trampoline safe point. Nested trampolines keep `NoGcScope`.
- Every `TaggedValue` reported from tree-walker Rust structures is reported through `visit_pinned`: `StepResult`,
  `ContValue` chains, `Rc<Environment>` bindings, `CpsExpr` literals, wind/prompt/handler vectors, `PENDING_ESCAPE`,
  payloads in `TwProcTable`/`TwContTable`. The referent stays in place for that cycle. Objects reachable only through
  heap edges still move.
- **Every collection is a major (major-only).** The tree-walker's environment stores are off-heap and unbarriered, so
  there is nothing to remember. Its GC-heap allocation rate is tiny: 43 heap allocations for `(fib 25)`
  [tree-walker §2.2]. Environments are traced only at majors.
- Drop-at-sweep becomes host-table registrations: `CpsLambda` and `CpsContinuation` payloads are ids.
- Environment creation adds about 240 B of external bytes to the trigger, so closure↔environment cycles are reclaimed.
- Effort is about 2–3 weeks [tree-walker §4 (b)+(e′)]. The tree-walker shares the object model, primitives,
  allocator and collector code; only the root-visiting mode differs.

### 8.5 Embedding API

- `Interpreter::eval_*` returns an `OwnedRoot` (handle table entry, released on `Drop`).
- Scoped bulk work: `interp.with(|cx| …)`.
- `display_tagged` accepts `impl AsValue`, so most of the ~200 test call sites compile unchanged.
- Heap drop finalises and unmaps.
- `patina-compat`'s standalone reader heap is `Heap::new_standalone()`: no driver, never collects.

### 8.6 Fate of every off-heap holder

| Holder today | Fate |
|---|---|
| VM register file `Vec<TV>`, `CallFrame` vector | per-thread non-relocating register stack with interleaved headers (stage 6); before that, slot-updated `Vec` |
| `CallFrame.closure: HeapIndex` | `TaggedValue` slot in the frame header |
| `CallFrame.code: Rc<CodeObject>` | raw descriptor pointer (non-moving, marked) |
| `CodeObject.constants` in `Rc` | inline slots in the descriptor, updatable; pre-logged at load |
| Environments: global, library | `Namespace` name → `CellRef` (cells non-moving, variant R then C [global-cells]); no values in Rust |
| `Library.exports: HashMap<String, TV>` | `FxHashMap<Box<str>, CellRef>` |
| `VmClosure.globals: Rc<Environment>` | deleted under C; under R, a per-descriptor link table |
| `CompiledMacro` literals and `heap` | literal vector in the `HOST(MACRO)` handle's slot; `heap` field deleted |
| `symbol_table`, `core_syntax_table` | weak name → symbol map over non-moving symbol space; core syntax in static space |
| `syntax_sources`, `SourceMap.locations`, child spans | **deleted**: inline `src` id in identifiers plus per-document location tables with no `TaggedValue`s [libload §5.3] |
| VM continuation side tables | deleted: continuation records and chunks are heap objects |
| `WindRecord.handlers: Rc<[H]>` | persistent heap lists (§10) |
| Records, parameters, promises with `Rc<RefCell>` | inline heap objects |
| Ports and `thread_local!` current ports | `PortTable` plus parameterization |
| Tree-walker `Rc` graphs | pinning roots plus host tables (§8.4) |
| `ParsedLibrary.body`, `with_globals` `saved` | `Loading` registry entry roots (heap list) plus a `globals_stack` root [libload §8 R1–R2] |
| Desugarer `quoted`, `OpenNodes`, partial `CoreExpr` literals | point C stays `NoGcScope`; `CoreExpr` literal pool (index-based) for syntax-case |
| Embedder-held values | `OwnedRoot` |
| Debugger hook storage | a `RootProvider` registered on the open root registry; event payloads are pinned for the pause |

---

## 9. Safepoints and polling

**The event word** (Chez `%trap` plus `something-pending` [chez §4]; threads-rec item 7):

```rust
#[repr(C)] struct VmCtx { /* … */ ticks: Cell<i32>, events: AtomicU32, /* … */ }
// events bits: GC_MINOR=1, GC_MAJOR=2, OOM=4, INTERRUPT=8, DEBUG=16, PREEMPT=32, TERMINATE=64, HANDSHAKE=128, TIMER=256
```

**JIT poll:** `ldr w9,[x21,#TICKS]; subs w9,w9,#1; str w9,[x21,#TICKS]; b.le Lpoll_cold`, 4 instructions.

**Poll slow path.** It is a may-GC helper, so it publishes first. Then it:
1. reloads `ticks = quantum`;
2. reads `events` (Acquire under the `threaded` feature);
3. services events in priority order: GC (collect), OOM, INTERRUPT (raise), PREEMPT (green-thread switch), DEBUG.

The thread's own allocator and barrier zero `ticks` directly, so a GC request fires at the next poll. Other threads or
signals can only set `events`, so they are seen within one quantum.

**Placement.**
- JIT: entry of non-leaf procedures and self-tail-call back edges. Leaf procedures without loops have no poll, as in
  Chez. Codegen emits forward jumps only, so every loop passes through a call or tail call [jit-ready R14].
- Interpreter: the same counter is decremented at `Call`/`TailCall`/`Apply`/`CallWithValues` dispatch, **not** at every
  instruction. Today's per-instruction check (+1.1–1.4% [DIGEST §1.9]) goes away.

**Deterministic mode.**
- The quantum defaults to 1,024 polls. Preemption is driven by tick expiry, not a timer, when `PATINA_DETERMINISTIC=1`
  (the default in test lanes).
- GC timing is a function of program and build only, so the differential lanes stay byte-identical.

**Nested Rust loops (what replaces `GcDeferGuard`):**
- **Rooted boundaries** at library-loading points A, B, D and E [libload §4.2], at `with_globals`, and at
  `across_reentry` callers once the remaining Rust→Scheme re-entries are resumable (`Step::LoadLibrary`, resumable
  `%parameterize-swap!` [prim-embed §3.4]). Nested driver loops then poll and collect normally.
- **`NoGcScope`** (a depth counter on `Mutator`; the poll slow path re-arms a short quantum and returns) remains only at
  desugaring point C and in nested tree-walker trampolines. Unlike today, a long `NoGcScope` no longer grows
  never-shrinking arenas. The nursery overflows into pool blocks, and the next poll after the scope collects them all.
  The largest single form is 746 K allocations, about 20 MiB [libload §2.4].
- **Safe regions.** `enter_safe_region`/`leave_safe_region` wrap blocking I/O and FFI. They are no-ops under M:1.

---

## 10. Continuations and stacks

The stack representation **departs from Chez** (heap-allocated segments sealed in place) and follows **Loom's freeze and
Larceny's stack-cache flush** over a fixed live buffer [cont-repr §3.1 C′]. It keeps Chez's control ideas: continuations
are procedures, a `dounderflow` stub, bounded underflow copying, and overflow handled by splitting.

**Live stack.** Each green thread has one register-stack buffer (§4.4). It holds frames above a bottom **underflow stub
frame**: an ordinary frame whose descriptor is `UNDERFLOW` and whose slots hold a view `(chunk, frames_remaining)`.

**Capture (`call/cc` in frame F).**
1. Clear the delivery hole in F (the 296 MB rule [DIGEST §5 pitfall 18]).
2. **Freeze** every live frame from the bottom stub through F into one new immutable `CHUNK` whose parent is the stub's
   current view. Retire dead slots from each frame's map as they are copied.
3. Reset the buffer to a fresh underflow stub viewing the new chunk.
4. Allocate `CONT` record = {chain, winders, handlers, prompts, parameterization, re-entry ids, deliver}.
5. Call the receiver.

Capture costs O(frames pushed or thawed since the last freeze). A loop that captures at the same depth pays O(1) per
capture (toy: 57 ns against 5 µs today at d = 1000 [cont-repr §2.2]). No `Return` path pays anything, because no
watermark is maintained: the live buffer *is* the set of frames resumed since the last freeze.

**Return into the stub.**
- Thaw: copy the top frames of the viewed chunk into the buffer above the stub, and re-point the stub's view below them.
- Thaw size is adaptive: the whole chunk if it is ≤ 4 KiB, else 1 frame (Loom) — Chez's 128 B plus one frame scaled to
  Patina frames.
- **Multi-shot works because chunks are never mutated.** Thawing copies.

**Invoke k.** Drop the live frames, install a stub viewing `k.chain`, thaw, deliver into `deliver_reg`. Winds travel one
thunk at a time through the existing `wind_jump` stub frames, which are ordinary frames in the live buffer.

**Overflow.** At 75% of the 1 MiB soft limit, freeze all but the top 16 frames and slide those down above a new stub
(Chez `S_overflow`, Gambit `___stack_limit`). Non-tail recursion is bounded by heap, not buffer, which keeps today's 10 M
frames [jit-ready §2.5].

**Dynamic state.**
- Winders, handlers and prompts become persistent heap lists of records with **virtual depths** (frozen depth plus live
  frames). Capture copies three pointers instead of three vectors, and a `dynamic-wind` stops copying the handler stack.
- `next_wind_step` walks lists to equal length, then compares ids.
- Handler and prompt depths are distances from their prompt, as `raise_step::DEPTH_BELOW` already is [cont-repr §6].
- Parameterize becomes deep-bound: a heap parameterization of `parameter → cell` (threads-rec item 9).

**Delimited continuations.** Capture freezes from the prompt's virtual depth to the top. Invoke **appends eagerly**,
thawing all captured frames above the invocation, so `append_delimited`'s relocation code is kept. Lazy append is
deferred until measurement asks for it.

**GC interaction.**
- Chunks are pointerful heap objects with a frame-walking tracer (descriptor → map at `pc`; word 1 is a code edge).
- Young chunks are copied at a minor. Chunks over 8 KiB go to young LOS and are promoted by relabelling.
- Chunks are immutable, so they need no barrier and no remembered-set entries after promotion.

**JIT interaction (S1).**
- Every return point is a fragment entry, `desc.entries[pc]`, or the interpreter if there is none.
- `Return` = pop the frame, then `return_call_indirect` to the caller's resume entry. The stub frame's entry is a Rust
  thaw routine, so `Return` never needs a special case.
- Escapes are "the runtime returns a new target".
- Native depth is constant, so `stack_switch` is not needed [cranelift-gc §2.6].

**Staging.** Design A comes first (stage 2): a continuation is one `CHUNK` holding the whole stack, with eager thaw. It
shares the chunk format, so the move to C′ in stage 6 changes the policy, not the representation.

---

## 11. Pluggability contract

**The contract is the object model plus four fast paths described as data.** One production collector ships;
configurations and torture modes are runtime knobs inside it.

```rust
// crates/patina-gc (new crate, no VM or JIT dependency, Miri-testable)
#[derive(Clone, Copy)] pub enum BarrierKind { None, FieldLog, Card /* runner-up */, FieldLogSatb /* future */ }
#[derive(Clone, Copy)] pub enum PollKind { TickCounter, Flag }
#[repr(C)] #[derive(Clone, Copy)]
pub struct GcAttrs {                              // Whippet gc-attrs.h, as constants
    pub granule: u32, pub block: u32, pub large_threshold: u32,       // 16, 32768, 8192
    pub heap_ref_bit: u8,                                              // 0
    pub barrier: BarrierKind, pub meta_shift: u8, pub log_bit0: u8,    // FieldLog, 4, 0
    pub poll: PollKind,
    pub can_move: bool, pub can_pin: bool,
    pub ctx: CtxOffsets,                                               // field offsets in VmCtx
}
#[repr(C)] pub struct CtxOffsets { pub alloc_ptr: u16, pub alloc_limit: u16, pub meta_bias: u16,
    pub remset_cur: u16, pub remset_soft: u16, pub ticks: u16, pub events: u16,
    pub stack_top: u16, pub stack_limit: u16, pub thread: u16 }

pub unsafe trait Plan: 'static {
    const ATTRS: GcAttrs;
    type Config: Default + Clone;                 // move policy, generational, nursery bounds, torture knobs
    fn alloc_slow(h: &HeapCore<Self>, m: &Mutator, req: AllocReq) -> NonNull<u8>;     // never collects
    fn log_slot(h: &HeapCore<Self>, m: &Mutator, slot: NonNull<TaggedValue>);        // never collects
    fn collect(h: &mut HeapCore<Self>, why: CollectReason, roots: &mut RootSet<'_>) -> GcStats;
    fn pin(h: &HeapCore<Self>, v: TaggedValue) -> PinToken;
    fn identity_hash(h: &HeapCore<Self>, v: TaggedValue) -> u32;
}

pub trait SlotVisitor {
    fn visit(&mut self, slot: &mut TaggedValue);                 // precise and updatable
    fn visit_pinned(&mut self, v: TaggedValue);                  // holder cannot be updated: pin the referent
    fn visit_code(&mut self, d: NonNull<CodeDescriptor>);        // code edge (counts at majors)
}
pub trait RootProvider { fn trace_roots(&mut self, v: &mut dyn SlotVisitor, kind: CollectKind); }
```

**The object model is shared by every plan**, generated by one `layout!` spec, the Rust analogue of `mkgc.ss`:

```rust
layout! {
  pair       tag 0b0001                { car: Value, cdr: Value }
  procedure  tag 0b0011 desc CodeDesc  { free: [Value; desc.nfree] }
  record     tag 0b1111 desc Rtd       { fields: [Value; rtd.nfields] mask rtd.ptr_mask }
  vector     tag 0b0101 header VECTOR  { elems: [Value; hdr.len] }
  object CELL      { value: Value }
  object EPHEMERON { key: WeakKey, value: Ephemeral, link: GcLink }
  object CHUNK     { parent: Value, flags: Raw, frames: Frames(hdr.len) }   // custom tracer
  /* … */
}
```

From the spec the macro generates `size_of`, `trace<V: SlotVisitor>`, `copy`, `verify`, the JIT field-offset constants
and the datum writer's kind view. This removes the "misfiled leaf is a use-after-free" hazard
(`core:heap/mod.rs:137-141`). After Chez's switch to generated code, a fused collect-and-measure traversal ran about
2× faster, and full GCs on locked-object workloads ran 10–20% faster [racket-larceny §3.2].

**Static selection.** `pub type ActivePlan = HybridPlan<FieldLog>;` under the default features;
`--features gc-card` gives `HybridPlan<Card>`. Nothing outside `patina-gc` names a plan. Fast paths are
`#[inline(always)]` code over `ActivePlan::ATTRS` and `const`-fold, so there is no `dyn` anywhere. The JIT emits from
`ActivePlan::ATTRS`.

**Collectors shipped.**

| Name | What it is | Used for |
|---|---|---|
| Hybrid (production) | §6: copying nursery, mark-in-place old with sparse evacuation | default VM heap |
| Hybrid, major-only | same code, `generational = false`, barrier `None` | kill-criterion K1 fallback; M5 baseline; tree-walker heaps (with `PinRoots`) |
| Null (`PATINA_GC=0`) | never collects automatically; `(gc)` runs a major | GC-off differential lane; speed-of-light baseline (Epsilon) |
| Torture | `PATINA_GC_STRESS=n` (minor every n polls), `PATINA_GC_MOVE_ALL=1` (every GC a major evacuating every unpinned block, from-space poisoned and `PROT_NONE`), `PATINA_GC_VERIFY=1` | debug and stress lanes (§14) |
| Two-mutator | `PATINA_GC_MUTATORS=2`: two `Mutator`s alternate deterministically on one thread | N-readiness lane (§12) |

There is **no separate mark-sweep oracle.** During stage 4 the arena build *is* the oracle (both builds in CI until
parity). After that, Null and MOVE_ALL bracket the production collector.

**How a future collector plugs in.**
- **Incremental old-space marking** (Racket BC style, slices piggybacked on minors). It uses `barrier_mode` (a `VmCtx`
  byte read only by slow paths) and the same armed bits as an incremental-update log. The JIT does not change.
- **SATB.** `BarrierKind::FieldLogSatb` drops the value filter and logs the old value. It is a compile-time kind, so
  JIT code is recompiled.
- **Parallel tracing.** The `threaded` feature makes mark-bitmap words atomic, gives each worker a work-stealing deque,
  and installs forwarding with a CAS on word 0, which headers and descriptor-first layouts allow. Not Chez's ownership
  partitioning [chez §11].
- **MMTk-backed.** It would need a `pair-headers` object-model variant (MMTk's untagged `ObjectReference` forces headers
  on pairs, 16 → 24 B) and runs one instance per process [immix-mmtk §10]. The `Plan`/`RootProvider`/layout seams are
  MMTk-shaped (ObjectModel, Scanning, Collection), so a throwaway A/B spike stays possible. It is not a drop-in.

**Explicitly excluded:**
- load or read barriers and coloured pointers [hotspot §3.5];
- conservative scanning of VM or JIT frames (#423's ephemeron tests forbid it);
- collection inside allocation;
- finalisers that run Scheme (Java finalizers, JEP 421);
- ownership-partitioned parallel GC;
- patching movable addresses into machine code.

---

## 12. Threading readiness

**Mutator (carrier) vs GreenThread** (threads-rec §1.5).

- `Mutator` = the `#[repr(C)] VmCtx` above plus Rust-side carrier state. It owns:
  - its allocation block(s), medium overflow block and young-LOS list;
  - its remembered-set buffer;
  - `ticks`/`events` and safepoint state (Running, AtSafepoint, InSafeRegion);
  - its `RootScope` stack and `NoGcScope` depth;
  - a pointer to the current `GreenThread`.

  Under M:1 there is exactly one. The JIT's pinned register points at it.
- `GreenThread` = a heap `HOST(THREAD)` handle plus a Rust record holding:
  - its register-stack reservation;
  - its dynamic environment heads (parameterization, winders, handlers, prompts, current ports via parameterization);
  - re-entry and escape fields;
  - the `ran_since_gc` bit.

  A switch is O(1): swap `stack_top`/`stack_limit`/`thread` in `VmCtx`.

**N-ready now at zero single-thread cost:**
- allocation buffers and nursery blocks are per mutator, drawn from a global pool with an uncontended lock;
- remembered sets are per mutator;
- granule-byte log bits are clear-only from mutators, so a lost race only duplicates an entry;
- the safepoint protocol is written for N: request, then every mutator acknowledges at a poll or in a safe region, then
  collect, then release;
- safe regions around blocking I/O;
- deep-bound dynamic state as heap data;
- continuations refer to no carrier state;
- no `thread_local!` runtime state;
- interning goes through one function;
- the `threaded` feature reserves the funnel's release fence and the JIT allocation's publication fence;
- the two-mutator test lane exercises all of the above.

**Deferred** (only on owner approval of shared-memory parallelism, open question 7):
- atomic slot access (`Slot` as relaxed `AtomicU64`);
- parallel tracing and parallel minors;
- handshakes;
- a `Send`/`Sync` heap and the remaining `Rc` graphs;
- Miri/loom/TSan lanes for N > 1.

The tree-walker stays M:1-only: a thread is a `StepResult` plus dynamic state, and deferral is its rooting
[threads-rec §4].

---

## 13. Heap sizing, pacing and observability

**Nursery.**
- Starts at 8 MiB (Chez's 8 MiB trip [chez §5]).
- Adapts between 2 MiB and 64 MiB toward a **minor-pause goal of 2 ms**: it grows by 25% while pauses are under 1 ms
  and survival is under 10%, and shrinks when pauses exceed 2 ms.
- Sticky mode (§6.11) handles high survival.
- Survival above 64 K objects barely improves with size [demographics §5.1], so the goal is pause, not size.

**Major trigger, in bytes.** A major runs when any of these holds:

| Trigger | Condition |
|---|---|
| Old growth | `old_bytes ≥ L + max(16 MiB, 8192·√L)`, where L = live bytes after the last major, with a 32 MiB floor before the first major (Racket CS's rule [racket-larceny §3.6]: 3.6× headroom at 10 MB, 1.8× at 100 MB, 1.25× at 1 GB) |
| Allocation volume | nursery bytes since the last major `≥ max(1 GiB, 64·L)`, so the Larceny ephemeron suite breaks load-time keys within 100 M pair allocations [finalization §4.6] |
| Explicit | `(gc)` |
| OOM | the `OOM` event |
| Remembered-set pressure | after a minor, the remembered set exceeded 64 K entries for 8 consecutive minors (likely old→young churn better served by a major) |

`old_bytes` counts old blocks, LOS, code and descriptor space, and **external bytes**:
- 8 KiB per open file port;
- host-payload byte estimates (tree-walker environments at 240 B each, compiled macros);
- JIT code bytes.

This fixes the gcold case (628 MB RSS for 12 MB live) and the descriptor-exhaustion case [demographics §3;
finalization §2.1].

**MemBalancer** (headroom ∝ √(live·alloc_rate/gc_rate) [whippet §1.10]) replaces the √L rule in stage 5b *only* if it
wins an interleaved A/B on RSS at constant GC time. Wingo's "hyperactive squirrel" caveat applies, so changes are
smoothed exponentially.

**Free reserve.** After a major, keep `max(64 blocks, 3% of heap)` free for promotion and evacuation, growing commitment
if needed. This avoids Wingo's livelock [whippet §1.10]. Evacuation stops at the reserve.

**Decommit.** As in §4.4. RSS follows live data instead of the high-water mark (today 464 of 501 MiB after a load is
empty capacity [libload §2.3]).

**Observability.**
- `GcStats` per collection: kind; phase times (rendezvous, roots, trace, weak, epilogue, accounting); bytes copied,
  promoted, marked, evacuated; blocks in-place; remembered-set entries; nursery size; survival.
- `PATINA_GC_LOG=path` writes one CSV row per collection.
- `(gc-stats)` gains bytes and pause fields.
- MMU at 1, 10 and 100 ms windows is computed from the log (Larceny `gc_mmu_log.c` [racket-larceny §4.6]).
- `last_pause_micros` finally has a reader.

---

## 14. Testing and verification

- **Torture lanes** (`scripts/run_gc_differential.sh`, extended). The chibi suite, `EXPECTED_TOTAL=1226`, runs on both
  backends × {off (Null), default, stress, **move-all**, **verify**} × {release, debug-poison}.
  - Default and the others must be byte-identical to off.
  - Port finalisation tests live outside this lane.
  - Move-all fills from-space with `POISON` and `mprotect`s it `PROT_NONE` until reuse, so a missed root faults at once
    (HotSpot `CheckUnhandledOops` [hotspot §3.9]).
- **Heap verifier** (`PATINA_GC_VERIFY=1`, debug and verify builds), after each collection:
  1. every reference reached from the roots points at an allocated object start (a debug-only start bitmap is set at
     allocation);
  2. no nursery reference survives a minor;
  3. every old pointer slot is either armed, or present in the remembered set or the boot remembered set;
  4. `FORWARD`/`POISON` are unreachable;
  5. `live_bytes` matches marked sizes;
  6. no reachable descriptor is released;
  7. every `HASH_MOVED` object has a table entry.
- **Miri.** `patina-gc` (block allocator, copy and mark, barrier slow path, remembered set, ephemeron table, hash table)
  over a synthetic object model and a `Vec`-backed fake reservation under `cfg(miri)`, run with
  `-Zmiri-strict-provenance`. The JIT is differentially tested against the Rust reference fast paths
  [rust-gcs §4.7].
- **Scoreboards**, required for every stage: `control_flow_matrix.rs` (64/64), `hygiene_matrix.rs` (139),
  `ephemerons.rs`, `escape_from_primitive.rs`, `finished_forms_release_code.rs`, `gc_vm.rs`, `gc_tree_walker.rs`, both
  chibi scripts, suite oracles, both Larceny lanes, compat smoke.
- **GC benchmark set.**
  - The 20 demographics workloads plus the Larceny GC suite, run from the reference checkout. They are LGPL and not
    vendored, the same way `run_larceny_tests.sh` works. The three Patina-written extras (deeprec, libload, eqtable) are
    vendored.
  - Continuation probes: `samedepth1000`, `escape1000`, `pingpong1000`, `ctakdeep`, `abort100`.
  - Metrics: wall, user CPU, max RSS, collections by kind, total, max and p99 pause, MMU(10 ms).
  - Every perf claim is an interleaved main/branch/main triple, medians with spread.
- **New semantic tests:**
  - E1/E2/E4/E5 port tests (subprocess `setrlimit` for E2);
  - SRFI 125 weak keys;
  - an ephemeron chain-scaling bound;
  - an identity hash that survives move-all;
  - capture-at-depth retained-bytes tests.
- **How each stage proves itself:** all lanes green; its listed measurement in §15; no interleaved A/B regression
  beyond noise on the Criterion quick set and the GC set, unless the stage's acceptance criteria accept one explicitly.

---

## 15. Migration plan

Every stage lands with both backends and every lane green. Each PR closes a GitHub issue filed first, per the project
rule. Effort is in engineer-weeks [I].

| Stage | Scope | Crates and files | Acceptance and measurement | Effort | Standalone value |
|---|---|---|---|---|---|
| **0. Measure and prepare** | GC bench lane; defect issues; frame closure slot; depth accessor | `scripts/`, `core:heap/gc.rs`, `vm:types/mod.rs`, `vm:runtime/{execution_state,control,vm_state}.rs` | Baseline table reproduced; matrix 64/64; A/B neutral | 2–3 | Every later claim becomes measurable |
| **1. Context, funnel, roots** | `Mutator`/`Cx` replaces `Rc<RefCell<Heap>>` (codemod of about 1,300 sites; 78 borrow-then-allocate functions simplified); `Instance` split; store funnel; slot visitor plus generated tracer; `OwnedRoot`/`RootScope`; heap teardown; rooted loading boundaries A/B/D/E; `Step::LoadLibrary` | all crates; `core:heap/*`, `prim:*`, `fe:desugarer`, `rt:library_loader.rs`, `patina-interpreter` | Embedder probe (no use-after-free); teardown probe (`Weak` upgrade fails); rbtree load peak 211 → ≤ 130 MiB; dispatch A/B ≥ neutral; M2 barrier-simulation tax < 1% | 8–12 | Embedding sound; library bodies collect; no `RefCell` on the hot path |
| **2. Globals, code, continuations A** | `CellSpace` and variant R cells; non-moving code descriptors with traced liveness; continuation design A as a traced object; VM weak tables deleted; byte-accounted trigger | `core:environment.rs`, `core:library.rs`, `vm:types/code_object.rs`, `vm:runtime/*`, `core:heap/gc.rs` | `vm_global_cache`, `import_modifiers`, `finished_forms_release_code` green; `samedepth1000` RSS 3.4 GB → ≤ 200 MB; capture at d = 1000 24 µs → ≤ 8 µs; fib/tak A/B (no `frame_globals` `Rc` clone) | 6–9 | Global reads lose an `Rc` clone (2.6% of samples); continuations stop exhausting memory; code lifetime traced |
| **3. Rc-free, canonical object model** (still on arenas) | `HostTables` (ports, tree-walker payloads, macros, libraries) with registration lists; canonical RTD/port/procedure identity; `values_eq` = word compare; `identity-hash` API; identifiers = symbol + scope-set id + inline src; provenance tables deleted; records/promises/parameters/cells inline; deep-bound parameterization | `core:heap/*`, `core:scope.rs`, `fe:parser`, `fe:desugarer`, `patina-macros`, `prim:{records,parameters,lazy,io/ports}.rs`, `lib/scheme/base/parameters.scm` | libload malloc peak 765 → ≤ 450 MiB, load CPU −15–20% [libload §5]; E2/E4/E5 pass; `hygiene_matrix` 139/139; `DIVERGENCES.tsv` rows for any answer changes | 8–12 | Fixes four present-day defects; the largest memory pathology shrinks |
| **4. New heap: major-only hybrid** | `patina-gc` crate; reservation, blocks, metadata, `VmCtx`/`GcAttrs`; new value encoding including self-tagged flonums; layouts from `layout!`; nursery bump allocation; LOS; static compaction after bootstrap; every GC copies nursery blocks, marks old in place, evacuates sparse; Null and MOVE_ALL modes. Dual build (`--features new-heap`) in CI until parity, then the default flips and the arenas are deleted | new `crates/patina-gc`; `core:tagged_value.rs`, `core:heap/*`, `core:numeric.rs`, `vm:runtime/*`, the datum writer | All lanes byte-identical across builds; move-all and verify lanes green; gcold RSS 628 → ≤ 80 MB; fibfp/mbrot allocations −80%; libload post-load pause 178 ms → ≤ 20 ms; no workload's max pause > 1.2× main | 14–18 | Bump allocation, no sweep, about 2.2× fewer bytes, memory returned, flonum boxes gone |
| **5. Generational** | field-log barrier (interpreter funnel), remembered-set buffer, minor collector, sticky mode, key-indexed ephemerons, byte pacing with external terms, symbol space and weak symbol table. **M5 gate**: generational vs barrier-only vs major-only | `patina-gc`, `vm:runtime/vm_state.rs` (barrier sites) | **K1 decides**; post-load and libload pauses ≤ 2 ms minors; queue3/deeprec within 5% of stage 4 | 8–10 | Dead young objects free; bounded pauses |
| **6. Stacks and continuations C′** | non-relocating register stack with interleaved headers; freeze/thaw chunks; underflow and overflow stubs; winders/handlers/prompts as heap lists with virtual depth | `vm:runtime/{execution_state,control,vm_state}.rs`, `core:continuation.rs`, tree-walker slice form | Matrix 64/64; `ctakdeep` 1.04 s/4 GB → ≤ 0.15 s/≤ 150 MB; `pingpong1000` 27 µs → < 1 µs; K6 | 8–12 | O(new) capture; JIT-ready stack |
| **7. Readiness** | `Mutator`/`GreenThread` split, two-mutator lane, safe regions; tree-walker `PinRoots` policy finalised | `vm`, tree-walker, `patina-gc` | Two-mutator lane green | 3–4 | SRFI 18 (M:1) can start |

Total: about **57–80 engineer-weeks**. Stages 0–3, plus the representation half of stage 4, are the common core
(C1–C14) that any candidate needs. The collector-specific part is about 25–35 weeks: copying, mark-in-place,
evacuation, barrier, minors and C′. Stages 2 and 3 parallelise across worktrees.

**The first three PRs.**

1. **PR-1 "GC benchmark lane with pause and MMU log"** (issue: "No GC-sensitive benchmark or pause metric exists").
   - Adds `PATINA_GC_LOG` CSV pause and phase logging to `core:heap/gc.rs`.
   - Adds `scripts/gc_bench.sh`, which runs the demographics workload set from the Larceny reference checkout plus the
     three vendored extras, interleaved main/branch/main, and reports wall, RSS, max and p99 pause and MMU(10 ms).
   - Adds the continuation probes to `crates/patina-tests/bench_programs/gc/`.
   - Documents the lane in `docs/TEST_ORGANIZATION.md`.
   - *Acceptance:* reproduces [demographics §3] within noise on both backends; no other lane changes.
   - *Value:* no claim in this plan is checkable without it. Today's repo suite collects at most 4 times.
2. **PR-2 "Frames carry the closure as a value; one depth accessor"** (issue: "Bare `HeapIndex` in `CallFrame` blocks
   precise moving").
   - `vm:types/mod.rs` `closure: Option<TaggedValue>`.
   - `ExecutionState::depth()` and `register_window()` at all 37 `frames().len()` sites [cont-repr §6].
   - Deletes `visit_object_index` (`vm:runtime/vm_state/gc_roots.rs:153-160`).
   - *Acceptance:* `control_flow_matrix` 64/64 and `escape_from_primitive` on both backends; chibi; differential lane;
     A/B ±1%.
3. **PR-3 "Single store funnel with barrier hook"** (issue: "Unbarriered stores through `Rc` payloads and
   `vector_slice_mut`").
   - `Heap::store`/`store_range` become the only mutation API. `vector_slice_mut` is deleted (one caller,
     `vm:runtime/vm_state.rs:2380`).
   - `%record-set!`, parameter installs, promise forcing, `vector-fill!`/`vector-copy!` and `set-car!`/`list-set!` are
     routed through the funnel.
   - A `gc-barrier-sim` feature arms every old slot after each collection and runs the real filter, bit test and log, to
     measure M2.
   - *Acceptance:* all lanes; funnel ≤ 0.5% with the feature off; M2 tax reported for nboyer, destruc, quicksort,
     gcbench, hashtable0 and eqtable.
   - *Value:* the barrier inventory is closed and its interpreter cost known before anything depends on it.

---

## 16. Risks, mitigations and kill criteria

| Risk | Mitigation |
|---|---|
| **The largest prerequisite set of any candidate; the first collector benefit arrives at stage 4 (about month 8)** | Stages 1–3 each deliver standalone value (§15) and are needed by every candidate; MOVE_ALL plus verify lanes make the switch auditable |
| Big-bang representation switch (stage 4) | Dual build in CI until byte-identical parity; the representation is encapsulated (`HeapObjectData` named 32 times outside core [DIGEST §1.10]) once stage 1's `Cx` lands |
| Raw pointers turn rooting bugs into undefined behaviour | Debug bounds checks against the reservation; poison plus `PROT_NONE` from-space; verifier; Miri on `patina-gc` |
| Generational loses (Wingo; queue3/deeprec) | Major-only is built first (stage 4) and stays a configuration; sticky mode; M5 gate |
| 16-B alignment padding | Paid back by headerless closures and records (§3.2); re-measure from the census at stage 4 |
| Self-tagged flonums cover only ±[2^-127, 2^129) with one tag | Canonical static zeros and specials; K5 |
| Identity-hash side table under heavy `eq?` hashing | Cost proportional to moved hashed objects; K4 |
| C′ virtual depth reproduces the #162/#163/#176 family | Design A first with the same chunk format; matrix gate; K6 |
| Tree-walker pinning leaves old space fragmented | Major-only with evacuation of unpinned blocks; "may lag" is accepted |
| Cranelift pinned register vs `CallConv::Tail` on aarch64 unverified [DIGEST App. A #3] | Pass `VmCtx` as the first fragment argument instead (Wasmtime style); the contract is unchanged |
| Tag ABI frozen before the JIT exists | Freeze at the end of stage 4 only after K5 is measured |

**Kill criteria** (measured on the §14 GC benchmark set, interleaved A/B):

- **K1 (generational).** At the stage 5 gate:
  - ship *major-only* if generational shows a geometric-mean wall-time regression worse than 1% **and** improves
    per-workload p95 max pause by less than 1.5×;
  - ship generational if it wins throughput on ≥ 14/20 workloads and loses more than 5% on none;
  - in between, ship generational with sticky mode tuned, and re-run after the JIT.
- **K2 (barrier).** If the M2 interpreter tax exceeds 1.5% on the store-heavy six, or JIT microbenchmarks show the
  field-log fast path more than 2× the card cost on vecsort/hashtab, switch to `BarrierKind::Card` with a young filter.
  Promoted blocks are already type-segregated and parseable. In-place blocks need an object-start bitmap (0.78%) plus
  dead-object masking by the retained mark bits [barrier-res §3].
- **K3 (sticky mode).** If queue3 or deeprec under stage 5 are more than 10% slower than stage 4, disable the nursery
  adaptively per heap (survival above 50% means major-only for that phase).
- **K4 (identity hash).** If eqtable is more than 10% slower than the stage 3 baseline, add a 32-bit hash field to
  headered objects and pin-on-hash pairs.
- **K5 (flonums).**
  - If fibfp/mbrot/nucleic gain less than 1.3×, or any fixnum workload regresses more than 1%, revert to 16 B nursery
    flonum boxes and keep tag `100` reserved.
  - Decided before the tag freeze.
- **K6 (C′).** If generator/coroutine/ctak benchmarks are more than 10% slower than design A, or a matrix row cannot be
  restored within 4 weeks, stop at design A.
- **K7 (poll).** If the tick-counter poll costs more than 1% over a 2-instruction flag poll on call-heavy JIT
  microbenchmarks, use the flag poll in production and keep ticks for deterministic builds only.
- **K8 (schedule).** If stage 4 has not reached lane parity in 18 weeks, land it with `move_policy = PinAll`: no
  evacuation, nursery blocks promoted in place, 128 B line reuse added. That is candidate A semantics on the same code.
  Moving waits.

---

## 17. Owner decisions (DIGEST §6)

| # | Decision | My default | Consequence of the alternative |
|---|---|---|---|
| 1 | Tree-walker role | *Answered:* kept, may lag → `PinRoots` + major-only heaps (§8.4) | Production-grade tree-walker = heap frames/continuations, 6–12 weeks [tree-walker §4c] |
| 2 | Rebinding semantics | Variant R in stage 2; recommend **C** after it (chibi/Chez/Racket), with `DIVERGENCES.tsv` rows | Staying on R keeps `VmClosure.globals`/link tables and a second load per JIT global |
| 3 | When `define` rebinds; mid-program imports | Under C, at compile time (C2), p8 an error as in every oracle | Today's behaviour pinned by 6 tests |
| 4 | Value encoding | §2: 61-bit fixnums, self-tagged flonums (one tag), raw 16-B-aligned addresses, 4-bit heap tags | NaN-boxing loses 61-bit fixnums; offsets add a base add per access |
| 5 | Strings and bignums | UTF-32 inline (O(1) `string-set!`, smallest primitive rewrite); heap-native limbs | UTF-8 halves string bytes but rewrites `strings.rs`/`characters.rs` (252 heap mentions) |
| 6 | Pauses | *Answered:* STW, throughput first, bounded → minor goal 2 ms, majors ∝ live | Incremental marking pluggable later (§11) |
| 7 | Shared-memory parallelism | **Not a goal now**: M:1 with N-ready interfaces (§12) | Yes → `threaded` feature program, 4–9 months [threads-rec §4] |
| 8 | JIT frame model | S1 fragments, VM frames, no Cranelift stack maps in any tier (§8.2) | S2 native call/ret needs unwinding and a depth cap; S3 needs deopt metadata |
| 9 | Async interrupts in JIT code | Yes, through the tick poll (Ctrl-C, preemption, profiler) | No → GC needs only entry polls, and non-allocating loops need none |
| 10 | Identity hashing | Side metadata bits plus a moved-hash table (§6.9) | Header hash word: +8 B per hashed headered object, and pairs still need a side scheme |
| 11 | Weakness scope | SRFI 124 + 254 ephemerons now; SRFI 125 weak keys via ephemeron buckets; guardians after stage 5 | Fewer facilities save little: the epilogue is designed for all of them |
| 12 | Port finalisation at GC | Yes (both oracles); `EMFILE` collect-and-retry; differential lane excludes it | No → R2/R3 stay broken |
| 13 | Embedding API | Both scoped (`with`) and owned (`OwnedRoot`) handles; mandatory teardown; multiple heaps per process | Scoped-only blocks long-lived host references |
| 14 | Dependencies | **No MMTk**; `patina-gc` depends on `libc` (or `rustix`) only | MMTk: singleton per process vs 318 heaps, worker threads, pair headers [immix-mmtk §10] |
| 15 | Footprint | 16 GiB address-space reservation per heap; √L headroom; decommit after majors; 3% free reserve | A tighter headroom trades GC time for RSS |
| 16 | Front-end sequencing | Inline provenance and identifier ids **before** stage 4 (stage 3); lazy scope propagation recommended in parallel | Without them, load-time memory stays 316 MiB higher and syntax objects keep `Drop` |
| 17 | syntax-case timing | After stage 5; transformer entry runs a minor, with an evacuation-free epoch for the expander [libload §7.7] | Earlier → expander root provider and literal pool move into stage 3 |
| 18 | Continuation end state | **C′** (stage 6), design A as the stage-2 stepping stone | Stop at A: O(depth) capture remains |
| 19 | Divergences | Record the rebinding changes (if C); fix port `eq?` | — |
| 20 | JIT code memory | Per-unit code arena with its own W^X toggle in one install function; freed with the descriptor | `cranelift-jit`'s `JITModule` cannot free single functions |
| 21 | Budget | Representation first (stages 0–3), then the collector | Collector-first builds on arenas that are about to be deleted |
