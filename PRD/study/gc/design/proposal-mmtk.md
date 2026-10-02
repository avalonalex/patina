# Proposal: an MMTk-shaped collector for Patina, with MMTk as a gated engine

Panel philosophy: **MMTk-backed (or MMTk-shaped)**. Date 2026-10-01, repo `main` at `28a94f8`.
Inputs: `DIGEST.md` and the reports it cites (cited as `[report §n]`), plus mmtk-core `master` source read for
this proposal (`src/util/address.rs`, `src/vm/{object_model,scanning,collection}.rs`, `src/plan/barriers.rs`,
`src/plan/sticky/immix/global.rs`, `src/util/alloc/allocator.rs`, `src/memory_manager.rs`). **[V]** = verified
in source or measurement, **[I]** = inference or judgement.

**Deviation, stated up front.** I was asked to make the strongest case for building on mmtk-core. On the question
"which engine runs Patina's heap in production on day one" I conclude **no**. The reasons are structural, and I
quantify them in §1.1. I keep everything else of that philosophy:
- the object model is built to MMTk's `ObjectModel` contract, including headerless 16 B pairs, which I show fit
  that contract;
- the runtime/collector seam is a copy of MMTk's `VMBinding` decomposition;
- the in-tree collector follows MMTk's plan progression (Immix, then StickyImmix, then opportunistic evacuation);
- an MMTk adapter is a planned, time-boxed stage with adoption criteria written down before it runs, scheduled at
  the point where MMTk's lead is largest (the moving collectors).

---

## 1. Thesis and key bets

**Thesis.** The cost of Patina's GC upgrade is the representation and the rooting discipline, not the collector
algorithm. DIGEST §4.0 and immix-mmtk §10 agree on this. So build that representation once, to the most
demanding external contract available, which is MMTk's `ObjectModel`/`Scanning`/`Slot` binding interface. Then:
- run a small in-tree, single-threaded, per-heap Immix-family plan behind the same seam;
- export the allocation, barrier and poll fast paths as data (`GcAttrs`) that a Cranelift JIT reads;
- treat MMTk as a second implementation of that seam, measured against the in-tree plan before Patina writes its
  own moving collector. Moving is where MMTk saves the most code and where in-tree bugs are hardest.

The design holds if these six bets hold:

1. **Headerless 16 B pairs fit MMTk's contract** through *cdr-anchored* references. A pair's `ObjectReference` is
   its cdr word (start+8), so bit 3 of any reference tells a pair from a headered object. MMTk permits this:
   references must be word-aligned and inside the object, and `UNIFIED_OBJECT_REFERENCE_ADDRESS` /
   `OBJECT_REF_OFFSET_LOWER_BOUND` exist for references that are not the object start [V, mmtk `address.rs`,
   `object_model.rs`]. If this is wrong, only the MMTk option dies; the in-tree plan does not depend on it.
2. **Allocation never collects, and collection happens only at polls with no Rust temporaries live.** This is
   Chez's model under a moving generational GC, and Patina's model today (`heap/mod.rs:581-609` [V]).
3. **Sticky mark bits with opportunistic young evacuation are good enough generations.** They give generational
   behaviour without a separate nursery space, and the mode can be switched off. Survival on Patina's workloads is
   bimodal [demographics §5], so the M5 experiment decides whether generations ship.
4. **One field-logging barrier on armed log bits** serves this plan and MMTk's object/field barriers. Only the
   address fed to the bit test and the table layout differ, and both are `GcAttrs` constants [barrier-res §7].
5. **The baseline JIT keeps every Scheme value in VM-managed frames at safepoints** (S1/S2, no Cranelift stack
   maps). This makes the JIT independent of which collector runs [cranelift-gc §6.1].
6. **The representation rewrite can land kind by kind** in a mixed arena/block heap. No big-bang PR is needed.

### 1.1 The case for MMTk, and why it is not the production engine now

**For MMTk** [V unless marked]:
- It has every algorithm this roadmap wants: Immix, StickyImmix (moving or non-moving nursery), GenImmix,
  ConcurrentImmix, Compressor, LXR (merged to master 2026-08-19).
- It provides parallel work-stealing GC, pinning and transitive-pinning roots, VO bits, a MemBalancer-style
  trigger, `gc_poll` for safepoint-driven GC, and `mmtk_shutdown`.
- It is MIT/Apache and production-proven: Ruby ships it in-tree, and OpenJDK's C1/C2 inline its fast paths.
- Its parallel marking is the cheapest available way to bound **major** pauses on a multicore machine. Patina's
  worst pauses are majors (41 ms queue3; 178 ms post-load) [demographics §0].
- If the owner later answers "yes" to shared-memory SRFI 18 (decision 7), MMTk's multi-mutator STW and parallel
  GC are worth person-months.

**Against MMTk as the day-one engine**, each obstacle measured against this design:

| Obstacle | Fact | Can the design absorb it? |
|---|---|---|
| One instance per process, fixed VA layout (`mmtk.rs:44-58,114`; #100 open since 2020; #1347) | Up to 318 live heaps per `cargo test` process; 4,200 heaps over 276 processes [demographics §8.1]. Embedding wants independent interpreters | Only by making every interpreter in a process a *mutator of one shared heap*. Then one test's allocation pauses every other test thread, GC timing in `cargo test` depends on its neighbours, and the N-OS-thread safepoint handshake must be **correct on day one** across test threads, against threads-rec's "do not build OS threads now". **Not absorbable without cost** |
| GC on MMTk-spawned workers (`spawn_gc_thread`); `single_worker` is still a thread | At least two cross-thread handoffs per GC [immix-mmtk §10] | Absorbable after the common core removes `Rc` from roots (roots become raw memory). The latency cost is unmeasured [I: 10–40 µs per GC, about 0.3–2% at JIT-era minor rates]; the spike measures it |
| Untagged `ObjectReference` | Would force 24 B pairs [immix-mmtk §10] | **Absorbed** by cdr-anchored pairs (bet 1). Unproven; the first spike test |
| aarch64-apple-darwin is tier 2 ("guaranteed to build") since 2026-09-24 | This is the owner's development platform | Not absorbable by design; only time and upstream CI fix it |
| Nondeterministic parallel trace | Byte-identical lanes | Absorbable: `single_worker` plus a binding-owned deterministic trigger, plus identity hashes that do not depend on copy order in lanes (§6.8) |
| StickyImmix uses `ObjectBarrier` [V, `sticky/immix/global.rs`]. The slow path is an opaque Rust call | Cranelift treats every non-tail call as a safepoint. Object logging rescans whole large vectors (vecsort: 7 events × 20 K slots) [barrier-res §2.3, §6] | Partly: the attrs carry an `ObjectLog` mode. A call-free inline slow path needs an upstream API (§11.5) |
| About 30 direct dependencies; bindings pin git revisions; LXR is unreleased | `patina-core` depends today on only `num-*`, `rustc-hash`, `smallvec` | Feature-gated (`gc-mmtk`), off by default |

**Verdict [I].** The representation work (Stages 1–3 of §15) is identical either way. MMTk's value is
concentrated in what comes after it: evacuation, nursery copying, parallel and concurrent marking. So the decision
is deferred to Stage 5. By then the binding costs about 2–4 k lines over an already MMTk-shaped core, and every
obstacle above can be measured rather than argued. §11.5 lists what would have to change upstream; §16 gives
the adoption criteria.

---

## 2. Value encoding

### 2.1 The word

`TaggedValue(u64)`, `#[repr(transparent)]`, `Copy`. Low 3 bits are the primary tag. Heap references are **raw
virtual addresses** of the object's `ObjectReference`, plus the tag. They are not offsets from a per-heap base.

**Why raw addresses.**
- MMTk's `ObjectReference` is an address.
- `car` becomes one load with the tag folded into the displacement.
- A 64-bit word gains nothing from base-relative offsets, because 32-bit compressed fields would break 61-bit
  fixnums [heap-repr §13].

**Why a per-heap VA reservation anyway.** It holds blocks and side metadata so the metadata bias is per heap (§4).

### 2.2 Primary tags

Gambit-style float self-tagging, with a different tag offset chosen so that a single bit over-approximates
"heap reference" for the barrier.

| Tag | Meaning | Notes |
|---|---|---|
| `000` | fixnum, 61-bit signed, `v >> 3` | Kept: tagged `adds`/`b.vs` works on raw words [jit-ready R1] |
| `001` | flonum, immediate, exponent class A: ±0.0, denormals, tiny | float rotated left 4, top 3 exponent bits were `000` |
| `010` | flonum, immediate, exponent class B: 1.7e-77 ≤ \|x\| < 2 | top bits `011` |
| `011` | other immediates, sub-tagged in bits 3–7 | §2.4 |
| `100` | **pair** (heap) | reference = start + 8 (cdr-anchored) |
| `101` | flonum, immediate, exponent class C: 2 ≤ \|x\| < 2.3e77 | top bits `100` |
| `110` | **procedure / closure** (heap) | headerless closure; word 0 = code reference |
| `111` | **headered object** (heap) | everything else |

**Float encoding (Melançon, Serrano and Feeley, OOPSLA'25).**
- Rotate the IEEE bits left by 4, so the top three exponent bits land in bits 0–2, then XOR with `0b001`.
- The tag set {000, 011, 100} becomes {001, 010, 101}. That leaves 000 for fixnums, as Gambit's +3 offset does.
- The paper reports that the three classes cover "0.0…7×10⁻²⁵¹ and 1.7×10⁻⁷⁷…2.3×10⁷⁷". ±0.0 needs no
  special case.
- NaN, ±inf and the rest are boxed: a 16 B `FLONUM` object.
- Flonums are 19.8% of all allocations, and 79–99.8% in float code [demographics §4]. Most of them disappear.

```
; encode, d0 -> x0 (aarch64)          ; decode, x0 -> d0
fmov x0, d0                           eor  x0, x0, #1
ror  x0, x0, #60      ; rotl 4        ror  x0, x0, #4
eor  x0, x0, #1                       fmov d0, x0
and  x1, x0, #7
lsr  w2, wFLOMASK, w1 ; FLOMASK=0x26 (tags 1,2,5), hoisted
tbz  w2, #0, Lbox     ; not representable: allocate 16 B box
```

### 2.3 Tests the JIT and interpreter use

| Test | Sequence |
|---|---|
| fixnum | `tst v,#7; b.ne` |
| pair / closure / object | `and t,v,#7; cmp t,#4/#6/#7` |
| exact "is heap" | bit 2 set and tag ≠ `101` |
| **barrier value filter** | `tbz v,#2,skip` |

The barrier filter is deliberately a *superset* of heap references. Floats with \|x\| ≥ 2 pass it, and at worst
they cost one duplicate remembered-set entry per armed slot per cycle, which a minor GC discards. A single-bit
test that is exact is impossible with three float tags and a closure tag: four tags carry each bit. I chose
the closure tag, which saves a header load on every call (calls are 10–31% of dispatches [jit-ready §2.3]),
over exactness.

### 2.4 Immediates under tag `011`

Bits 3–7 are a 5-bit sub-tag; the payload is in bits 8–63.

| Sub-tag | Values |
|---|---|
| `00000` constant | `#f` = `0x03` (one-word compare), `#t` `0x103`, `()` `0x203`, eof `0x303`, unspecified `0x403`, default-object `0x503` |
| `00001` char | `(cp << 8) \| 0x0B` |
| `00010` internal, **never visible to Scheme** | `UNBOUND` `0x13` (cell placeholder, replaces `FORWARDED`), `BROKEN` `0x113` (ephemeron broken state, distinct from `#f`), `POISON` `0xDEAD_0000_0013` (debug from-space fill), `FILLER` |
| `11111` | **object header marker** (`0xFB`). Never a value, so a header word never looks like a reference |

### 2.5 Forwarding

There is **no in-band forwarding sentinel**:
- forwarding *state* is the `FORWARDED` bit in the per-granule side metadata byte (§4.3), as in MMTk's
  `LOCAL_FORWARDING_BITS_SPEC` on the side;
- the new reference is written into the word **at the ObjectReference**: the cdr word of a pair, the header of a
  headered object, word 0 of a closure;
- this is MMTk's `LOCAL_FORWARDING_POINTER_SPEC` at offset 0.

A pair's car therefore stays readable in from-space. Debug torture mode then overwrites the whole from-space with
`POISON` and `PROT_NONE`s it (§14).

**Why keep state off the object.** No header bit is written by both mutator and collector [threads-rec item 12].
The same layout serves a CAS-installed forwarding state later.

---

## 3. Object model

### 3.1 Telling objects apart from an untagged reference

`scan_object(ref)` and `size(ref)` receive a reference with no tag, which is MMTk's constraint. Every object
starts 16 B-aligned, so:

1. `ref & 8 != 0`: a **pair**. Start = `ref − 8`, size 16.
2. Otherwise read `w0 = *ref`:
   - if `w0 & 0xFF == 0xFB`, it is a **headered object**; dispatch on the type byte;
   - if `w0 & 7 == 0b111`, it is a **closure**: `w0` is the tagged reference to its code descriptor, which lives
     in the non-moving code space and holds `nfv`.

Closures are headerless, as in Chez. A code descriptor is never a first-class Scheme value, so a closure's word 0
can never be confused with a header.

### 3.2 Header word (headered objects)

| Bits | Field | Writer |
|---|---|---|
| 0–7 | `0xFB` marker (tag `011`, sub-tag `11111`) | allocator |
| 8–15 | type code (u8) | allocator |
| 16–17 | identity-hash state: 00 unhashed, 01 hashed, 10 hashed-and-moved (hash word appended) | mutator sets 01; the GC writes 10 on the *copy* only |
| 18 | `IMMUTABLE` (literal constant) | allocator |
| 19–23 | type-specific flags (bignum sign, string kind, record opacity) | allocator or mutator |
| 24–63 | length, 40 bits (elements, chars, bytes or fields, by type) | allocator |

There are **no GC bits in the header**: mark, pin, forward and log all live in side metadata (§4.3). That is the
MMTk shape, and it removes mutator/collector read-modify-write conflicts. A `TYPE_INFO[type]` table gives the scan
kind (`Values{first, count}`, `None`, `Ephemeron`, `HostPayload`, `Code`) and the size function.

### 3.3 Layouts (sizes rounded up to 16 B)

| Kind | Tag | Layout (8 B words) | Size | Today |
|---|---|---|---|---|
| Pair | `100` | `[car][cdr]`, reference at the cdr word | 16 | 16 B slot in a relocating `Vec` |
| Flonum (boxed residue) | `111` | `[hdr FLONUM][f64]`, pointer-free | 16 | 72 B slot |
| Vector | `111` | `[hdr VECTOR n][e0..]` | 8+8n | 24 B slot + malloc |
| String | `111` | `[hdr STRING n][u32 chars…]` UTF-32 inline, pointer-free | 8+4n | 24 B slot + malloc (UTF-32) |
| Bytevector | `111` | `[hdr BYTEVECTOR n][bytes]`, pointer-free | 8+n | `Vec<u8>` payload |
| Bignum | `111` | `[hdr BIGNUM nlimbs, sign flag][u64 limbs]`, pointer-free | 8+8k | `num-bigint` payload, cloned per op |
| Ratnum / compnum | `111` | `[hdr][num][den]` / `[hdr][re][im]` | 32 | 72 B slot (+`Rc`) |
| **Closure** | `110` | `[code-desc ref][fv0..fvn-1]`, **headerless** | 8+8n (1 fv: **16**) | slot + `Vec` + `Rc<Environment>` |
| Cell (box) | `111` | `[hdr CELL][value]` | 16 | 72 B `RefCell` slot |
| Record | `111` | `[hdr RECORD n][rtd][f0..]` | 16+8n | slot + `Rc<RTD>` + `Rc<RefCell<Vec>>` |
| RTD | `111` | `[hdr RTD][name][parent][uid][field-names vector][flags fixnum][protocol]`, canonical | 64 | fresh wrapper per `%record-type-of` |
| Symbol | `111` | `[hdr SYMBOL][name string][hash fixnum][flags]`, non-moving space | 32 | 72 B slot + `Rc<str>` |
| Identifier | `111` | `[hdr IDENT, len field = scope-set id][symbol][src id u32 \| flags]` | 32 | 72 B + `Rc<str>` + 40 B `SmallVec` |
| Continuation | `111` | `[hdr CONT n][depth\|deliver\|flags fixnum][winds][handlers][prompts][reentry]` + frames (4 words each, all value-encoded) + registers | 8+8n | 5 cloned `Vec`s in weak `Rc` side tables |
| Code descriptor | `111` | `[hdr CODEDESC][constants vector][*const CodeObject as fixnum][jit entry table as fixnum][id fixnum][nfv/arity fixnum]`, code space | 48 | `Rc<CodeObject>` + `live_closures` count |
| Ephemeron | `111` | `[hdr EPHEMERON, BROKEN flag][key][value][gc link]` | 32 | 72 B slot |
| Port | `111` | `[hdr PORT][port id fixnum][dir/flags fixnum]`, pointer-free, canonical | 32 | slot + `Rc<Port>`, new wrapper per call |
| Promise / Parameter / Values | `111` | inline value fields | 16–32 | `Rc<RefCell>` payloads |
| Host payload (residual Rust data, FFI) | `111` | `[hdr HOST][slot id fixnum][type id fixnum]`, see §3.4 | 32 | 19 Drop-carrying variants |
| Global cell | `111` | `[hdr GCELL][value][name symbol][meta fixnum]`, cell space | 32 | `Environment` slot + `FORWARDED` links |

**Raw pointers inside objects** (`*const CodeObject`, JIT entry tables) are 8-aligned. They are therefore valid
fixnum bit patterns, so every word of a pointerful object is a valid value word, and scanning is uniform. This is
Chez's trick.

**Frame words in continuations** have four fields:
- `code`: a tagged code-descriptor reference;
- `closure`: a value;
- `meta`: a fixnum packing pc (24 bits), return register (16) and register count (16);
- `base`: a fixnum offset.

**Net effect [I].**
- About 1.0× the "hypothetical layout" bytes, against 2.05× today [demographics §4.4].
- Closures with 1 or 3 free variables are 16 B smaller than in the hypothetical headered layout. Closures are
  39.7% of bytes.
- Most flonum allocations are gone.

### 3.4 Rust `Drop` payloads

**No heap object owns a Rust value with `Drop`.** Residual Rust-owned data goes into a per-heap
`HostPayloadTable`. It is a slab of `Box<dyn HostPayload>` addressed by `u32` id, with registrations split into
young and old lists. Residents:
- tree-walker `CpsLambda`/`CpsContinuation` [tree-walker §5.3];
- `CompiledMacro` bodies (their literals move into a heap vector);
- namespaces behind environment specifiers;
- FFI objects.

```rust
pub trait HostPayload: 'static {
    fn trace(&mut self, v: &mut dyn RootVisitor) {}   // TVs it holds: updatable slots or pins
    fn external_bytes(&self) -> usize { 0 }           // fed to the trigger (§13)
}                                                     // Drop = the finalizer: Rust-only, no Scheme,
                                                      // no GC-heap allocation [finalization R4]
```

A dead payload is dropped in the epilogue (§6.9), and only payloads registered in the collected generation are
visited. Expected population: ≤1% of allocations. Today's census of Drop-carrying kinds (47% of allocations)
falls to ports, macros, libraries and tree-walker payloads [finalization §4.3].

---

## 4. Heap organisation

### 4.1 Spaces

Spaces compose into a plan, as in MMTk. The allocation semantic follows MMTk's `AllocationSemantics`.

| Space | Moving? | Contents | Semantic |
|---|---|---|---|
| **Immix** (main) | Opportunistic, Stage 6+; off for tree-walker heaps | everything ≤ 8 KiB not listed below | `Default` |
| **LOS** | Never | objects > 8 KiB; buffers lent to Rust/FFI | `Los` |
| **Non-moving** | Never | interned symbols; canonical RTDs of the boot set; pinned-at-allocation objects | `NonMoving` |
| **Code** | Never | code descriptors (marked; released by finalization) | `Code` |
| **Cell** | Never; append-only, never freed | global binding cells (2,806 cells = 90 KB for the R7RS-large set [global-cells §7]) | `Immortal` |
| **Immortal boot** (Stage 6) | Never | live bootstrap heap, frozen after `(scheme base)` loads | `Immortal` |

- There is **no separate nursery space**. Young objects are allocated into free lines of Immix blocks and are
  told apart by sticky mark bits.
- A copying nursery (GenImmix) is not built in-tree. It is evaluated only through the MMTk adapter (§11.5).
- Pointer-free kinds share Immix blocks. `TYPE_INFO` skips scanning them. Unlike Chez, a separate data space buys
  nothing here, because card scanning is not used [barrier-res §0].

### 4.2 Sizes, with their evidence

| Parameter | Value | Why |
|---|---|---|
| Granule / alignment | 16 B | 56% of objects are exactly 16 B; 16 B alignment makes bit 3 identify pairs; the identity-hash word often fits in padding |
| Line | **128 B** (MMTk uses 256) | 99.2% of objects are ≤128 B, mean 26.9 B [demographics §9.3]; Immix: "line size tracks the object demographics" [immix-mmtk §2] |
| Block | 32 KiB = 256 lines = 2 pages of 16 KiB | Immix/MMTk default; also the decommit unit on macOS |
| Medium objects | 128 B – 8 KiB, overflow bump allocator, exact line marking | 0.8% of objects by count, but gcold's 816 B vectors dominate its bytes |
| LOS threshold | 8 KiB | 148 of 348 M objects exceed it; a block holds ≥ 4 Immix objects, bounding block fragmentation to about 25% |

### 4.3 Side metadata

Each heap reservation is laid out as `[block table][line marks][granule bytes][block region][LOS region]`. All
metadata is reached by `bias + (addr >> k)`.

**Granule byte**, 1 B per 16 B (6.25%), Whippet-like:

| Bit | Meaning | Written by |
|---|---|---|
| 0 | `MARK` (valid only if the block's epoch is current) | GC |
| 1 | `PINNED` | mutator (pin API, pin-on-hash) or GC (pinning roots) |
| 2 | `FORWARDED` | GC (evacuation) |
| 3 | `HASHED` for headerless kinds (pairs, closures) | mutator |
| 4 | `EPH_KEY`: a pending ephemeron is keyed on this object | GC |
| 5 | reserved (`BUSY` for a future parallel CAS forward) | — |
| 6, 7 | `LOG0`, `LOG1`: armed bits for the two 8 B words of the granule | GC arms; the mutator barrier disarms |

**Other metadata.**
- **Line marks**: 1 B per 128 B line (0.78%). The value is the line epoch (Immix mark-state cycling), so it never
  needs clearing.
- **Block table**: 16 B per block, holding space, mark epoch, live-line count, state
  (free/recyclable/full/evacuating), pinned count and flags.
- **Lazy clearing by epoch.** A major GC advances the epoch. The first time the tracer touches a block whose epoch
  is stale, it clears that block's `MARK`/`EPH_KEY` bits and stamps the epoch. That costs O(blocks touched), with
  no whole-heap clearing pass.
- **Hole reuse.** When the allocator takes a hole it zeroes the hole's granule bytes, clearing stale mark, log,
  pin and hash bits. That is 1/16 of the hole size; the hole's memory itself is not zeroed, because constructors
  initialise every field before the next safepoint.

Marking writes the granule byte anyway, so "mark + arm the object's log bits" is a single byte store for 16 B
objects (§7.4).

### 4.4 Per-heap reservation and decommit

- Each `Heap` reserves **32 GiB of VA** by default (`PATINA_HEAP_RESERVE`), `PROT_NONE`, and commits in 2 MiB
  units (64 blocks plus their 128 KiB of granule bytes).
- The measured worst case, 318 heaps × 32 GiB ≈ 10 TiB, is inside the 64 TiB probed successfully on this
  machine [demographics §8.2]. This requires heaps to die (§8.6).
- After a major GC, free blocks above `max(4 MiB, 0.25 × live)` are returned with `MADV_FREE` (macOS; measured
  to drop RSS at once) or `MADV_DONTNEED` (Linux), with one GC of hysteresis.
- The 464 MiB of empty arena capacity retained today after libload [libload §0.5] becomes returnable.

---

## 5. Allocation

### 5.1 Fast paths

The `Mutator` (§12) holds an MMTk-compatible `#[repr(C)] BumpPointer { cursor, limit }` at offset 0.

```rust
#[inline(always)]
pub fn alloc_small(&self, bytes: usize /* multiple of 16, <= 128 */) -> Address {
    let cur = self.cursor.get();
    let end = cur + bytes;
    if end <= self.limit.get() { self.cursor.set(end); Address(cur) } else { self.alloc_slow(bytes, AllocKind::Small) }
}
pub fn cons(&self, car: TaggedValue, cdr: TaggedValue) -> TaggedValue {
    let a = self.alloc_small(16);
    unsafe { a.write(0, car); a.write(8, cdr); }        // initializing stores: no barrier
    TaggedValue::from_ref(a + 8, TAG_PAIR)             // cdr-anchored reference
}
```

JIT `cons` on aarch64. `x21` is the Mutator pointer (Cranelift pinned register, or the first argument):

```
ldp   x1, x2, [x21, #0]      ; cursor, limit
add   x3, x1, #16
cmp   x3, x2
b.hi  Lslow                  ; cold: call rt_alloc_slow (a leaf: never collects)
str   x3, [x21, #0]
stp   xCar, xCdr, [x1]
add   x0, x1, #12            ; start + 8 (anchor) + 4 (pair tag)
```

That is seven instructions with no metadata stores. Whippet's mmc needs 8–10 because it writes start and end
bytes [DIGEST §4.1]. Inline sizes: pair, cell and flonum box (16), and closures up to 128 B. Everything else
goes through `rt_alloc(kind, n)`, which is also a leaf.

### 5.2 Slow path (never collects)

1. Next hole in the current block: scan line marks from the cursor, apply Immix's "skip the first line of a hole"
   rule for straddlers, zero the hole's granule bytes, set `cursor`/`limit`.
2. Otherwise a recyclable block, in address order.
3. Otherwise a free block from the pool.
4. Otherwise commit a new 2 MiB unit.
5. Medium objects use the overflow `BumpPointer` over empty blocks only. LOS objects get page runs.
6. Accounting: add the bytes consumed to `alloc_bytes` and raise the event word (§9) when the budget (§13) is
   crossed. The allocation still succeeds; this is overcommit, as in Chez's `S_get_more_room`.

### 5.3 Keeping "allocation never collects"

I keep the contract.
- 230–890 Rust functions hold unrooted values across allocation and are correct only because of it [DIGEST
  fact 1].
- JIT allocation sites are then not safepoints: no publish/reload, initializing-store barrier elision holds
  (JEP 475's condition), and SSA-held partial structures survive.
- Every report rejected the alternative, which is SpiderMonkey's three-year rooting effort and V8's 17,000
  `HandleScope`s [DIGEST §5 item 1].

**Under MMTk** the same contract is approached with three pieces (§11.5):
- `alloc_with_options(…, AllocationOptions{ at_safepoint: false, allow_overcommit: true, allow_oom_call: false })`;
- a binding-owned `Delegated` trigger whose `is_gc_required` is true only inside `gc_poll` called from Patina's
  safepoint;
- `post_alloc` folded into the slow path.

MMTk's documentation says `at_safepoint: false` returns null when a GC is needed [V, `allocator.rs`]. So the exact
interplay with overcommit and the delegated trigger is **spike test #2** [I].

### 5.4 Limits

- `soft = heap target` (§13): crossing it raises the GC event.
- `max_heap` (default: 80% of physical memory, configurable): crossing 90% sets `EMERGENCY`. The next safepoint
  runs a full GC; if occupancy is still above 90%, it raises a catchable Scheme `&heap-exhausted` condition.
- Reaching the reservation end in a slow path is an unrecoverable `oom_abort()` that prints heap statistics, as
  Chez does with `S_error_abort`. A program reaches it only through unbounded allocation inside a single Rust
  primitive between safepoints.

---

## 6. Collection

One plan, `ImmixPlan`, with three modes chosen per heap: **FullHeap** (non-generational), **Sticky**
(generational, non-moving), **Sticky+Evac** (Stage 6). STW, single-threaded, on the mutator thread, entered only
from a safepoint (§9).

### 6.1 Minor collection (Sticky)

- **Roots.**
  - registers of frames above each green thread's **stack watermark** (§8.1);
  - root scopes and handle tables;
  - young host-payload registrations;
  - the remembered set: barrier SSB entries (exact slot addresses) plus a "remember whole" list for objects
    allocated old.
- **Trace.** A young object is one not marked in the current epoch. Visiting a slot:
  - old: skip;
  - young: set `MARK` and arm its log bits (one fused store per granule), mark its start line, or the exact line
    range for medium objects, then push it;
  - Sticky+Evac: copy unpinned young survivors into clean blocks instead, leave the forwarding state, update the
    slot, and arm at the destination.
- **SSB entries.** Re-read `*a`; trace if it is young; update; re-arm `a`.
- **Weak epilogue** over young registrations only (§6.6).
- **Nothing is swept.** Dead young objects cost nothing until their lines are reused.

### 6.2 Major collection

1. Advance the mark and line epochs. In evacuating mode, select defrag candidates from the previous major's
   live-line counts:
   - start when fragmentation exceeds 10% and stop when it falls below 5% (Whippet mmc);
   - the evacuation reserve is 2.5% of the heap (Immix's default; sensitivity is low between 1 and 3%).
2. Trace from all roots: every frame of every green thread, root scopes, handle tables, the cell space, the
   immortal boot space as a root region, and all host-payload registrations.
3. Mark in place, or evacuate unpinned objects in candidate blocks until the reserve runs out. Mark and arm in one
   byte store.
4. Discard the SSB: it is re-derived from the marks (barrier-res §7.1 item 7).
5. Weak epilogue over all registrations, then release code (§6.7).
6. Block summary: a byte scan of line marks per block (256 B per block), classifying free/recyclable/full, plus
   decommit (§4.4). This is the only eager per-block pass, and it never reads object memory.

`(gc)` always runs a **major**, as the ephemeron tests require [finalization §4.6].

### 6.3 Marking order and the mark-stack bound

- The worklist holds *objects*. Each slot is updated as its parent is scanned, which serves evacuation too.
- Pairs are pushed **cdr first, then car**, so a car subtree is finished before the walk continues down the spine.
  The mark stack is O(1) per element on heap-car lists, against today's O(length) (+55 MB on 2 M two-vectors)
  [gc-impl; DIGEST pitfall 19].
- The stack is chunked (4 KiB segments) and capped at `max(1 MiB, heap/32)`. On overflow:
  - stop pushing;
  - flag the block holding the unscanned object;
  - finish the current stack;
  - rescan flagged blocks by walking their granule bytes for `MARK` bits (object starts), scanning those objects'
    children;
  - repeat until no block is flagged.

  This needs no heap parsability, because `MARK` sits only at start granules.

### 6.4 Evacuation and pinning

An object is never moved if any of these holds:
- its granule byte has `PINNED`, which comes from:
  - pinning roots (tree-walker Rust holders, debugger payloads, embedder `pin`);
  - pin-on-hash for headerless kinds (§6.8);
  - FFI calls;
- it lives in LOS, non-moving, code, cell or immortal space;
- its heap's policy is `moving = false` (tree-walker heaps).

Young pinned objects are promoted in place (gen-ZGC/JEP 423 style). A block whose pinned share exceeds 25% is not
a defrag candidate. When the reserve is exhausted, the remaining objects are marked in place (opportunistic
Immix). **Torture mode** (`zeal-move`) makes every block a candidate with an unbounded reserve (§14).

### 6.5 Sweep

The sweep is lazy and line-granular (§5.2). Its cost is proportional to the lines the allocator inspects, never
to the high-water mark. Today's sweep visits every arena slot at about 2.4 ns each, 1.1–3.9 slots per
allocation [demographics §3]; that disappears. The 178 ms post-load pause was 7.5 M dead slots being swept and
their `Rc` payloads dropped [libload §0.4]. It becomes a mark of about 35 MiB of live data.

### 6.6 Weak references and ephemerons: key-indexed, one fixpoint

- **Scanning an ephemeron E.** If key K is immediate, marked, or old during a minor, trace value V. Otherwise:
  - thread E onto a pending chain through E's GC link word;
  - insert K → chain head into a pending map (FxHashMap keyed by K's reference);
  - set `EPH_KEY` in K's granule byte.
- **Marking or copying any object** checks `EPH_KEY` in the granule byte the tracer is writing anyway, at no extra
  memory access. If it is set, look up the chain and trace each value. The 16 k-chain case drops from today's
  O(n²) (289 ms) to O(n) [finalization §0.7].
- **Fixpoint.** When the worklist is empty, run the host-payload weak-id step: trace the payloads of ids whose
  reference objects are now marked. Then drain again. Guardians (SRFI 254, later) are the last step. Loop until a
  round makes no progress.
- **Break.** Each pending E gets the `BROKEN` flag and its key and value set to `BROKEN`.
- **Generations.** Ephemerons are allocated only through the runtime (never inline, never pretenured), so E is
  never older than K or V. Minors skip old ephemerons, and ephemeron edges need no remembered-set entries
  [finalization §4.6].
- **Under MMTk** the same algorithm runs in the binding's `scan_object` (checking a side bit) and in
  `process_weak_refs(…) -> bool` (re-invoked until false) [V `scanning.rs`]. It needs `scan_object` to run for
  pointer-free objects too. The docs do not say it does: **spike test #4**.

### 6.7 Finalization, ports and code liveness

**Ports (R1–R6, [finalization §2.2]).**

| Requirement | Mechanism |
|---|---|
| R6: one port, one object | A port is the canonical `[hdr PORT][id]` heap object; the `PortTable` slab owns `PortData`; `current-*-port` parameters hold the heap value |
| R2: flush and close a port proven dead | Registration lists per generation. After a minor, unmarked young entries are finalized (flush ignoring errors, close, free slot) and marked ones promoted; after a major, unmarked old entries are finalized. Each epilogue processes entries **in port-id order**, so the result is deterministic |
| R1: output written at exit | `end_process` flushes every table entry, live or not yet finalized |
| R5: interpreter drop flushes and closes | Heap `Drop` finalizes the whole table |
| R3: no `EMFILE` failure while garbage holds descriptors | Each file port charges 8 KiB to `external_bytes`. Opening `min(128, RLIMIT_NOFILE/4)` file ports since the last GC raises the event. On `EMFILE`/`ENFILE` the open primitive returns `Step::RetryAfterMajorGc`, a resumable step: the machine runs a major at the safepoint, then resumes the primitive once |
| R4: no Scheme, no GC-heap allocation | Finalizers are Rust-only |

GC-time flushing is observable, so its tests run outside the differential lane.

**Code liveness.**
- Code descriptors live in the non-moving code space and are marked like any object, through closure word 0,
  frame `code` words, continuation frames and constant vectors.
- After a **major**, each unmarked descriptor is finalized: its `Box<CodeObject>` and any JIT code are freed.
  Minors release nothing.
- This deletes `live_closures` and `gc_freed_closure_code_ids` [cont-repr §4], and keeps
  `finished_forms_release_code.rs` ("released within a bounded number of collections") with "bounded" meaning
  "by the next major".
- A JIT function with a native activation at the safepoint is reachable through its VM frame's `code` word, so it
  is never freed underneath itself.

### 6.8 Identity hash

The scheme is Bacon–Fink–Grove address-based hashing, the scheme MMTk documents:

```
identity_hash(o) = mix32(o.ref - heap_base)            // relative to the heap: identical across runs (no ASLR)
  headered:  unhashed → hashed (header bits 16–17 = 01, set by the mutator)
             GC copy of a hashed object: append the hash word (or use existing padding), state = 10
  pair, closure (headerless): first hash sets PINNED|HASHED in the granule byte (Guile's choice for hashq)
  symbol: stored hash field;  immediates: mix(bits)
```

- One instruction sequence per call, with no table probe. eqtable makes 1.66 M calls [demographics §7.1].
- **Determinism.** Allocation and copy order are deterministic in a single-threaded collector, so hashes are
  identical run to run within one GC mode. They differ *between* GC modes, as today's index-based hashes already
  do, and the differential lanes pass today [V, finalization §0.4].
- Under parallel copying (MMTk production with >1 worker), hashes taken after a move would vary run to run. That
  is recorded as an MMTk adoption cost (§16).

### 6.9 Epilogue order

1. Trace to fixpoint: marking/evacuation, inline key-indexed ephemeron resolution, the host-payload weak-id step,
   (later) guardians.
2. Break pending ephemerons.
3. (Later) SRFI 254 transport cells: enqueue entries whose key moved.
4. Weak-key tables: rekey forwarded entries, drop dead ones. Users: the weak symbol table (later) and any residual
   provenance keyed by id.
5. Weak-id/host-payload tables: drop dead entries (queue their `Box` for drop) and promote the survivors'
   registrations.
6. Finalization registry: ports, then host payloads, in id order.
7. Code release (majors only).
8. Block summary, decommit, pacing update, statistics record.
9. Clear the event's GC bits and resume.

---

## 7. Write barrier

### 7.1 Kind

- Field logging, **pre-write**, on armed log bits. Armed means the first store this cycle must be logged.
  Polarity: fresh memory is 0, so young holders are unarmed by construction.
- The value pre-filter is bit 2 (§2.3).
- The slow path is **inline and call-free**: disarm, append the slot address to a per-mutator SSB, and request a
  GC when the soft limit is crossed [barrier-res §7].

### 7.2 Fast paths (aarch64)

Slot address is `x1`, value `x2`, granule-byte bias `x27` (loaded once per function from `[x21, #LOG_BIAS]`).

```
; static field: set-car! (word 0, LOG0 = bit 6), set-cdr!/WriteCell (word 1, LOG1 = bit 7)
tbz   x2, #2, Lstore         ; value filter (fixnum, two float classes, immediates skip)
lsr   x3, x1, #4
ldrb  w4, [x27, x3]
tbnz  w4, #7, Lslow          ; armed
Lstore: str x2, [x1]

; dynamic index: vector-set!
tbz   x2, #2, Lstore
lsr   x3, x1, #4
ldrb  w4, [x27, x3]
ubfx  x5, x1, #3, #1         ; word parity
add   x5, x5, #6
lsr   w6, w4, w5
tbnz  w6, #0, Lslow
Lstore: str x2, [x1]
```

That is 4 instructions for static offsets and 7 for dynamic ones, plus the store. The field offset parity of every
16 B-aligned layout is known at JIT time (pair car 0, cell value 1, record field *i* at word `2+i`, closure fv
*i* at word `1+i`).

```
Lslow:                        ; cold block, no call
mov   w6, #1
lsl   w6, w6, w5             ; or a static constant
bic   w4, w4, w6
strb  w4, [x27, x3]          ; disarm (in a threaded build: AtomicU8::fetch_and, Relaxed)
ldr   x7, [x21, #REMSET_CUR]
str   x1, [x7], #8
str   x7, [x21, #REMSET_CUR]
ldr   x8, [x21, #REMSET_SOFT]
cmp   x7, x8
b.lo  Lstore
ldr   w9, [x21, #EVENT]  ;  orr w9, w9, #EV_GC_MINOR  ;  str w9, [x21, #EVENT]
str   wzr, [x21, #TICKS]     ; force the next poll into its slow path
b     Lstore
```

- The SSB is a 256 MiB per-mutator VA reservation, committed lazily, with a soft limit of 1 MiB (128 K entries).
- Polls bound straight-line code (codegen emits only forward jumps [jit-ready §2.2]), so the hard end is
  unreachable.
- Because there is no call, tier-2 Cranelift code does not spill all stack-map values at this site
  [barrier-res §6].

### 7.3 Filters and elision

| Rule | Where |
|---|---|
| Static immediate value: literals, chars, booleans, `'()`, results of `Not`/`NullP`/`PairP`/`Eq`/`Lt`/`NumEq*` (not arithmetic, which can make bignums) | codegen, both tiers |
| Initializing stores: holder allocated with no safepoint since. Constructors take their contents (`alloc_vm_closure(captured)`), and the JIT groups a block's allocations | Rust constructors; JIT |
| Register-file and frame stores are roots | — |
| Strings, bytevectors, bignums are pointer-free | — |
| Continuations are immutable after capture; large ones live in LOS as *young* objects (§6.1), so pre-logging is needed only for objects allocated old (boot freeze) | — |
| `letrec*` write-once defines unboxed, removing most `WriteCell`s (93% of nboyer's barrier stores) | compiler work item [jit-ready R13] |

Young holders are free: their bits are unarmed. Old→old shuffles (vecsort's 407 K) are filtered only by the bit,
so each armed slot is logged at most once per cycle. Measured: vecsort 51 K field-log entries against 7
whole-object rescans of 20 K slots each [barrier-res §2.3].

### 7.4 Arming

- **Minor:** marking (promoting) a young object stores `MARK|LOG0|LOG1` into each of its granule bytes. That is
  one store for 16 B objects and *n*/16 for larger ones.
- **Major:** the same, during marking.
- **Evacuation:** arm the destination; the source bytes die with the source lines.
- **Cell space and immortal space:** armed at creation or freeze.
- **Hole reuse** zeroes bytes (§4.3), so no freed granule stays armed.

### 7.5 The single store funnel (Rust)

```rust
impl Mutator {
    #[inline(always)]
    pub fn write(&self, holder: TaggedValue, slot: Slot, v: TaggedValue) {
        if ATTRS.barrier != BarrierKind::None && v.bit2() {
            barrier::<ActivePlan>(self, holder, slot, v);   // #[inline(always)] test, #[cold] log
        }
        unsafe { slot.store(v) }
    }
    pub fn set_car(&self, p: TaggedValue, v: TaggedValue);       // → write(p, p.car_slot(), v)
    pub fn set_cdr(&self, p: TaggedValue, v: TaggedValue);
    pub fn vector_set(&self, vec: TaggedValue, i: usize, v: TaggedValue) -> Result<(), RangeError>;
    pub fn record_set(&self, r: TaggedValue, i: usize, v: TaggedValue);
    pub fn cell_set(&self, c: TaggedValue, v: TaggedValue);
    pub fn global_set(&self, c: CellRef, v: TaggedValue);
    pub fn write_range(&self, holder: TaggedValue, first: Slot, len: usize); // vector-fill!, vector-copy!
}
```

- `holder` is passed even though field logging ignores it, because MMTk's `object_reference_write_post(src, slot,
  target)` needs it. That parameter is the whole cost of keeping the ObjectLog mode open.
- `vector_slice_mut` is deleted. Its single non-test caller is the VM `VectorSet` arm [V,
  `vm_state.rs:2380`].
- Record, parameter and promise stores that today bypass the heap through cloned `Rc` handles (`records.rs:262`,
  `parameters.rs:168,267`, `lazy.rs:134`) become heap stores through the funnel.
- **Interpreter cost [I]:** under 1%. Every store already pays a `RefCell` borrow and a bounds check; this is M2.

---

## 8. Roots and rooting

### 8.1 VM frames

- Each green thread owns a **non-relocating register stack**: reserved VA (4 GiB for the main thread, 64 MiB
  for others, growable later by C′ overflow chunks), committed on demand, with an explicit limit check on push.
- Frames are `#[repr(C)] Copy`, 32 B: `{ code: TaggedValue /*code desc*/, closure: TaggedValue, meta: u64
  /*pc|ret|nregs*/, base: u64 }`.
  - `CallFrame.code: Rc<CodeObject>` and its four refcount operations per call/return go away.
  - `closure: Option<HeapIndex>`, an untraceable bare index, also goes away [cont-repr §0.2].
- **Liveness.** The per-pc may-root maps (#423) are kept, compressed to safepoint pcs only (today about 40 B per
  pc [libload §9]). `retire_registers` still writes `UNSPECIFIED` into dead slots before a GC, which keeps the
  four #423 ephemeron tests passing.
- **Stack watermark** for minors: `wm = min(wm, depth)` on every `Return` (before writing the caller's `dst`), on
  escape, on reinstatement and on debugger writes. A minor scans frames `[wm, top)` only, then resets `wm = top`.
  Frames below the mark were traced at the previous GC and have not been written since, so they hold only old
  references. deeprec's 11.5 M registers stop being rescanned per minor [demographics §7.2].

### 8.2 JIT frames

- **Tier 1 (S1 fragments with `tail` calls, or S2):** no native roots at any safepoint. Live values are published
  to the register window before every may-GC helper or poll slow path, and every base and cached value is
  reloaded afterwards [jit-ready §5 D16]. No Cranelift stack maps are used. Machine code never embeds a movable
  reference: constants are loaded through the code descriptor's constant vector. Symbols and cells are
  non-moving, so they may be immediates.
- **Tier 2 (optional, later):**
  - Cranelift user stack maps (`declare_value_needs_stack_map`), frame-pointer walk, and a code registry keyed by
    return address; slots are updated in place.
  - No derived pointers across safepoints.
  - Barrier slow paths stay inline (§7.2), so they force no spills.
  - Continuation capture deoptimizes native frames into VM frames, so tier-2 code needs all-values deopt
    metadata at capture points. Otherwise it is restricted to functions proven not to reach `call/cc`.

### 8.3 Rust code

- **Context.** The `Mutator`, borrowed as `&Mutator` (shared; allocator and barrier state in `Cell`s). Primitives
  get `Cx<'_> { m: &Mutator, inst: &Instance }` [prim-embed §11]. Only the driver, holding `&mut Heap`, can call
  `collect`.
- **Heap-only primitives** hold bare `TaggedValue`s freely, because nothing can collect under them. That is about
  95% of the ~1,300 sites, migrated by a receiver codemod.
- **Re-entry boundaries** use a LIFO `RootScope` on the current green thread:
  ```rust
  let scope = cx.root_scope();               // RAII; truncates the root stack on drop
  let body = scope.root(parsed_body);        // Rooted: an index into the stack
  … nested loop or library load: may collect …
  let v = body.get(cx);                      // reload; the collector updated the slot
  ```
  `Global` handles (index plus generation into a traced table; `Drop` frees) cover arbitrary lifetimes.
- **Resumable primitives** (`Step::Call`) already keep values in their step state. That state becomes a heap
  object or a `Rooted` value.
- **Raw-bits transient maps** (writer, parser, desugarer `quoted`) remain legal inside one no-GC region.
  `identity_bits()` is documented as "valid until the next safepoint".

### 8.4 Tree-walker policy (it may lag)

- **Same object model and allocator, but `HeapPolicy { moving: false, generational: false }`.** The tree-walker
  heap runs FullHeap Immix: no evacuation, no arming, so the barrier slow path is never taken.
- Its Rust-held values (`StepResult`, `ContValue`, `CpsExpr` literal pools, `Rc<Environment>` frames) are
  reported through the same `RootVisitor`, marked as **pinning** roots: enumerated, never updated. Under the
  in-tree plan that is moot, because nothing moves. Under a shared MMTk instance it becomes MMTk's
  `create_process_pinning_roots_work`, which is what Ruby does.
- `CpsLambda`/`CpsContinuation` become host payloads with the generational weak-id rule (1–3 weeks
  [tree-walker §4]).
- It collects only at outermost trampoline safe points, plus registered nested trampolines later. Its off-heap
  allocation is charged to `external_bytes` per payload.

### 8.5 Fate of each off-heap holder

| Holder today | Becomes |
|---|---|
| `Environment` globals, `FORWARDED`, `Owner` links | Cells in the cell space (variant R first). Imports share the exporter's cell, giving #406 by construction. Namespaces hold `CellRef`s only [global-cells §3] |
| `Library.exports: HashMap<String, TV>` | `name → CellRef`; no value copies |
| `CodeObject.constants: Vec<TV>` in `Rc` | A heap vector referenced from the code descriptor (traced, updatable); boot constants in immortal space after Stage 6 |
| `CompiledMacro` literals; `foreign_expansions` | A literals vector in the `MACRO` heap object; the rest is a host payload; `CompiledMacro.heap` is deleted |
| `Heap.syntax_sources`, `SourceMap.locations`, child spans | Deleted, or replaced by an inline `src` id in identifiers plus per-document `u32`-keyed location tables [libload §5.3] |
| VM continuation stores, `VmContinuationRef` | Deleted: continuations are heap objects (design A) |
| `WindRecord.handlers: Rc<[…]>` | Heap lists (persistent winders), with design A, at the latest by C′ |
| `symbol_table` (immortal, re-marked per GC) | Symbols in the non-moving space; the table is `FxHashMap<Box<str>, SymRef>`, later a weak-key table |
| Tree-walker `CpsExpr` literals | A per-form literal pool, traced (pinning) [tree-walker §5.4] |
| `PENDING_ESCAPE` thread-local | A field of the green thread |
| Embedder-held results (`eval_*`) | `Global` handles. This fixes the measured use-after-free [prim-embed §7.2] |

### 8.6 Heap teardown

- `Environment.heap` and `CompiledMacro.heap` disappear: nothing below the interpreter owns the heap. Dropping the
  `Interpreter` drops `Heap`, which finalizes the port and host-payload tables and unmaps the reservation.
- This fixes the leak (`strong_count` 36–37) [prim-embed §7.3] and lets 4,200 reservations per test run come
  and go.

---

## 9. Safepoints and polling

### 9.1 The event word

`Mutator.event: AtomicU32`, with `Relaxed` plain loads and stores in the single-threaded build:

| Bit | Purpose |
|---|---|
| `GC_MINOR`, `GC_MAJOR` | collection requested |
| `EMERGENCY` | near `max_heap` (§5.4) |
| `DEBUG_BREAK` | debugger [tree-walker §6] |
| `PREEMPT` | green-thread quantum expired |
| `SIGNAL` | Ctrl-C |
| `TERMINATE` | `thread-terminate!` |
| `HANDSHAKE` | reserved for N mutators |

Alongside it: `Mutator.ticks: i32`.

### 9.2 The poll

The poll is Chez's `%trap`: decrement `ticks`, and take the cold path when it reaches ≤ 0. Raising any event
also stores `ticks = 0`.

```
ldr  w9, [x21, #TICKS]
subs w9, w9, #1
str  w9, [x21, #TICKS]
b.le Lpoll_cold            ; rt_safepoint(mutator): publish, handle events, reload
```

**Placement.**
- JIT: function entry (which is also the self-tail-call loop header) and returns to the driver. Codegen emits only
  forward jumps, so every loop passes one of these [jit-ready R14].
- Interpreter: the same check moves from *every dispatched instruction* (1.1–1.4% today [DIGEST §1.9]) to `Call`,
  `TailCall` and `Return` dispatch.

**The cold path** reloads `ticks` from the quantum, which defaults to 10,000 polls. It sets `PREEMPT` only when
another green thread is runnable, then services the event bits in a fixed order: `TERMINATE`, `SIGNAL`,
`DEBUG_BREAK`, `GC_*`, `PREEMPT`.

### 9.3 Deterministic mode

- Test lanes never let a timer write `ticks`, so preemption and GC points depend only on the program, and lanes
  stay byte-identical.
- A timer thread setting `ticks = 0` (async interrupts, profiling) is a production option (owner decision 9).
- Under N mutators, a cross-thread request's `ticks = 0` store can be lost to a racing decrement. It is then seen
  at the next quantum, which is threads-rec's stated latency bound.

### 9.4 Nested Rust loops (replacing `GcDeferGuard`)

- `is_outermost` becomes `unrooted_depth == 0`. Only boundaries that cannot root their Rust frames increment
  `unrooted_depth`:
  - expansion point C (inside `desugar_with_imports` after a mid-form import) [libload §4.2];
  - tree-walker trampolines whose `StepRoots` are not yet registered;
  - the remaining `apply_proc` fallbacks.
- Library loading points A, B and D, and nested VM loops, root their state with `RootScope`, so **library bodies
  collect**. Measured: between-form collection cut rbtree's peak from 211 to 118 MiB [libload §0.6].
- A GC event raised while `unrooted_depth > 0` stays pending (overcommit). `EMERGENCY` with `unrooted_depth > 0`
  is the one hard-limit hazard, and it is logged.

---

## 10. Continuations and stacks

**Representation, Stage 2: design A.**
- A full capture copies the green thread's frames (as value-encoded 4-word records) and live registers into one
  immutable `CONT` heap object.
- Dead registers are cleared through the liveness map at capture (the `deliver_reg` fix for the 296 MB leak
  [DIGEST §2.1]).
- The object is byte-accounted, goes to LOS when over 8 KiB, is young like any allocation, and is movable.
- Winds, handlers and prompts are referenced as heap lists.
- Delimited capture is the same object over `[prompt floor, top)`, and `append_delimited` stays eager.
- The weak side tables and the "store touched within one dispatch" soundness rule are deleted. Toy measurement:
  capture overhead about 5.8× lower [cont-repr §0.3].

**Multi-shot.** Reinstatement copies out (thaws) into the live register stack and never runs in place. Captured
state is never mutated, so continuations need no barrier, and invoking one twice is safe.

**dynamic-wind.** `wind_jump`/`value_wind` stub frames are unchanged. Wind records become persistent heap lists
(Chez `winders`), and `next_wind_step` compares list identities by depth. The control-flow matrix (64 rows, both
backends) gates every change.

**C′ (later, owner decision 18).**
- Frozen watermark plus immutable chunks plus an underflow stub frame.
- Capture costs about 57 ns regardless of depth, against 5.0 µs today at depth 1000 [cont-repr §0.4].
- The same `Return`-time `min` serves both the GC watermark (§8.1) and the frozen watermark.

**JIT.** S1 fragments resume at per-return-point entries. The native stack is never captured. Escapes return a
new target to the trampoline.

**GC interaction summary.**
- Continuations are ordinary traced objects. Their frame words keep code descriptors alive.
- Reinstatement lowers the stack watermark to the reinstated base.
- A future incremental mode must darken a continuation's contents on reinstatement, as OCaml fibers do. The rule
  is written down now and costs nothing in STW modes.

---

## 11. Pluggability contract

### 11.1 Shape

The contract follows MMTk's split: the runtime is the *binding*, and a collector is a *plan* composed of spaces.
Fast paths are *data* (Whippet `gc-attrs.h`). Selection is static.

```rust
// crate patina-gc (new; depends on nothing but std/libc)
pub struct ObjRef(NonZeroUsize);              // word-aligned, inside the object (MMTk's ObjectReference rules)
pub struct Slot(*mut TaggedValue);            // load(): Option<ObjRef> (exact heap test); store(r) keeps the old tag

/// Implemented once, by patina-core. The MMTk VMBinding analogue.
pub trait Binding: 'static {
    const REF_OFFSET_LOWER_BOUND: isize = 0;  // MMTk OBJECT_REF_OFFSET_LOWER_BOUND
    const UNIFIED_REF_ADDRESS: bool = false;  // pairs are cdr-anchored
    fn object_start(r: ObjRef) -> Address;    // r & !15
    fn size(r: ObjRef) -> usize;              // includes an appended hash word
    fn size_when_copied(r: ObjRef) -> usize;  // +8 if hashed and no padding
    fn copy_to(r: ObjRef, to: Address) -> ObjRef;
    fn scan_kind(r: ObjRef) -> ScanKind;      // Values / None / Ephemeron / HostPayload / Code
    fn scan_object(r: ObjRef, v: &mut impl SlotVisitor);
    fn scan_roots(cx: &RootCx, v: &mut impl RootVisitor);       // slots, pins, transitive pins
    fn weak_step(cx: &RootCx, t: &mut impl Tracer) -> bool;     // MMTk process_weak_refs: re-run while true
    fn epilogue(cx: &RootCx, liveness: &impl Liveness);         // §6.9 steps 3–8
}
pub trait RootVisitor: SlotVisitor {
    fn pin(&mut self, r: ObjRef);             // MMTk create_process_pinning_roots_work
    fn pin_transitively(&mut self, r: ObjRef);// MMTk create_process_tpinning_roots_work
}

/// Implemented by each collector. The MMTk Plan analogue.
pub trait Plan: Sized + 'static {
    const ATTRS: GcAttrs;                                       // read by the JIT and by Mutator
    fn new(cfg: &HeapConfig) -> Self;                           // per heap; HeapPolicy{moving, generational}
    fn bind_mutator(&self, m: &mut MutatorAbi);                 // initialize the #[repr(C)] fields
    fn alloc_slow(&self, m: &MutatorAbi, bytes: usize, sem: Semantics) -> Address; // never collects
    fn post_alloc(&self, m: &MutatorAbi, r: ObjRef, bytes: usize, sem: Semantics) {}
    #[cold] fn write_slow(&self, m: &MutatorAbi, holder: ObjRef, slot: Slot, v: TaggedValue);
    fn collect<B: Binding>(&mut self, why: GcReason, kind: GcKind, cx: &RootCx) -> GcStats; // safepoint only
    fn is_live(&self, r: ObjRef) -> bool;
    fn forwarded(&self, r: ObjRef) -> Option<ObjRef>;
    fn pin(&self, r: ObjRef) -> bool;
}
pub enum Semantics { Default, NonMoving, Immortal, Los, Code }  // = MMTk AllocationSemantics
```

### 11.2 Static selection

```rust
#[cfg(not(any(feature = "gc-null", feature = "gc-mmtk")))] pub type ActivePlan = immix::ImmixPlan;
#[cfg(feature = "gc-null")]                                pub type ActivePlan = null::NullPlan;
#[cfg(feature = "gc-mmtk")]                                pub type ActivePlan = mmtk_adapter::MmtkPlan;
pub const ATTRS: GcAttrs = <ActivePlan as Plan>::ATTRS;   // compile_error! if two features are set
```

- Fast paths are monomorphic: no `dyn` anywhere on allocation, barrier or poll.
- **Runtime modes** of `ImmixPlan` (off, default, stress, zeal-minor, zeal-move, verify) and per-heap
  `HeapPolicy` are branches in *slow paths only*. One binary therefore runs every differential lane, as
  `PATINA_GC` does today.

### 11.3 The JIT ABI

`MutatorAbi` is `#[repr(C)]`, it is the first part of `Mutator`, and its address is the JIT's context register.

| Offset | Field | Used by |
|---|---|---|
| 0, 8 | `cursor`, `limit` (MMTk `BumpPointer` layout) | inline allocation |
| 16 | `ticks: i32` | poll |
| 20 | `event: u32` | poll cold path; barrier slow path |
| 24 | `log_bias: *const u8` (per heap) | barrier |
| 32, 40 | `remset_cur`, `remset_soft` | barrier slow path |
| 48, 56 | `reg_top`, `reg_limit` (current green thread) | calls |
| 64 | `thread: *mut GreenThread` | helpers |
| 72 | `barrier_mode: u8` (0 = generational; reserved: 1 = incremental update, 2 = SATB) | slow paths only |

```rust
pub struct GcAttrs {
    pub alloc: InlineAlloc,          // BumpPointer{cursor_off:0, limit_off:8, max_inline:128, align:16} | None
    pub pair_ref_offset: u8,         // 8
    pub barrier: BarrierKind,        // None | FieldLog | ObjectLog (address tested = holder ref)
    pub log_table: LogTable,         // GranuleByte{bias_off:24, shift:4, bit0:6} | Dense{base: Base, shift:6}
    pub value_filter_bit: u8,        // 2
    pub log_slow: LogSlow,           // InlineSsb{cur_off:32, soft_off:40} | Call(extern "C" fn)
    pub poll: Poll,                  // Ticks{ticks_off:16, event_off:20}
    pub can_move: bool, pub can_pin: bool,
}
pub enum Base { MutatorField(u16), Absolute(u64) }  // in-tree: per-heap field; MMTk: process-wide constant
```

The JIT's `emit_alloc`, `emit_write_barrier` and `emit_poll` each switch on `ATTRS` **at JIT-compile time**, and
are shared with the interpreter's `#[inline(always)]` Rust versions so the two never diverge (JEP 475's
late-expansion lesson).

### 11.4 Collectors shipped

1. **`ImmixPlan`**, the single production collector, with modes FullHeap / Sticky / Sticky+Evac and the test modes
   of §14.
2. **`NullPlan`**: never collects, bump only, Epsilon style. It serves allocation-cost baselines and
   "GC-correctness-independent" differential runs. `PATINA_GC=off` maps to ImmixPlan's off mode for lanes;
   `NullPlan` exists for measurement.
3. **`MmtkPlan`**, feature `gc-mmtk`, off by default; the Stage 5 spike and possibly more (§11.5).

Today's mark-sweep is **not** kept as an oracle: it cannot run on the new representation. The differential oracle
is ImmixPlan's own `off` mode plus `verify` (§14), the same discipline as today's off/default/stress lanes.

**Future collectors plug in by implementing `Plan`:**
- **Incremental:** `barrier_mode` switches slow paths to also push old values (SATB, value filter dropped through
  a JIT recompile) or to re-grey (incremental update). Continuations darken on reinstatement.
- **Parallel marking:** granule-byte marks become `fetch_or`, the worklist becomes Chase-Lev deques, and
  `RootVisitor` batches become work packets.
- **LXR/MMTk variants:** through the adapter, with `barrier = FieldLog`, `log_table = Dense`.

**Explicitly excluded:**
- load/read barriers (`BarrierKind` has no load variant; ZGC/Shenandoah-style concurrency pays ~5.4% read cost
  for a single mutator [DIGEST pitfall 11]);
- conservative scanning of VM or JIT frames (forbidden by #423);
- collection inside allocation;
- `dyn Plan` on fast paths;
- more than one production collector [DIGEST pitfall 21].

### 11.5 MMTk as a plan: the adapter, the spike, and what must change upstream

**Adapter (`crates/patina-gc-mmtk`, about 2–4 k lines [I]).** `impl mmtk::vm::VMBinding for PatinaVM` forwards to
`Binding`:

| MMTk piece | How the adapter fills it |
|---|---|
| `ObjectModel` | `UNIFIED_OBJECT_REFERENCE_ADDRESS = false`; `OBJECT_REF_OFFSET_LOWER_BOUND = 0`; forwarding pointer in-header at offset 0; forwarding bits, mark bit, pin bit and the global log bit all on side metadata; `get_current_size`, `copy`, `ref_to_object_start` from `Binding` |
| `Slot` | the tagged-word slot (MMTk explicitly supports tagged slots [immix-mmtk §8]) |
| `Scanning` | `scan_object` → `Binding::scan_object` plus the `EPH_KEY` side-bit check; roots through `RootsWorkFactory` (slots, pinning, tpinning); `process_weak_refs` → `Binding::weak_step` |
| `Collection` | one mutator per interpreter; `stop_all_mutators` waits on a condvar until every bound mutator is `AtSafepoint`/`InSafeRegion`; `spawn_gc_thread` runs `std::thread` workers; trigger `Delegated` |
| JIT ABI | Patina keeps a mirrored `BumpPointer` at offset 0 and syncs it around `alloc_slow` (MMTk perf guide option 2). `ATTRS = { barrier: ObjectLog, log_table: Dense{base: Absolute(MMTk side-metadata base), shift: 6}, log_slow: Call(mmtk_post_write_slow) }` |

**Spike tests, in order (each a hard gate on the next):**
1. cdr-anchored pairs survive MMTk's debug assertions (`extreme_assertions`, `sanity` features) on Immix and
   StickyImmix;
2. `at_safepoint: false` plus overcommit plus the delegated trigger never blocks inside allocation;
3. single-worker determinism: 20 consecutive runs of the GC differential lane byte-identical;
4. `scan_object` is called for pointer-free objects (key-indexed ephemerons);
5. per-GC handoff latency on macOS arm64 and Linux x86-64;
6. the full chibi suites, control-flow matrix and ephemeron tests on both platforms (the `patina` binary is one
   interpreter per process; integration-test binaries run under `cargo nextest`, process per test);
7. the benchmark set (§14), interleaved against ImmixPlan's best mode;
8. build-time and binary-size delta.

**What would have to change upstream for MMTk to win outright:**
1. **Multiple instances per process (#100) with dynamic heap ranges (#1347):** instance-scoped
   `VM_MAP`/`MMAPPER`/`SFT_MAP`, or one address-space manager handing disjoint chunk sets to instances. Side
   metadata bases would be per instance and exported, so JITs read them from a context field rather than an
   immediate.
2. **An in-thread collection mode:** a `gc_poll` variant that runs the work packets on the calling mutator thread
   when there is one worker, with no `spawn_gc_thread` and no handoff.
3. **A binding-owned barrier slow path:** an API to append pre-logged objects or slots to the plan's modbuf
   (`flush_external_modbuf(&[ObjectReference])`), so JIT slow paths can be inline and call-free. Also a plan
   constraint override so StickyImmix can run `FieldBarrier`.
4. **Documented determinism** for single-worker scheduling.
5. **aarch64-apple-darwin at tier 1**, with CI runs and not only builds.
6. **A guaranteed "object traced" hook**, or native ephemeron support, so key-indexed ephemerons do not depend on
   undocumented `scan_object` behaviour.
7. **semver releases carrying LXR**, so bindings do not pin git revisions; optional heavy dependencies (`sysinfo`,
   `regex`) behind features.

Items 1 and 2 are decisive. Without them, MMTk forces either a shared-heap, OS-threaded protocol across
interpreters, or "one interpreter per process" as a product limitation.

---

## 12. Threading readiness

**The split** [threads-rec §1.5]:
- `Mutator` is the carrier: allocation state, SSB, `ticks`/`event`, the safepoint state (`Running` /
  `AtSafepoint` / `InSafeRegion`), the current green thread, statistics.
- `GreenThread` holds the register stack and frames, prompt/wind/handler lists, the dynamic environment
  (parameterization and current ports, as heap data), escape fields, the watermark and a `ran_since_gc` bit.
- Under M:1 there is exactly one `Mutator` and many `GreenThread`s. The JIT context register points at the
  `Mutator`.

**N-ready now, at zero single-thread cost:**
- allocation state off `Heap`; a global block pool behind a lock that nobody contends;
- **per-mutator** SSBs (never shared);
- log-bit disarm as a relaxed atomic RMW on a byte, which compiles to `ldrb`/`strb`. A lost race costs a duplicate
  entry, never a missed one, because only the GC arms;
- granule-byte marks, so parallel marking needs no bit-packed shared words;
- a safepoint protocol written as "request all, acknowledge (poll or safe region), collect, release" over a
  mutator list of length 1;
- `enter_safe_region`/`leave_safe_region` around blocking I/O, as no-ops for now;
- deep-bound parameterization and ports;
- one interning function;
- a **two-mutator deterministic test mode** that alternates two `Mutator`s on one OS thread in CI.

**Deferred:**
- OS-thread carriers;
- publication fences (feature `threaded`: in the funnel and after the JIT's allocation initialization);
- parallel marking;
- handshakes.

If owner decision 7 becomes "yes", MMTk's parallel GC and multi-mutator STW change the Stage 5 calculus; §16
records that as an adoption trigger.

---

## 13. Heap sizing, pacing and observability

### 13.1 Triggers (bytes, never object counts)

**Minor budget.** Default 4 MiB of freshly consumed lines (about 150 K objects at 27 B), adapted per minor:
- survival above 30% for two consecutive minors doubles the budget, up to 32 MiB;
- survival below 5% for four consecutive minors halves it, down to 2 MiB;
- survival above 50% at the cap enters **bypass**: majors only, until a major shows survival below 25%. This is
  the queue3/deeprec case [demographics §9.1].

**Major trigger.** `old_bytes ≥ target`:
- the default target is **deterministic**: `target = live_after_major + max(8 MiB, live_after_major)`, i.e. 2× live
  with a floor;
- an optional `adaptive` policy uses MemBalancer, `extra = sqrt(live × g / (c × s))`, with *g* the allocation rate
  and *s* the mark rate. It is time-based and therefore excluded from test lanes (owner decision 15);
- in both policies, `target ≥ live + free_reserve`, with `free_reserve = max(2.5% evacuation reserve, 10% of
  heap)`, which addresses Wingo's livelock.

**External terms.** `external_bytes` counts:
- port buffers (8 KiB per file port);
- host payloads' `external_bytes()`;
- `CodeObject` and JIT code bytes;
- tree-walker payload estimates.

Each term counts toward both budgets. Port-count and `EMFILE` triggers are in §6.7.

### 13.2 Return memory

Decommit as in §4.4. RSS after a library load should track live data (about 35 MiB on the VM [libload §0.5])
plus `free_reserve`.

### 13.3 Observability

- `GcStats` per collection: kind, reason, phase times (roots, remset, trace, weak, epilogue, block summary), bytes
  allocated/promoted/evacuated/freed, live bytes, pinned objects, SSB entries, frames scanned versus total.
- `PATINA_GC_LOG=path` writes JSON lines.
- `(gc-statistics)` returns an alist.
- `scripts/gc_mmu.py` computes MMU at 1/10/100 ms windows from the log, after Larceny's `gc_mmu_log.c`.
- `last_pause_micros` (computed and never read today) is subsumed.

---

## 14. Testing and verification

**Test modes of `ImmixPlan` (runtime, one binary):**
- `off`, `default`;
- `stress`: collect at every poll;
- `zeal-minor`: a minor at every poll;
- **`zeal-move`**: every unpinned object evacuated at every GC. From-space is filled with `POISON`, then
  `mprotect(PROT_NONE)` for one GC cycle in debug builds, so a stale reference faults at its first use;
- **`verify`**, before and after each GC:
  - a reachability walk asserting that every reference targets a granule with a live `MARK` in a block of the
    right space, and is not `FORWARDED`;
  - the **remembered-set invariant**: no armed slot of an old object holds a young reference, checked by a
    full-heap walk at each minor in debug;
  - that every old object's words are armed;
  - pin, space and line-mark consistency.

**Other layers:**
- **Miri** on `patina-gc`: allocator, metadata arithmetic, barrier, the SSB and evacuation copy, with a `Vec`-backed
  `Reservation` under `cfg(miri)`.
- **Differential lanes:** `scripts/run_gc_differential.sh` adds `zeal-move` and `verify` on the VM. The
  tree-walker keeps off/default/stress, since it does not move. `EXPECTED_TOTAL=1226` stays byte-identical across
  modes. Two-mutator mode becomes a lane.
- **Semantic gates every stage:**
  - both chibi suites;
  - `control_flow_matrix.rs` (64 rows);
  - `hygiene_matrix.rs`;
  - `ephemerons.rs`, `escape_from_primitive.rs`, `finished_forms_release_code.rs`, `unclosed_output_ports.rs`;
  - the suite oracles;
  - new E1 (GC flush), E2 (`EMFILE` under `setrlimit`), E4 (port `eq?`), E5 (teardown), each landing with its
    issue.
- **GC benchmark set**, `scripts/run_gc_bench.sh`:
  - the 20 workloads of `PRD/study/gc/probes/workload-demographics/instrumented/REPRODUCE.sh`, run from the external Larceny checkout as
    `run_larceny_tests.sh` does (not vendored);
  - libload (26 libraries) on both backends;
  - deep-capture probes `samedepth1000`, `pingpong1000`, `ctakdeep`;
  - `make-vector 100000` churn;
  - 100 k port opens.
- **Metrics:** wall, GC CPU, max/p95/mean pause, MMU at 1/10/100 ms, peak RSS, collections. Runs are
  **interleaved** main/branch/main, five rounds, reporting the median and the min–max spread.

**How each stage proves itself:** every stage in §15 lists its gates. The rule throughout: no stage lands on a
benchmark claim without an interleaved A/B, and no stage lands with any lane non-identical.

---

## 15. Migration plan

**Common-core mapping.** C1 → Stage 1; C2, C5, C13 → Stages 3 and 6; C3, C4, C6 → Stage 3; C7, C8, C10 → Stage 2;
C9 → Stages 0 and 3; C11 → Stage 1; C12 → Stages 1 and 7; C14 → every stage.

**Issues first (project rule).** Before Stage 0 code, file issues for the present-day defects of DIGEST §1.11:
embedder use-after-free, interpreter heap leak, port `eq?`, `EMFILE`, the unrecorded rebinding divergence, and doc
drift (AGENTS.md "NaN-boxed", "24 transfer shapes"). Each stage's PR closes its issues.

### Stage 0: instruments and quick wins (2–3 weeks)

**First three PRs:**

1. **PR1 "GC benchmark set, per-collection log, interleaved runner."**
   - Files:
     - `scripts/run_gc_bench.sh` (new; references the Larceny checkout);
     - `crates/patina-tests/bench_programs/gc/` (new in-repo probes: deep capture, vector churn, port churn,
       library-load driver);
     - `crates/patina-core/src/heap/gc.rs` (phase timings in `GcStats`, `PATINA_GC_LOG`);
     - `scripts/gc_mmu.py`.
   - Acceptance: no behaviour change; all lanes identical; baseline numbers posted to the tracking issue (no new
     markdown file without approval).
   - Value: the first pause/MMU baseline, which every later claim needs.
2. **PR2 "Delete the unread provenance stores."**
   - Removes the `SourceMap.locations` writes, the `child_source` store and the throwaway per-expansion
     `SourceMap`. None has a production reader [V, libload §5.1].
   - Files: `patina-frontend/src/parser/{mod,datum}.rs`, `desugarer/mod.rs`, `patina-core/src/heap/source.rs`,
     `patina-core/src/source_map.rs`, and the tests that read them.
   - Acceptance: libload peak −26 MiB and churn −355 MiB (measured, interleaved); `interpreter_api.rs:270` error
     locations unchanged; lanes identical.
   - Value: memory now, and two fewer raw-bits maps standing in the way of moving.
3. **PR3 "Mutator context and event word."**
   - New `patina-core/src/mutator.rs` with `#[repr(C)] MutatorAbi`. `allocs_since_gc`, `gc_threshold`,
     `gc_pending: Rc<Cell<bool>>` and `gc_defer_depth` move off `Heap` (`heap/mod.rs:379-417`).
   - Safepoints read `event`/`ticks` (VM `vm_state.rs:1195-1215,1287-1296`; tree-walker
     `cps_eval/mod.rs:213-230`). The interpreter poll moves to `Call`/`TailCall`/`Return` dispatch.
   - Acceptance: interleaved A/B within ±1% wall on the set (expect a small gain from fewer polls); lanes
     identical; `stress` still collects at every poll.
   - Value: the JIT ABI object exists; threads-rec item 1.

### Stage 1: context, funnel and handles (C1, C11, C10 embedding part; 6–9 weeks)

**Scope.**
- `Cx`/`&Mutator` receivers replace `Rc<RefCell<Heap>>` at every call site. A shim keeps arenas underneath. This
  is the ~1,300-site codemod, about 95% mechanical [prim-embed §10].
- The store funnel with a no-op barrier (`BarrierKind::None`).
- `vector_slice_mut` deleted.
- Records, parameters and promises become heap objects.
- `Global` and `RootScope` handles; `eval_*` returns handles.
- `Environment.heap` and `CompiledMacro.heap` removed; teardown on drop.

**Files.** `patina-primitives` (39 files), `patina-frontend`, `patina-macros`, `patina-runtime`,
`patina-interpreter`, and core `heap/mod.rs`, `environment.rs`, `compiled_macro.rs`.

**Acceptance.**
- The embedder probe [prim-embed §7.2] passes in debug and release.
- The teardown probe: `Weak` upgrade fails after drop.
- Interleaved A/B: wall ≥ −1% (expect a gain from removing `RefCell` borrows).
- All lanes identical.

**Value.** It fixes two defects (use-after-free, leak) and removes the `RefCell` tax.

### Stage 2: bindings, rooted boundaries, continuations, code (C7, C8, C10; 7–10 weeks)

**Scope.**
- Global cells variant **R**: cell space; `FORWARDED`/`Owner`/`links` deleted; `Library.exports` → `CellRef`
  [global-cells §8.1].
- `RootScope` at loading points A, B, D and E, so library bodies collect.
- Continuations, design A: weak tables deleted.
- Code descriptors marked by tracing: `live_closures` deleted.
- Frames become `Copy` with a traced `closure`.
- Non-relocating register stack.
- The control-flow matrix gets new rows for capture at depth.

**Acceptance.**
- libload peak RSS ≤ 300 MiB on the VM, against 627 MiB today [I target, from libload §0.6].
- ctak under 500 frames ≤ 0.5 GB, against 4.0 GB today.
- The 64-row matrix on both backends; `finished_forms_release_code.rs`.
- Interleaved A/B on fib/tak: global access ≥ +3% [I: `frame_globals` was 2.6% of samples].

### Stage 3: representation and the non-moving Immix heap (C3, C4, C5, C6, C13; 12–18 weeks)

**Mechanism.** Kind by kind, in a mixed world:
- the block heap lives at VA ≥ 2⁴⁰, so a heap-tagged word whose bits 35–63 are zero is still an arena index;
- `visit` dispatches arena index → bitset, address → granule byte;
- arenas are swept as today, blocks lazily;
- **non-moving throughout.**

**Sub-steps (each a PR series with lanes green):**

| Step | Content |
|---|---|
| 3a | Pairs (27.9% of allocations) and block, line and granule metadata |
| 3b | Cells, closures (headerless, code descriptors) and the flonum box |
| 3c | Vectors, strings, bytevectors, heap-native bignums |
| 3d | Records/RTDs (canonical), symbols (non-moving), identifiers (ids, inline `src`), ports (`PortTable`, finalization registry), host payloads (tree-walker ids), ephemerons (key-indexed). The `HeapObjectData` enum is deleted |
| 3e | Tag renumbering to §2.2 and self-tagged floats |

**Acceptance.**
- Lanes identical after every PR.
- At 3d: no `Drop` impl reachable from any heap object (a `static_assertions`-style test over `TYPE_INFO`).
- Benchmark set:
  - wall geomean ≥ +10% [I; 2.05× fewer bytes and no high-water sweep];
  - GC CPU ≤ 50% of the baseline;
  - gcold RSS ≤ 100 MB (628 MB today);
  - the post-load major pause ≤ 30 ms (178 ms today).
- The float subset (fibfp, mbrot, nucleic) ≥ +30% at 3e [I; the paper reports 2.3×].

**Value.** The largest throughput and memory step, with no generations yet.

### Stage 4: Sticky generations and barrier, measured (6–9 weeks)

**Scope.**
- `FieldLog` barrier through the funnel and the interpreter, SSB, arming.
- Sticky minors, stack watermark, adaptive minor budget with bypass.
- Port and host-payload young lists.

**M2:** barrier on, minors off. Interpreter tax target ≤ 1%.

**M5:** three-way comparison:
1. Sticky;
2. barrier on, minors off;
3. FullHeap.

**Acceptance.** Sticky ships as the default only if it does not lose wall geomean by more than 1% **and** cuts
p95 pause by at least 30% on the set (§16 K2). Otherwise FullHeap is the default and Sticky stays a runtime mode.

### Stage 5: the MMTk A/B spike (4–6 weeks, time-boxed; needs owner approval for the dependency)

The adapter and spike tests of §11.5, run against Stage 4's ImmixPlan.

**Outcomes:**
- **adopt** (MMTk becomes `ActivePlan`; Stage 6 is replaced by enabling MMTk's moving plans);
- **keep as oracle** (feature lane, maintained only while cheap);
- **drop.**

The decision follows the pre-registered criteria in §16, not judgement after the fact.

### Stage 6: evacuation (8–12 weeks, unless Stage 5 adopts MMTk)

**Scope.**
- Slot-updating root visitors everywhere: the tree-walker heap stays non-moving.
- Pin-on-hash for headerless kinds; appended hash words.
- Young-survivor evacuation, then major defrag with the reserve.
- `zeal-move` and `verify` lanes.
- Immortal boot freeze: one evacuating major after bootstrap copies live boot objects into immortal space, which
  becomes a root region.

**Acceptance.**
- Six consecutive weeks of `zeal-move` lanes clean.
- Fragmentation workloads (gcold, earley, hashtable0): RSS −10% or wall +3% against Stage 4.
- No regression elsewhere above 1%.

### Stage 7: JIT-facing freeze (3–4 weeks)

**Scope.**
- `GcAttrs`/`MutatorAbi` offsets frozen and documented in `docs/GC_DESIGN.md`.
- Shared emitters with Rust-side inline twins.
- Poll ticks with deterministic preemption.
- Helper classification (leaf versus may-GC) as an attribute checked by a test.

The JIT itself is a separate track.

### Later and optional

- C′ continuations (owner decision 18);
- SRFI 18 green threads [threads-rec §6];
- SRFI 125 weak tables over ephemerons;
- SRFI 254 guardians;
- incremental mode, only on measured need.

**Total effort [I].** About 48–71 engineer-weeks to Stage 7 for one experienced engineer. Stage 3 dominates, as
every report predicted. Stage 5 adds 4–6 weeks of option value. If MMTk is adopted, it saves most of Stage 6
and all future moving and parallel work.

---

## 16. Risks, mitigations and kill criteria

| # | Risk | Mitigation | Kill or switch criterion (measurable) |
|---|---|---|---|
| K1 | cdr-anchored references break MMTk invariants | Spike test 1 runs first | Any MMTk assertion relying on `ref == start` that cannot be fixed by a small upstream PR → **drop the MMTk route** and record it. The in-tree plan is unaffected |
| K2 | Generations do not pay (Wingo nboyer/splay; queue3/deeprec) | Bypass mode; switchable | Sticky loses > 1% wall geomean, **or** cuts p95 pause by < 30%: FullHeap becomes the default and Sticky stays a mode until JIT allocation rates (5–10×) justify re-running M5 |
| K3 | Barrier tax | Elision, static parity, filter | M2 > 1.5% geomean: investigate. > 3%: switch to card + young filter. That needs only scannable spaces, which the never-a-value header marker already makes possible |
| K4 | Hidden references under moving (Julia's failure mode) | Inventory (§8.5), `zeal-move` from-space faults, pin escape hatch | After 6 weeks of Stage 6 the `zeal-move` bug rate is above 1 per 2 weeks, **or** evacuation gains < 3% wall and < 10% RSS on the fragmentation workloads: ship non-moving Sticky (Ruby and Julia default to this) with free reserve plus boot freeze only |
| K5 | Self-tagged floats slow non-float code | Fixnum fast path unchanged; tests are 1–3 instructions | Non-float benchmarks regress > 1% at 3e: box flonums (16 B) and keep float tags reserved |
| K6 | Stage 3 overrun (mixed world complexity) | Kind-by-kind, lanes per PR | At the midpoint (end of 3b) elapsed time > 1.5× estimate: re-plan before 3c |
| K7 | Major pauses too long for "bounded pauses" | Watermark, lazy sweep | p99 major > 50 ms at ≤ 50 MiB live after Stage 4: parallel marking becomes the next item, implemented in-tree **or via MMTk**, re-entering the Stage 5 decision |
| K8 | VA reservations fail on CI (Linux overcommit, `vm.max_map_count`) | `MAP_NORESERVE` | Any lane failure from reservation: default reserve becomes 4 GiB with chained reservations |
| K9 | Tree-walker drift | Same allocator; non-moving policy; its lanes | Tree-walker chibi or matrix red at any stage blocks that stage |
| K10 | MMTk spike becomes a sink | Time box, pre-registered criteria | Over 6 weeks with tests 1–4 not passed: stop |

**MMTk adoption (pre-registered).** MMTk becomes `ActivePlan` iff **all** of these hold:
- (a) multi-instance support (upstream item 1) is usable in a release, **or** the owner accepts "one MMTk heap per
  process, interpreters as mutators" for embedding and for `cargo test`;
- (b) the best MMTk plan beats ImmixPlan's best mode by ≥ 10% GC CPU geomean **and** ≥ 3% wall geomean on the set,
  with p95 pause no worse;
- (c) zero failures in 20 runs of every lane on macOS arm64 and Linux x86-64;
- (d) the differential lanes are byte-identical with `single_worker`;
- (e) either upstream item 3 exists, or a tier-2 microbenchmark shows the barrier slow-path call costs ≤ 1%;
- (f) the dependency is approved and pinned to a release, not a git revision.

**Adoption triggers.** Owner decision 7 turning "yes" (parallel mutators), or K7 firing, re-runs the gate even
after a "keep as oracle" outcome.

---

## 17. Owner decisions (DIGEST §6)

| # | Decision | Proposed default | Consequence of the alternative |
|---|---|---|---|
| 1 | Tree-walker role | **Answered: keep, may lag.** Non-moving, non-generational tree-walker heap; pinning roots; host-payload ids (§8.4) | Full parity (heap frames) costs 6–12 weeks [tree-walker §4c] |
| 2 | Rebinding semantics | **R** in Stage 2 (no change). Propose **C** after Stage 3 with a `DIVERGENCES.tsv` update; it matches chibi, Chez and Racket | Staying on R costs the JIT one dependent load per global, or code invalidation |
| 3 | `define` rebinding time and mid-program imports | Follow C's compile-time rules when C lands; unchanged under R | Under C six pinned tests flip, matching the oracles |
| 4 | Encoding | 61-bit fixnums; **self-tagged floats** (§2.2); raw addresses; closure tag | Boxed flonums frees two tags (exact one-bit heap test) but keeps 19.8% of allocations |
| 5 | Strings, bignums | UTF-32 inline (O(1) `string-set!`); heap-native limbs, with `num-bigint` at operation boundaries first | UTF-8 saves 4× string bytes (strings are ~0 of allocations) but rewrites `strings.rs` |
| 6 | Pauses | **Answered: throughput first, STW.** Targets: minor p99 ≤ 5 ms, major ≤ 50 ms at ≤ 50 MiB live | A hard latency target brings incremental marking: `barrier_mode` 2, SATB, +~10 barrier instructions |
| 7 | Shared-memory parallelism | **No for now:** M:1 with N-ready interfaces (§12) | "Yes" makes M:N the destination and makes MMTk's parallel GC an adoption trigger (§16) |
| 8 | JIT frame model and tiers | S1 baseline (fragments + `tail`), no S3 in GC scope; contract keeps S3 possible | S2 needs unwinding and a native depth cap; S3 needs deopt metadata at capture points |
| 9 | Async interrupts in JIT | Yes, through the tick poll; timer only outside test lanes | Without them, non-calling loops could skip polls (none exist today) |
| 10 | Identity hashing | Address-based BFG relative to the heap base; pin-on-hash for pairs and closures (§6.8) | Stored serial hashes would make hashes mode-independent at the cost of header bits or a side table (+1–3% on eqtable) |
| 11 | Weakness scope | SRFI 124/254 ephemerons now; SRFI 125 weak tables after Stage 4; guardians after Stage 6 | Adding guardians earlier only adds a fixpoint step |
| 12 | Port finalization | Flush and close at GC, `EMFILE` retry; excluded from the differential lane | Not flushing diverges from chibi and Gauche (E1) |
| 13 | Embedding API | Scoped closures plus owned `Global` handles; mandatory teardown; **multiple heaps per process** | Accepting one heap per process removes MMTk's main obstacle (§11.5) but limits embedders |
| 14 | Dependencies | No new dependencies in the default build; approve the feature-gated MMTk spike at Stage 5 (crates download, `cargo nextest` for process-per-test) | Refusing the spike leaves the MMTk question unmeasured; the in-tree plan proceeds unchanged |
| 15 | Footprint | 32 GiB VA per heap; deterministic 2× live target; return memory above `max(4 MiB, 25% live)`; adaptive MemBalancer opt-in | Adaptive by default trades lane determinism of GC timing for memory |
| 16 | Front-end sequencing | Inline provenance and identifier ids **with Stage 3d** (required for Drop-free identifiers). Lazy scope propagation as a parallel front-end project before Stage 4 measurements | Later lazy scopes: Stage 4 is measured against an allocation profile that will change (44% of load CPU) |
| 17 | syntax-case timing | After Stage 2 (expander root provider, literal pool); transformer entry = minor GC over a non-moving mature space | Earlier: whole-form deferral reproduces today's load pathology |
| 18 | Continuation end state | Design A (Stage 2); C′ only if post-JIT capture-at-depth benchmarks demand it | C′ now: medium-high risk in the matrix's historical defect area |
| 19 | Recording divergences | Record port `eq?` (convergence), rebinding under C, and any delimited-continuation corner cases in `DIVERGENCES.tsv` with each stage | — |
| 20 | JIT code memory | Freed through the code finalization registry at majors; `MAP_JIT` toggling confined to one install function; verify `cranelift-jit` per-function freeing, else epoch-based code arenas | — |
| 21 | Budget order | **Representation first** (Stages 1–3) before any generational work; MMTk decided at Stage 5 | Collector first means building on `Vec` arenas that every option discards |
