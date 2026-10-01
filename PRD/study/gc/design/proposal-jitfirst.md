# Patina GC redesign — the JIT-first proposal

Panel position: **JIT-first.** I start from the machine code that a Cranelift baseline tier, and later an
optimizing tier, should emit for Patina's measured hot paths. From that code I derive the value encoding,
the object model, the frame model and the barrier, and only then the collector. Repo `main` at `28a94f8`,
read-only. Date 2026-10-01.

Conventions: **[V]** = verified in source, by measurement or in a primary source (by me or by the cited
report); **[I]** = inference or design judgement. Citations use the DIGEST keys (`[demographics §5.2]`,
`[barrier-res §7]`, `[cranelift-gc §2.1]`, …). Repo paths are crate-relative as in the DIGEST
(`core:` = `crates/patina-core/src/`, `vm:` = `crates/patina-vm/src/`, etc.). aarch64 is the reference ISA
because it is the development platform; x86-64 counts are given where they differ.

---

## 1. Thesis and key bets

**Thesis.** The fastest code a Cranelift tier can emit for Patina's opcode mix needs one load (tag folded
into the displacement) for `car`, `cdr`, `vector-ref`, closure slots, cells and globals; a call made of a tag
check, one load and an indirect jump; allocation as bump, compare and branch; a 4-instruction store barrier
(6 when the slot parity is dynamic); polls that are free at calls and 2 instructions at back-edges; and
flonums that are not heap-allocated. Each of these is a property of the *representation and the ABI*, not of
the collection algorithm. So I freeze a collector-neutral ABI first — low-tag raw addresses with a single
"is heap" bit, self-tagged flonums, GC state in a per-granule side byte, closures whose word 0 is a machine
entry address, VM frames as the only home of Scheme values at every non-leaf safepoint in every tier, a bump
pointer and limit, one field-log bit per word, and one event word folded into the register-stack limit — and
let the collector behind it evolve without touching the JIT: first a non-moving block-structured mark-region
heap (Candidate A without generations), then a copying nursery with adaptive in-place promotion over an
Immix-style mark-region mature space (Candidate C), then opportunistic mature defragmentation (B).

**Key bets.** If any of these is wrong, the design needs rework.
1. **No Cranelift stack maps in any tier.** All Scheme values are written to the frame's VM register window
   before every call that can reach a safepoint, and reloaded after it. This is no worse than Cranelift's own
   mechanism, which spills each declared value at its definition and reloads it at *every* use
   [cranelift-gc §2.1, barrier-res §6]. It keeps `call/cc` a VM operation, because multi-shot capture of
   Cranelift native frames is unsupported [DIGEST fact 6].
2. **Allocation, barrier slow paths and leaf helpers never collect.** This keeps ~1,300 Rust sites
   handle-free [prim-embed §0], and makes JIT allocation and barrier slow paths leaf calls that need no
   publish and no reload.
3. **Self-tagged flonums pay off.** The paper reports 89% of mandelbrot floats immediate and 2.3× on float
   benchmarks [heap-repr §11]. Boxed flonums are 19.8% of all allocations [demographics §4.1]. Encoding them
   costs a few ALU operations, and it must cost ≤1% on non-float code.
4. **A copying nursery beats whole-heap marking for a JIT-speed mutator** once representation, bump
   allocation and lazy sweep are in place. This is gated by the M5 experiment and switchable per heap.
5. **The representation rewrite can be a strangler** that moves one object kind at a time, with every
   differential lane green after each step.
6. **Position-independent, interleaved VM frames with trampolined returns (S1) cost at most ~8% against native
   call/ret (S2).** The GC contract is the same under both, so this bet only decides the JIT's call
   sequence.

**Deviations from my starting brief, stated explicitly.**
- The brief imagines "native frames" as a candidate. I reject them for *all* tiers (bet 1); the optimizing
  tier gets raw (unboxed) window slots instead of native stack maps.
- The brief's "allocation in 3–4 instructions" holds only per allocation *within a group*. Cranelift gives
  one pinned register, which holds the `Mutator`, so `ap`/`limit` live in memory and are cached in SSA
  values between safepoints.
- I keep "allocation never collects" even though Wasmtime's copying allocator collects in its slow path
  [cranelift-gc §2.3]. Wasmtime's host code uses rooted handles; Patina's does not, and should not have to.

---

## 2. Value encoding

### 2.1 Layout

`TaggedValue(u64)`, `#[repr(transparent)]`, `Copy`. The low 3 bits are the primary tag. **Bit 2 set ⇔ heap
reference.**

| Tag | Meaning | Payload / address arithmetic |
|---|---|---|
| `000` | fixnum | 61-bit signed, `n << 3` |
| `001` | immediate | low byte = kind; see §2.3 |
| `010` | flonum, self-tagged, range A | see §2.2 |
| `011` | flonum, self-tagged, range B | see §2.2 |
| `100` | pair | 16 B-aligned address + 4: car at `[v−4]`, cdr at `[v+4]` |
| `101` | procedure (closure) | address + 5: entry word at `[v−5]`, free variable *i* at `[v+3+8i]` |
| `110` | headered object | address + 6: header at `[v−6]`, word *k* at `[v+2+8k]` |
| `111` | vector | address + 7: header at `[v−7]`, element *i* at `[v+1+8i]` |

**Why this assignment** [I, grounded where cited]:
- **Fixnum `000`.** Tagged add and subtract work directly (`adds`/`b.vs`), and a tagged index is a byte
  offset for 8-byte elements, so `vector-ref` needs no shift [jit-ready R1].
- **One heap bit.** The barrier's value filter becomes `tbz xv, #2`, a single instruction. barrier-res §4.1
  recommends exactly this, and it removes 64% of heap stores before the metadata load
  [demographics §6.2]. GC root and slot visiting test one bit.
- **Two float tags.** These cover the two top-3-exponent-bit patterns around 1.0 (§2.2).
- **Pair, procedure and vector get primary tags** because the JIT inlines their predicates and accessors on
  the measured hot path [jit-ready §2.3]:
  - Car and Cdr are 14.6% of nboyer dispatches.
  - Calls are 10–31% of dispatches.
  - `VectorSet` makes 37 M of the 113 M heap stores.
  - **Every** procedure is tag `101`: compiled closures, primitives, continuations and parameter objects
    alike. `procedure?` is one tag test, and a call has one shape (§3.3).
- **Strings, symbols, records and everything else** sit under the object tag, with a header.
  - Strings are ~0% of allocations in the 20-workload GC set [demographics §4.1].
  - Records keep a JIT-fast type test through their RTD-pointer header (§3.2).
- **Raw addresses, not offsets from a per-heap base.**
  - `car` is `ldur x, [v, #-4]`: one load. An offset form needs `add base` plus a load, and a second
    register.
  - Offsets buy compression (V8, Wasmtime), but Patina keeps 61-bit fixnums in heap slots, so slots stay
    8 B and compression gains nothing [heap-repr §13, js-engines].
  - 318 heaps per process are fine: each heap has its own reservation [demographics §8.2].
- **16 B alignment** gives every object room for a forwarding marker plus an address, as in Chez
  [chez §1]. Addresses have bit 3 clear, but I do not spend it on tags; it stays free for the future.
- **Fixnum width stays 61 bits** (`fx-width` unchanged). No semantics move.

### 2.2 Flonum self-tagging (exact)

Let `raw = f64::to_bits(d)`. Define `K = 1 << 60`.

```
encode(d) = rotate_left(raw.wrapping_sub(K), 4)     // valid iff (w & 7) ∈ {0b010, 0b011}
decode(w) = rotate_right(w, 4).wrapping_add(K)
```

**Why it works.**
- After the rotation, bits 2..0 of `w` are the top three exponent bits minus 1, and bit 3 is the sign.
- Biased exponents `0x300..=0x4FF` (top bits `011` and `100`) map to tags `010` and `011`.
- So every double with |d| ∈ [2⁻²⁵⁵, 2²⁵⁷), of either sign, is immediate.

**What is not immediate.**
- These are boxed as 16 B `FLONUM` objects (§3.2): ±0.0, subnormals, |d| < 2⁻²⁵⁵, |d| ≥ 2²⁵⁷, ±inf and NaN.
- **±0.0, ±inf and the canonical NaN** use per-heap immortal boxes that are allocated once. The encode slow
  path returns them without allocating.
- The JIT constant-folds literal zero.
- Other NaN payloads get a fresh box, so NaN bits are preserved.

**Properties.**
- The representation is canonical: a given double has exactly one encoding.
- So `eqv?` is a bit compare for immediates and a payload compare for boxes.
- `eq?` on flonums becomes `#t` for equal in-range values. R7RS leaves this unspecified.
- This is my own variant of Melançon/Serrano/Feeley (OOPSLA'25) [heap-repr §11], chosen so that the float
  tags avoid the heap bit. **Its immediate share must be measured** (kill criterion K1).

**aarch64 sequences** (`xK` holds `K`, materialized once per fragment):

```
; is-flonum-immediate(a):            and  t, a, #6 ; cmp t, #2 ; b.ne slow           (3)
; decode a → d0:                     ror  x0, a, #4 ; add x0, x0, xK ; fmov d0, x0     (3)
; encode d2 → w (with range check):  fmov x2, d2 ; sub x2, x2, xK ; ror x2, x2, #60
;                                    and  t, x2, #6 ; cmp t, #2 ; b.ne box_slow        (6)
```

- `(fl+ a b)` in tier 1 is about 18 instructions, against a 72 B heap allocation today
  [demographics §1.9: 8.2 ns per flonum allocation].
- Tier 2 keeps doubles unboxed in registers between safepoints, and stores them across calls into raw
  window slots (§8.2).

### 2.3 Immediates and special constants (tag `001`)

The low byte is `kind << 3 | 001`, and the payload sits in bits 63..8. Today's singleton values are kept
bit-for-bit (`core:tagged_value.rs:91-110` [V]).

| Low byte | Value | Notes |
|---|---|---|
| `0x01` | `#f` | `if` = `cmp x, #1 ; b.eq else` |
| `0x09` | `#t` | |
| `0x11` | `()` | |
| `0x19` | eof | |
| `0x21` | unspecified | |
| `0x29` | default-object | spare, reserved |
| `0x31` | **BWP** | broken weak/ephemeron marker, distinct from `#f` [finalization §0.8] |
| `0x39` | **UNBOUND** | unbound global cell; never escapes to Scheme. Replaces `FORWARDED` once cells land (Stage 5) |
| `0x41` | char | `(codepoint << 8) \| 0x41`. Moves out of today's `010` tag |
| `0x81` | **object header** | never a value; see §3.1 |
| `0xE9` | **FWD_MARKER** | forwarding sentinel: a forwarded object's word 0 = `FWD_MARKER`, word 1 = new tagged reference |
| `0xF9` | GC_POISON | debug and torture from-space and dead-slot poison (exists today) |

**The forwarding marker is unambiguous** for every object kind:
- a pair's word 0 is a Scheme value, which never equals `FWD_MARKER`;
- a closure's word 0 is an 8-aligned machine address, whose low 3 bits are `000`;
- an object's word 0 is a `0x81` header or an RTD reference (tag `110`);
- a vector's word 0 is a header.

### 2.4 What the JIT gets from the encoding (aarch64, tag checks included)

| Operation | Sequence | Instrs |
|---|---|---|
| `(car p)` | `and t,p,#7; cmp t,#4; b.ne slow; ldur r,[p,#-4]` | 4 |
| `(null? x)` / `if` | `cmp x,#0x11` / `cmp x,#1` + branch | 2 |
| `(+ a b)` fixnum | `orr t,a,b; tst t,#7; b.ne slow; adds r,a,b; b.vs ovf` | 5 |
| `(- a 1)` | `tst a,#7; b.ne slow; subs r,a,#8; b.vs ovf` | 4 |
| `(vector-ref v i)` | tag (3); `ldur h,[v,#-7]`; `tst i,#7; b.ne`; `cmp i,h,lsr #13; b.hs oob`; `add a,v,i; ldur r,[a,#1]` | 10 |
| ReadCell / LoadClosure *i* | `ldur r,[c,#2]` / `ldur r,[clo,#3+8i]` | 1 |
| LoadGlobal (cell bound at compile time) | `movz/movk` cell address (ALU only, off the critical path); `ldur r,[cell,#2]` | 1 load |
| `procedure?` / `pair?` / `vector?` | `and; cmp` | 2 |
| `symbol?` | tag (2); `ldurh w,[x,#-6]; cmp w,#(T_SYMBOL<<8 \| 0x81)` | 4 |
| record type test | tag (2); `ldur h,[p,#-6]; cmp h, x_rtd` (RTD is pinned, so its address is an immediate) | 4 |

---

## 3. Object model

### 3.1 Header word

Every object under tag `110` or `111` starts with a header, except records (§3.2). **The header holds no GC
state.** Mark, pin, log and hash state all live in the side metadata byte (§4.3). So the mutator never
read-modify-writes GC-owned header bits [threads-rec §2.12], and headers are immutable after allocation.

```
 63                                    16 15          8 7            0
+----------------------------------------+-------------+--------------+
|        length / aux  (48 bits)         |  type (8)   |  0x81        |
+----------------------------------------+-------------+--------------+
```

- **Low byte `0x81`.** It decodes as an immediate (tag `001`), so a header can never be mistaken for a heap
  reference. That keeps a linear heap walk for the verifier possible.
- **`length`** is in type-specific units: elements, chars, bytes or limbs.
- **Type codes below 32 keep header bits 13–15 zero.** For a vector that lets the bounds check be
  `cmp i, h, lsr #13`: it compares a tagged index against `len << 3`, unsigned, so negative indices fail too.

### 3.2 Layouts

All objects are 16 B-aligned and rounded up to 16 B. "Word *k*" counts from the object start.

| Kind | Tag | Words | Size | Pointer-free | Notes |
|---|---|---|---|---|---|
| pair | `100` | car, cdr | 16 | no | headerless (Chez) |
| closure | `101` | **entry** (raw machine address), fv0…fv(n−1) | 8+8n → 16 | no | headerless. `n` and the code descriptor come from `*(entry−8)` (§3.3). 0-fv closures are one canonical static closure per descriptor |
| vector | `111` | hdr(len, T_VECTOR=1), e0… | 8+8n | no | |
| flonum box | `110` | hdr(T_FLONUM), f64 | 16 | yes | out-of-range only |
| bignum | `110` | hdr(nlimbs, T_BIGNUM; sign in type flags), u64 limbs | 8+8n | yes | inline limbs, **no `num-bigint` payload**. Arithmetic converts on entry and exit, as today's clone-per-dispatch already does [DIGEST §1.1] |
| ratnum / compnum | `110` | hdr, num/re, den/im (TVs) | 24 → 32 | no | compnum parts are usually immediate floats |
| string | `110` | hdr(nchars, T_STRING), UTF-32 code units | 8+4n | yes | O(1) `string-set!` kept. A Latin-1 `T_STRING8` is a later option (owner decision 5) |
| bytevector | `110` | hdr(nbytes, T_BYTEVECTOR), bytes | 8+n | yes | |
| cell / box | `110` | hdr(T_CELL or T_BOX), value | 16 | no | today 72 B [jit-ready §2.3] |
| global cell | `110` | hdr(T_GCELL; flags WATCHED), value, name (symbol), home-namespace id (fixnum) | 32 | no | **pinned** (§4.2) |
| symbol | `110` | hdr(T_SYMBOL), name (string), hash (fixnum), aux | 32 | no | **pinned**; interned through one function |
| record | `110` | **word 0 = RTD reference** (tag `110`), f0… | 8+8n | no | RTDs are **pinned**, so record headers never need updating and the JIT embeds RTD addresses |
| RTD | `110` | base-RTD reference, name, field-count (fixnum), parent, field-names, flags, uid | 64 | no | itself a record of the base RTD; pinned |
| identifier | `110` | hdr(scope-set id in length bits, T_IDENT), symbol, source id (fixnum) | 24 → 32 | no | no `Rc<str>`, no `SmallVec` [libload §6.2]; Stage 6 with the front-end |
| promise | `110` | hdr(T_PROMISE), state, value-or-thunk | 32 | no | stores go through the funnel |
| ephemeron | `110` | hdr(T_EPHEMERON), key, value, GC link | 32 | no | always nursery-born (§6.5) |
| values | `110` | hdr(n, T_VALUES), v0… | 8+8n | no | |
| stack chunk | `110` | hdr(nwords, T_STACK_CHUNK), prev-chunk, frame words… | var | no | frames traced through descriptor maps (§10) |
| continuation | `110` | hdr(T_CONT), chunk, winds, handlers, prompts, meta (fixnum: deliver/flags), re-entry ids | 64 | no | invoked through a procedure wrapper (tag `101`) |
| port | `110` | hdr(T_PORT), port id (fixnum) | 16 | no | payload in the `PortTable` (§6.6) |
| foreign | `110` | hdr(T_FOREIGN, kind in length bits), payload id (fixnum) | 16 | no | payload in the `HostPayloadTable` |
| env specifier | `110` | hdr(T_ENVSPEC), namespace id, mutable flag | 32 | no | |

**Hash extension.** An object that was hashed and then moved carries one extra trailing granule holding its
original hash (§6.9). The layout functions add it when the metadata says `HASHED_MOVED`.

**One declarative layout spec.**
- A `define_layouts!` table in `patina-core` generates per-kind functions: `size(obj)`, `trace(obj, &mut
  impl SlotVisitor)` over **writable** slots, `copy`, `verify`, and the `const` field offsets that the JIT
  and the interpreter use.
- This follows Chez's `mkgc.ss` [racket-larceny §3.2] and deletes the "misfiled leaf variant is a
  use-after-free" hazard (`core:heap/mod.rs:137-141` [V]).

### 3.3 Procedures, code references and entry blocks

The goal: a call is a tag check, one load and an indirect jump.

```
entry block (non-moving; executable when the JIT is on, plain data when it is off):
  [entry−8]  *const CodeDesc         ; written once, at install time
  [entry+0]  b  <target>             ; interpreter trampoline for this desc kind, or the JIT body
```

**The call sequence.**

```
and  x9, xf, #7 ; cmp x9, #5 ; b.ne not_proc_cold       ; tag check            (3)
ldur x10, [xf, #-5]                                     ; entry                (1)
mov  w2, #argc                                          ; argc in a fixed register
blr/br x10                                              ; S2 / S1 (§10.4)      (1)
```

**Tier-up.** Tier-up rewrites the single `b` instruction in the entry block. That is one aligned 4-byte
write inside `install_code`, the only place where W^X is toggled. Existing closures, including long-lived
top-level procedures such as `fib`, then reach JIT code through one predicted direct branch. No other code
is ever patched [threads-rec §3.3].

**Descriptors.**
- A `CodeDesc` is a Rust struct in a per-heap `CodeStore`. Its address is stable and never moves.
- It holds: the bytecode, the per-safepoint liveness maps, the resume table, `nfree`, arity, the `constants`
  slot (a TV pointing to a **pinned** heap vector), nested descriptors, the global cells it embeds, and a
  mark bit.
- The GC reaches it from three places: closure entry words (`*(entry−8)`), frame `code` words, and stack
  chunks.

**Other procedures** are closures whose entry blocks are shared trampolines:
- **primitive:** fv0 = primitive id (fixnum);
- **continuation:** fv0 = continuation object;
- **parameter:** fv0 = parameter record.

The probe order of today's `call_value_with_probe` becomes the entry address itself. A fast path still keys
on the *binding*, never the spelling, because the closure *is* the binding's value.

### 3.4 Rust `Drop` payload policy

- **No GC heap object owns a Rust allocation.**
- Objects that need Rust state get a `u32` id into a per-heap table (`PortTable`, `HostPayloadTable`) and are
  **always allocated old**. That covers ports, tree-walker CPS procedures and continuations, `CompiledMacro`,
  and future FFI objects.
- Payloads are dropped in the major-GC epilogue when their handle object is unmarked.
- The nursery is therefore Drop-free by construction. A "drop list" over 47% of allocations would be a sweep
  in disguise [finalization §0.2]; this policy avoids it.

---

## 4. Heap organisation

### 4.1 Per-heap reservation

- **One reservation per heap.** Default 32 GiB of VA, configurable (`PATINA_HEAP_RESERVE`).
  - It is mapped **read-write with lazy commit and no `mprotect` splitting**, so each heap is one VMA. That
    avoids Linux `vm.max_map_count` exhaustion at 318 heaps. On macOS, 64 × 16 GiB RW lazily committed
    measured RSS-neutral [demographics §8.2].
  - It is placed above 2³⁶ by an mmap hint. During the Stage 3 strangler, `v ≥ 2³⁶` distinguishes block
    references from legacy arena indices, which are below 2³⁵.
- **Layout inside the reservation:** `[metadata: 1/16 of the block area][block area][LOS area]`.
- **Outside the heap reservation:**
  - per-mutator remembered-set buffers: 256 MiB reserved each, lazily committed;
  - per-green-thread register stacks (§10.1);
  - the code reservation for entry blocks and JIT code (`MAP_JIT` on macOS).
- **Heaps must be dropped.** Stage 5 removes the `Rc` cycle heap → closure → environment → heap
  [demographics §8.1], and `Drop for Heap` unmaps the reservation. Without that, reservations would
  accumulate exactly as heaps leak today.

### 4.2 Spaces

| Space | Contents | Moves? | Collected by |
|---|---|---|---|
| **Nursery** | every small allocation by default, as per-mutator chunks of 8 free blocks (256 KiB) | yes (copying minor), or promoted in place | minor |
| **Mature** | promoted objects and pretenured objects, in Immix blocks with lines | opportunistic evacuation (Stage 10), never if pinned | major |
| **Pinned** | not a separate space: mature objects with `PINNED` set (symbols, RTDs, global cells, code-constant vectors, canonical boxes, FFI-lent buffers) | never | major |
| **LOS** | objects ≥ 8 KiB, 16 KiB-page-granular runs | never | major (allocated old; *remember-whole* on creation) |
| **Code** | entry blocks plus JIT bodies (executable reservation); `CodeDesc` in Rust | never | marked at majors, freed after them |
| **Host tables** | `PortTable`, `HostPayloadTable`, owned-handle table | n/a | swept in the major epilogue |
| **Immortal boot** | later optimization (Chez static generation): freeze the stdlib image after bootstrap | never | not traced; its slots stay remembered |

**Pointer-freedom** is decided by the header type, not by a space. The tracer skips strings, bytevectors,
bignums and flonum boxes after setting their marks. Field logging needs no heap parsability
[barrier-res §3], so there is no reason to pay for Chez's 19-space matrix [chez §2].

### 4.3 Sizes and metadata

| Parameter | Value | Justification |
|---|---|---|
| granule | 16 B | 56% of objects are exactly 16 B; pairs, cells and flonum boxes carry no extra header [demographics §4.3] |
| line | 128 B (8 granules) | 99.2% of objects are ≤128 B; mean 27 B [demographics §9.3]; Immix default |
| block | 32 KiB = 256 lines = 2 × 16 KiB pages | decommit granularity on macOS arm64; Immix default |
| nursery chunk | 8 blocks = 256 KiB | refill every ~10 K objects |
| nursery target | 4 MiB initially, adaptive 1–64 MiB (§13) | about 155 K objects at a 27 B mean, inside the 64 K–256 K band [demographics §9.1] |
| LOS threshold | 8 KiB | 148 of 348 M objects exceed it [demographics §4.3]; Immix and Whippet use the same threshold |
| medium objects (128 B–8 KiB) | bump into a hole, else into an overflow block | Immix overflow allocation |

**Per-granule metadata byte** (6.25% of committed block area). It is the only home of GC state. It sits at
`meta_bias + (addr >> 4)`, where `meta_bias = meta_base − (heap_lo >> 4)`, a per-heap constant held in the
`Mutator`.

```
bit 7  LOG1     word 1 of this granule is armed (first store must be logged)
bit 6  LOG0     word 0 of this granule is armed
bit 5  HASH_HI  ─┐ 00 unhashed · 01 hashed (hash = f(address)) · 10 hashed-and-moved (extension word holds it)
bit 4  HASH_LO  ─┘   (valid on the object's first granule)
bit 3  START    first granule of an object (set when marked or copied; used by the verifier and by hash lookup)
bit 2  PINNED   never move this object (mutator-set)
bit 1  —        reserved (mark epoch, if the clear pass ever shows in profiles)
bit 0  MARK     marked in the current major (written on every granule of the object's extent)
```

How the byte is used:
- **Marking writes MARK on every granule of an object** (Chez's per-granule extent marks
  [barrier-res §3]). A line is free iff the 8 bytes covering it, read as one `u64`, have no MARK bit. So no
  END bit is needed, and allocation never writes metadata, unlike Whippet's 8–10-instruction allocation
  [whippet §1.3].
- **MARK bits are cleared** in mature blocks at the start of each major: one pass over 1/16 of the heap,
  about 0.3 ms for 100 MB.
- **Nursery bytes stay zero** except HASH and PIN, and the nursery's metadata is `memset` after each minor
  (256 KiB for 4 MiB, ~10 µs).

**Block table**, 16 B per block (0.05%): `kind: u8` (FREE, NURSERY, MATURE, LOS_HEAD, LOS_TAIL), `flags: u8`
(HAS_PINNED, EVAC_CANDIDATE, HAS_PENDING_KEYS), `live_lines: u16`, `owner: u16`, `next: u32`,
`live_bytes: u32`. It is indexed by `(addr − heap_lo) >> 15`.

### 4.4 Decommit

- After each major, free blocks beyond the reserve (nursery target + evacuation reserve + 25% of the mature
  limit) are returned oldest-first with `MADV_FREE` (macOS) or `MADV_DONTNEED` (Linux). Both were measured
  to drop RSS immediately [demographics §8.2].
- Freed LOS runs of 64 KiB or more are decommitted immediately.
- This removes today's 464 MiB of empty arena capacity retained after a library load [libload §0.5].

---

## 5. Allocation

### 5.1 Contract (kept)

**Allocation never collects.** The slow path:
1. retires the current buffer;
2. takes the next nursery chunk, or a mature hole for old allocations;
3. **raises the GC event** when the nursery target is crossed, and returns.

The collection runs at the next safepoint (§9).

I keep this contract for three reasons, from a JIT-first view:
1. **No safepoint at allocation sites.** So there is no publish/reload, and allocation *grouping* plus
   initializing-store barrier elision both work (JEP 475's rule [barrier-res §5]).
2. **Rust primitives stay handle-free.** 280 heap-only primitives keep holding raw `TaggedValue`s across
   allocation [prim-embed §3.1]; Chez runs a moving generational collector under the same contract
   [DIGEST fact 1].
3. **SSA temporaries survive the refill call.** That includes a half-built list. jit-readiness R7 asks for
   exactly this.

### 5.2 Fast paths

**Rust (interpreter and primitives):**

```rust
#[inline(always)]
pub fn alloc_bytes(&mut self, size: usize) -> NonNull<u64> {          // size: multiple of 16, ≤ 8 KiB
    let p = self.abi.ap; let n = p.wrapping_add(size);
    if n > self.abi.alloc_limit { return self.alloc_slow(size) }        // #[cold], never collects
    self.abi.ap = n; unsafe { NonNull::new_unchecked(p as *mut u64) }
}
```

Constructors take their contents: `alloc_pair(car, cdr)`, `alloc_closure(entry, &fvs)`,
`alloc_vector_from(&[TV])`. Initializing stores therefore never take a barrier.

**JIT (aarch64).**
- At fragment entry and after every safepoint, `ldp x22, x23, [x21, #AP]` loads `ap` and `limit`; `x21` is
  the pinned `Mutator` register.
- Allocations in one basic block with no safepoint between them are **grouped**: one compare covers the
  summed size.
- `cons` (fast path 3 instructions plus stores):

```
add   x9, x22, #16          ; (group: #16·k + closure sizes …)
cmp   x9, x23
b.hi  refill_cold           ; leaf call rt_refill(x21, bytes) → (ap, limit) in x0/x1; may return OOM
stp   xcar, xcdr, [x22]     ; initializing stores, no barrier
orr   xres, x22, #4         ; tag
mov   x22, x9               ; renamed by regalloc
...   str x22, [x21, #AP]   ; once, before the next safepoint or fragment exit
```

- A 2-fv closure is the same, with size 32: `adr x10, entry_block+8` (or `movz/movk` of the non-moving
  address), then `stp x10, fv0, [x22]; str fv1, [x22,#16]; orr xres, x22, #5`.
- Allocations larger than 256 B, variable-size vectors and strings go through a leaf helper (jit-ready R9),
  which still never collects.

### 5.3 Slow path, overdraft, hard limit

`rt_refill(m, size)` handles these cases, in order:
- **Size ≥ 8 KiB:** the LOS allocator. The object is pushed onto the mutator's *remember-whole* list
  (§7.4).
- **Otherwise:** the next free chunk, giving bump `[chunk, chunk + 256 KiB)`.
- **Nursery target crossed** (`nursery_used ≥ nursery_target`): set `event |= GC_MINOR` and zero
  `reg_limit` (§9). Allocation continues in **overdraft** chunks up to `overdraft_limit` = max(4 × nursery
  target, 64 MiB).
- **Overdraft limit reached:** allocate from mature holes, still raising the event.
- **`max_heap` reached** (default: the lower of 75% of physical memory and the reservation): return
  `Err(Oom)`.
  - A primitive turns it into `EvalError::OutOfMemory`, raised at the instruction boundary.
  - The JIT's cold path calls `rt_raise_oom`, a may-GC helper (§9.2).
  - Out-of-memory is a Scheme error, never an abort.

Overdraft is reachable only by Rust code that allocates in a long loop without returning to a safepoint
(`make-list 10000000`, the reader, `list-copy`). Polls bound straight-line JIT and interpreter code.

---

## 6. Collection

### 6.1 Overview

- **Stop-the-world, single-threaded** for the mutator count of 1. The protocol is written for N (§12).
- **The production plan is `GenImmix`** with three per-heap policies:
  - `Generational` (default for VM heaps after Stage 9);
  - `NonMovingFullHeap` (the tree-walker's heaps, and the default before Stage 9);
  - `Torture` (§14).
- `Null` (never collect) is a separate cargo-feature plan, used for the "off" lane (§11).

### 6.2 Minor collection (policy `Generational`)

1. **Safepoint handshake.** Each mutator retires its buffer and publishes its top `fp`.
2. **Decide the mode.** If the last minor's byte survival exceeded **30%**, or three consecutive minors
   exceeded 20%, run *in-place mode* (step 7). Blocks with `HAS_PINNED` are always promoted in place.
3. **Roots.**
   - Register-stack frames *above each green thread's watermark* (§10.3), through the descriptor maps.
   - The remembered set: exact slot addresses and range entries.
   - Each mutator's remember-whole list.
   - Mutator root stacks and owned handles.
   - Tracer snapshots.
   - Frames' `closure` words.
4. **Evacuate.** For each root slot holding a nursery reference (block kind NURSERY): if the object is
   forwarded, rewrite the slot. Otherwise copy it into the **mature allocator** (promote on first survival;
   80–100% of survivors live on in retaining workloads [demographics §5.4]). Then:
   - write `[FWD_MARKER, new]` into from-space;
   - set `START` on the copy;
   - set `LOG0|LOG1` on every granule of the copy, which arms it;
   - transfer HASH state, appending the extension if the old state was `HASHED` (§6.9).
5. **Scan.** Push each copy on a worklist and scan its slots until empty (Cheney order through an explicit
   worklist, because mature targets are holes, not one contiguous to-space).
6. **Re-arm** each remembered slot after processing it. Clear the remembered set.
7. **In-place mode.** Nursery objects reachable from roots are *marked* (MARK, START, LOG on their granules)
   instead of copied. Their blocks become MATURE, and their holes become recyclable lines. This bounds
   queue3 and deeprec, where nursery copying costs 1.7–1.8× today's marking [demographics §5.2], to roughly
   the cost of marking.
8. **Ephemerons** (§6.5), **host payloads** created since the last minor (traced through their slot
   iterators), then the epilogue (§6.10, without code release or port finalization).
9. **Reset.** `memset` the evacuated nursery blocks' metadata, return them to the free pool, and in torture
   mode poison them (§14).

**No code-liveness decisions at minors.** Descriptors are released only after a complete mark
[cont-repr §4].

### 6.3 Major collection

1. Run a minor's evacuation first: the nursery is emptied into the mature space.
2. Clear MARK bits in mature blocks.
3. With evacuation enabled (Stage 10): pick **candidate blocks** whose `live_lines` at the previous major was
   below 50%, sorted ascending, until their live bytes fill the reserve (2.5% of the heap plus free blocks).
   This is Immix's default headroom, with low sensitivity between 1% and 3% [immix-mmtk §2].
4. **Mark** from all roots, including the entire register stack, since the watermark is ignored at majors.
   - Objects in candidate blocks that are not pinned are evacuated, with in-object forwarding.
   - Every marked granule gets `MARK|START?|LOG0|LOG1`. This **re-derives all log bits** and replaces the
     remembered set [barrier-res §7.1 item 7].
   - Per-block `live_bytes` and `live_lines` are accumulated during the pass.
5. Ephemeron and weak fixpoint (§6.5), then the full epilogue (§6.10).
6. **Sweep.** Blocks with `live_bytes == 0` go straight to the free pool. Partially live blocks join the
   *recyclable* list. Their holes are found **lazily** by the mature allocator, which reads 8 metadata bytes
   per line, and resets the metadata of reused lines. No dead object memory is ever touched. That removes
   today's 2.4 ns × every arena slot sweep and the 176 ms post-load sweep [demographics §3].

**Mark stack.**
- It is segmented: 4 KiB segments taken from a cache, so it grows without relocating.
- For a pair, **push the cdr and continue with the car.** The stack then grows with car-nesting depth, not
  with list length, which removes today's +55 MB for a 2 M-element list of 2-vectors [DIGEST §1.4].
- Vectors are pushed as `(vector, next_index)` slices of 256 elements.
- Worst case is O(car depth), bounded by heap size / 16.

### 6.4 Evacuation and pinning policy

- **Who pins.** Objects are pinned by `Mutator::pin(v)`, which sets `PINNED`: FFI-lent buffers, debugger
  event payloads, and any object a non-updatable holder must see. The tree-walker needs no pins because its
  heap policy never moves objects.
- **Pinned in the nursery.** The block is promoted in place (JEP 423 / generational ZGC style
  [DIGEST §3.5]).
- **Pinned in the mature space.** The object is marked in place, and its block can never become free while
  it lives.
- **Pinned at allocation.** Symbols, RTDs, global cells, code-constant vectors and the canonical
  zero/inf/NaN boxes are allocated pinned and old. The JIT embeds their addresses (§8.2), so they must not
  move.

### 6.5 Weak references and ephemerons (key-indexed, no round rescans)

**Generational rule.**
- Ephemerons are always nursery-born and immutable to the mutator.
- At a minor GC, a young ephemeron whose key was not reached is **broken**: both fields are set to BWP. The
  minor is complete for young objects given the remembered set.
- A promoted ephemeron's key and value were promoted with it, so **old ephemerons never reference young
  objects**. No remembered-set edges exist for ephemerons [finalization §3.10 rule "E never older than K or
  V"].

**Resolution in linear time.**
1. When the tracer meets an ephemeron whose key is unmarked or unforwarded, it inserts
   `key_addr → ephemeron` into `pending: FxHashMap<usize, EphRef>`. Ephemerons with the same key chain
   through their 4th word, the GC link.
2. It then sets `HAS_PENDING_KEYS` on the key's block.
3. Whenever the tracer marks or forwards an object whose block has `HAS_PENDING_KEYS`, it looks the object
   up in `pending`. On a hit it traces the values of every chained ephemeron and removes the entry.
4. When the worklist drains, the remaining entries are broken.

**Cost:** one flag test per marked object, plus one hash probe per object in a flagged block. The 16 k chain
that costs 289 ms in the bad order today [finalization §0.7] becomes O(n).

**Kept semantics:**
- `(gc)` requests a **major**, which breaks dead-key pairs of every age (`ephemerons.rs` tests).
- Liveness is precise in frames and chunks; the #423 tests forbid conservative scanning [finalization §0.8].
- `reference-barrier` is an opaque use: a call to a leaf identity helper in JIT code.
- All weak kinds, meaning ephemerons and, later, guardians and transport cells, resolve in **one**
  fixpoint (pitfall 7).

### 6.6 Finalization and ports (R1–R6)

Ports become `{hdr T_PORT, port_id}` heap objects, allocated old. The `PortTable` (per heap) owns the
`PortData` (buffers and descriptor), the open flag and a `finalize` method that flushes, closes and ignores
errors.

| Requirement | Mechanism |
|---|---|
| **R1** output written at exit | `end_process` (including `emergency-exit`, matching today's pinned test) flushes every *open* entry in the table, live or garbage. Entries leave the table only when closed or finalized |
| **R2** dead file port flushed and closed before Scheme resumes | major epilogue: entries whose handle is unmarked → `finalize()` → remove. Minors never reclaim ports, which are old |
| **R3** no `EMFILE` while garbage holds descriptors | `open-*` becomes resumable. On `EMFILE` it returns `Step::CollectAndRetry`: the machine runs a major at its safepoint and re-invokes once, and a second `EMFILE` raises a file error. The trigger also requests a major when open descriptors exceed 75% of `RLIMIT_NOFILE` (§13) |
| **R4** no Scheme and no allocation in finalizers | `finalize` is Rust on `PortData` only |
| **R5** interpreter drop flushes and closes | `Drop for Heap` runs every table finalizer before unmapping |
| **R6** one port, one object | `current-*-port` are parameters whose stored value is the port object. The `thread_local!` port cells (`prim:io/ports.rs:41-45`) move into the dynamic environment. Fixes `(eq? (current-output-port) (current-output-port))` [finalization E4] |

- GC-time flushing is observable, so the differential lanes must not depend on it. Nothing in the suites
  does today [finalization §0.4].
- I add oracle-scored tests for R2 and R3 (chibi and Gauche agree on both).
- `MemoryFs` writers are payloads too; their commit runs in `finalize`.

### 6.7 Code liveness

- **Marking.**
  - Descriptors are marked at majors through closure entries, frame `code` words, stack chunks,
    descriptor → nested descriptors (`MakeClosure` operands) and descriptor → constants.
  - Unmarked descriptors are released after the epilogue, together with their entry blocks and JIT bodies,
    in one W^X-toggled batch.
  - This deletes `live_closures`, `gc_freed_closure_code_ids` and `RETIRED_VM_CLOSURE_CODE`
    (`vm:runtime/vm_state.rs:542-575` [V]).
- **The release bound.** Code is released "within a bounded number of collections"
  (`finished_forms_release_code.rs`), and a major is now that bound. The trigger must therefore reach a
  major after a bounded number of minors while descriptors are pending release; the trigger's
  "code-release pressure" term forces it after ≤ 64 minors.
- **Native activations.** Under S1 (§10.4) no native activation survives a safepoint. Under S2 every
  suspended native frame mirrors a VM frame that marks its descriptor. Release at a safepoint is therefore
  safe without epochs.

### 6.8 Sweep

- Lazy, per block, metadata only (§6.3 step 6).
- Payload tables are swept eagerly in the major epilogue. Their size is the count of foreign objects, about
  1% of allocations once Stage 6 is done.

### 6.9 Identity hash

`eq-hash(v)`:
- **Immediates:** `mix(bits)`.
- **Heap objects:** read the HASH bits of the first granule:
  - `00` → set `01` and return `mix(addr ^ heap_seed)`;
  - `01` → return `mix(addr ^ heap_seed)`;
  - `10` → read the extension word at `addr + size(obj)`.

When the GC copies a `01` object, it allocates `size + 16`, stores `mix(old_addr ^ seed)` in the extension
and sets `10` on the copy. That is the Bacon–Fink–Grove scheme also used by JDK compact headers [hotspot §3.10].

This scheme:
- works for headerless pairs and closures, which is the eqtable case: 1.66 M `identity-hash` calls on pair
  keys [demographics §7.1];
- needs no pinning;
- costs nothing for objects that are never hashed.

`equal-hash`'s index fallback becomes `eq-hash`. SRFI 69's stored hashes stay valid across moves.

### 6.10 Epilogue order

1. Trace to a fixpoint, with ephemerons resolved key-indexed (§6.5) and host-payload slots traced as edges.
2. Break the pending ephemerons (BWP).
3. *(future, SRFI 254)* guardians and transport cells, which fill queues only.
4. GC-processed weak tables: the symbol interner, only if made weak (owner decision 11).
5. Host payloads whose handle is dead: drop them, which finalizes ports (R2) and commits `MemoryFs`.
6. Code release (majors only).
7. Heap sizing and stats, the decommit pass, then release mutators.

This is finalization §4.9's order with the continuation weak table removed, because continuations are now
ordinary heap objects.

---

## 7. Write barrier

### 7.1 Kind

**Field logging**, pre-write, with an armed-bit polarity in which fresh memory is unarmed (LXR, Whippet mmc
and pcc, MMTk FieldBarrier) [barrier-res §0.3, §7]. I choose it because:
- it needs no heap parsability, unlike cards with headerless pairs [barrier-res §3];
- its remembered set is exact and deduplicated: nboyer takes **5** slow paths, against 24,504 entries for an
  OCaml-style ref table [barrier-res §2.3];
- it fits both the copying nursery and in-place promotion;
- it keeps the old value available for a future SATB mode.

### 7.2 Fast path (aarch64)

`x_bias` holds `meta_bias`, loaded from `[x21, #META_BIAS]` once per fragment (a `readonly` load that LICM
can hoist). `xs` is the slot address, which the store needs anyway.

**Static parity** (offset known statically): `WriteCell`, `set-car!`, `set-cdr!`, record set, closure
fix-up, global cell. Example `WriteCell`, where the value is word 1, so the bit is LOG1 (bit 7):

```
      tbz   xv, #2, 1f            ; immediate → no barrier            (1)
      lsr   x10, xs, #4           ;                                   (2)
      ldrb  w11, [x_bias, x10]    ;                                   (3)
      tbnz  w11, #7, log1_cold    ; armed?                            (4)
1:    str   xv, [xs]
```

**Dynamic parity** (`vector-set!`; element *i* is at word 1+*i*):

```
      tbz   xv, #2, 1f            ; (1)
      lsr   x10, xs, #4           ; (2)
      ldrb  w11, [x_bias, x10]    ; (3)
      ubfx  x12, xs, #3, #1       ; parity of the word within its granule (4)
      lsrv  w11, w11, w12         ; bring LOG0 or LOG1 to bit 6       (5)
      tbnz  w11, #6, log_cold     ; (6)
1:    str   xv, [xs]
```

- **Totals:** 4 instructions (static parity) or 6 (dynamic).
- **On x86-64:** `test v,4; jz; mov; shr; movzx; test; jnz`, which is 5 or 7.
- **Unverified:** whether Cranelift selects `tbz`/`tbnz` for `brif (band_imm x, 1<<k)`. If it does not,
  each test costs one more instruction (`tst` + `b.ne`), giving 6 and 8. Measuring this is part of M3.

### 7.3 Slow path (inline, cold block, **no call**)

```
log1_cold:
      and   w11, w11, #0x7f        ; disarm (dynamic: bic with 1<<(6+parity))
      strb  w11, [x_bias, x10]
      ldr   x12, [x21, #REMSET_CUR]
      str   xs, [x12], #8
      str   x12, [x21, #REMSET_CUR]
      ldr   x13, [x21, #REMSET_SOFT]
      cmp   x12, x13
      b.lo  1b
      ldr   w14, [x21, #EVENT] ; orr w14, w14, #GC_MINOR ; str w14, [x21, #EVENT]
      str   xzr, [x21, #REG_LIMIT] ; b 1b
```

About 12 instructions with no call, so Cranelift sees no safepoint and tier 2 suffers no spills
[barrier-res §6].

- The buffer is reserved VA (256 MiB), with a soft limit at 8 MiB (1 M entries). Polls bound straight-line
  code, so its end is unreachable from JIT code.
- Rust bulk operations call `Mutator::remset_reserve(n)`, which grows the committed range.

### 7.4 Filters and elision

**Dynamic filters, in order:**
1. value is immediate (`tbz`), removing 64% of stores in aggregate [demographics §6.2];
2. holder is young: free, because nursery bits are never armed;
3. already logged this cycle: free, the bit is clear.

**Static elision** (the JIT and the bytecode compiler, through one `emit_store(builder, holder, slot,
value, facts)`, following the JEP 475 lesson [barrier-res §6]):
- **Immediate values:** literals, chars, booleans, and results of `Not`/`NullP`/`PairP`/`VectorP`/`Eq`/
  `Lt`/`NumEq` and their `Imm` forms. Not raw arithmetic, which can overflow to a bignum.
- **Fresh holders:** stores into objects allocated in the same fragment since the last safepoint.
- **Register-window and frame stores:** these are roots.
- **Stack chunks:** immutable after capture.
- **Pointer-free objects:** strings and bytevectors.

**Remember-whole list.** Objects allocated **directly old** are pushed here: LOS vectors, pretenured
symbols, cells, RTDs, constant vectors, foreign objects and large stack chunks. The next minor scans each one
whole, then arms it. This is G1's `on_slowpath_allocation_exit` compensation [barrier-res §5].

**Bulk stores.** `vector-fill!`, `vector-copy!` and `list->vector` into an old vector push one range entry
`(start | 1, len)` and disarm the range's bits with a `memset`-style AND.

### 7.5 The Rust store funnel

Every mutation of a heap slot goes through these methods. Nothing else can produce a `*mut TaggedValue`
into the heap.

```rust
impl Mutator {
    #[inline(always)] pub fn write_field(&mut self, holder: TaggedValue, word: u32, v: TaggedValue);
    #[inline(always)] pub fn set_car(&mut self, p: TaggedValue, v: TaggedValue);    // = write_field(p,0,v)
    #[inline(always)] pub fn set_cdr(&mut self, p: TaggedValue, v: TaggedValue);
    #[inline(always)] pub fn vector_set(&mut self, vec: TaggedValue, i: usize, v: TaggedValue) -> Result<(), RangeError>;
    #[inline(always)] pub fn cell_set(&mut self, c: TaggedValue, v: TaggedValue);
    #[inline(always)] pub fn record_set(&mut self, r: TaggedValue, field: u32, v: TaggedValue);
    #[inline(always)] pub fn global_set(&mut self, cell: CellRef, v: TaggedValue);  // + WATCHED invalidation
    pub fn write_range(&mut self, vec: TaggedValue, start: usize, vals: &[TaggedValue]);
    pub fn fill_range(&mut self, vec: TaggedValue, start: usize, len: usize, v: TaggedValue);
}
// Initializing stores: only through a fresh-object token that cannot outlive the &mut borrow,
// so no safepoint can intervene between allocation and initialization.
pub struct Fresh<'m> { obj: TaggedValue, _m: PhantomData<&'m mut Mutator> }
impl<'m> Fresh<'m> { pub fn init(&mut self, word: u32, v: TaggedValue); pub fn finish(self) -> TaggedValue; }
```

Consequences:
- `vector_slice_mut` (`core:heap/mod.rs:816` [V]; its only non-test caller is `vm_state.rs:2380`) is
  removed.
- Records, parameters and promises lose their `Rc<RefCell>` payloads, which close the three
  barrier-bypassing channels in `prim:records.rs:262`, `prim:parameters.rs:168,267` and `prim:lazy.rs:134`
  [DIGEST §1.5].
- The funnel is also the **single place** where a threaded build adds its publication fence
  [threads-rec §2 item 4].

---

## 8. Roots and rooting

### 8.1 VM frames

- **Precise.** Frames are walked through the interleaved register stack (§10.1). For each frame, the GC
  reads `code` (a `*const CodeDesc`) and `pc`, then the liveness map *at that safepoint*.
- **Maps are compressed** to safepoint pcs only: call return points, polls and may-GC helper sites. They are
  stored as a sorted `(pc, map_index)` table into deduplicated bitsets. Today they cost 40 B per pc, 65% of
  the instruction stream [libload §2.2].
- **Dead slots are skipped, not overwritten.** In torture mode the GC *poisons* skipped slots with
  `GC_POISON`, which turns any map error into an immediate assertion.
- The `closure` header word is a traced, updatable TV. It replaces `Option<HeapIndex>` and its special trace
  path (`vm:types/mod.rs:41-60` [V]).

### 8.2 JIT frames, per tier

| Tier | Values held natively between safepoints | At a non-leaf safepoint | Cranelift stack maps |
|---|---|---|---|
| **Tier 1 (baseline, template)** | VM registers cached in SSA within a fragment; `ap`/`limit`; `fp`; `x_bias` | Every dirty cached value is stored to its window slot and `pc` is written. The call is a full memory clobber (alias regions [cranelift-gc §2.7]). Reload after | **none** |
| **Tier 2 (optimizing, later)** | Anything, including unboxed f64 and untagged ints, plus derived pointers *within* a block | All live values go to the window. Unboxed values go to **raw slots**, which the descriptor's per-safepoint map marks as raw, so the GC skips them and capture copies their bits. Derived pointers never cross a safepoint (pitfall 9) | **none** |
| *Tier 3 (excluded)* | native-frame GC references | — | would need a frame walker and deopt for `call/cc`; excluded by contract (§11.4) |

**Leaf calls are not safepoints.** That covers `rt_refill`, fixnum overflow promotion, `eqv?`/`equal?`,
heap-only primitives and the encode slow path. These calls never collect, so values stay in SSA registers
across them, and only Cranelift's ordinary caller-saved register pressure applies.

Each helper is declared `Leaf` or `MayGc` in a machine-checked table, a `#[helper(class = …)]` attribute
(jit-ready R17). `MayGc` covers closure calls, resumable and higher-order primitives, raise, `call/cc` and
the wind operations.

**Constants.**
- JIT code embeds *only non-moving addresses*: entry blocks, descriptors, pinned symbols, RTDs, global
  cells, canonical boxes and pinned constant vectors.
- Every other constant is loaded from the descriptor's pinned constants vector with one load.
- There is no code patching at GC (pitfall 10).

### 8.3 Rust code

- **Primitive signature:** `fn(&mut Cx<'_>, &[TaggedValue]) -> Result<TaggedValue, EvalError>`, where
  `Cx { m: &mut Mutator, inst: &Instance }` [prim-embed §11].
- **Primitives hold raw `TaggedValue`s freely.** They cannot reach a safepoint: allocation never collects,
  and callbacks into Scheme are `Step::Call`/`Step::Eval` [AGENTS.md].
- **LIFO root scopes on the `Mutator`** cover the few boundaries that span a safepoint:

```rust
let s = cx.m.root_scope();               // RAII: truncates on drop
let h: Root = s.root(v);                 // index into Mutator::roots (traced, updatable)
/* … safepoint may occur: library body runs, nested driver collects … */
let v = s.get(h);
```

Users: library-loading points A, B, D and E, `with_globals`, `across_reentry`, `ParsedLibrary.body` (moved
into the registry), and `%parameterize-swap!` until it is made resumable [libload §4, prim-embed §11].

### 8.4 Tree-walker policy (it may lag)

- **Its heaps use `NonMovingFullHeap`.**
  - Every collection marks in place.
  - There are no minors and no evacuation.
  - Nursery chunks are promoted in place at each GC.
  - It uses the same object model, primitives and funnel. Its barrier fast path falls through, because no
    bit is ever armed.
- **Roots.** Its existing `GcRoots` visitor keeps visiting by value, as *pinning* roots. That is sound
  because nothing in a tree-walker heap moves.
- **Payloads.** `CpsLambda`, `CpsContinuation`, `ContValue` and `ContEnv` stay `Rc` graphs, owned by
  foreign objects through the `HostPayloadTable`. They are traced when their handle is marked and dropped
  when it is not [cont-repr §5].
- **Safepoints** stay at the outermost trampoline steps (`tw:eval/cps_eval/mod.rs:213-230`). Its
  `NoGcRegion` (today's `GcDeferGuard`) stays.

### 8.5 Embedding

- `Interpreter::eval_*` returns `Owned`: an index plus generation into a per-heap handle table that the GC
  updates. `Drop` frees the slot (Wasmtime `OwnedRooted`, JNI global references).
- `interp.with(|cx| …)` gives scoped access in which raw values never escape.
- `display_tagged(impl AsValue)` keeps most of the ~200 test call sites compiling.
- **Heap teardown is mandatory.**
- This fixes DIGEST defects 1 (embedder use-after-free) and 2 (interpreter drop leaks).

### 8.6 Fate of each off-heap holder

| Holder (today) | Fate | Stage |
|---|---|---|
| VM register file `Vec<TV>` + `frames: Vec<CallFrame>` | per-green-thread reserved register stack with interleaved, position-independent frames | 4 |
| `CallFrame.code: Rc<CodeObject>` (4 `Rc` operations per call/return) | raw `*const CodeDesc` (non-moving, marked) | 4 |
| `CodeObject.constants: Vec<TV>` (traced every GC) | pinned heap vector referenced from the descriptor; traced at majors, remember-whole at creation | 4 |
| Environments, globals, `FORWARDED`/`Owner` links | `Namespace` (Rust map name → `CellRef`) plus pinned heap cells | 5 |
| `Library.exports: HashMap<String, TV>` copies | name → `CellRef` | 5 |
| `CompiledMacro` literals (+ untraced `foreign_expansions`) | literal pool as a heap vector held by the macro's foreign object; payload slots updatable | 6 |
| VM continuation side tables, `VmContinuationRef` | deleted: continuations are heap objects | 4 |
| `WindRecord.handlers: Rc<[H]>` copies | green-thread `Vec` (updatable) in Stage 4; persistent heap list in Stage 11 | 4/11 |
| `symbol_table`, `core_syntax_table` (`HashMap<String, HeapIndex>`) | `Interner: FxHashMap<Box<str>, SymbolRef>` over pinned symbols; core syntax in cells | 3/5 |
| `syntax_sources` (316 MiB with entries), `SourceMap.locations`, `child_source` | inline source ids in identifiers and reader pairs; the two unread stores deleted [libload §5] | 6 |
| transient raw-bits sets (writer, parser, `quoted`, `OpenNodes`, memos, `PrimitiveCallMap.by_value`) | unchanged: still valid because no GC happens inside a Rust call | — |
| `Parameter{values: Rc<RefCell<Vec>>}` | deep-bound parameterization (heap) [threads-rec item 9] | 11 |
| record `Rc<RTD>` + `Rc<RefCell<Vec>>` | inline record fields with an RTD header | 3 |
| `Port(Rc<Port>)` + `CURRENT_*_PORT` thread-locals | port object + `PortTable`; current ports in the dynamic environment | 6 |
| embedder-held values (unrooted) | `Owned` handles | 5 |
| `scratch_args`, tracer snapshots, `pending_escape` | `Mutator` root region (updatable) | 2/4 |
| tree-walker `StepResult`, CPS graphs, `PENDING_ESCAPE` | unchanged; pinning roots in a non-moving heap | — |

---

## 9. Safepoints and polling

### 9.1 The event word

The `Mutator` holds `event: AtomicU32`. Its bits are: `GC_MINOR`, `GC_MAJOR`, `PREEMPT`, `SIGNAL` (Ctrl-C),
`DEBUG`, `TERMINATE`, `HANDSHAKE`, `STACK` (register-stack growth). It also holds `ticks: i32`.

- **Posting an event** sets the bit and stores `0` into `reg_limit`, the register-stack limit that every
  Scheme call checks anyway (§9.2). This is V8's stack-limit fold and OCaml's `young_limit` idea
  [cranelift-gc §4], applied to the call path rather than to allocation.
- **Why not fold it into the allocation limit.** Allocation slow paths never collect, so a zeroed
  `alloc_limit` would only buy a wasted refill.
- **Only the owner resets `reg_limit`.** It re-checks `event` after the reset (OCaml's
  `caml_reset_young_limit` protocol [ocaml-gambit §2.4]). With one mutator these are plain stores.

### 9.2 Placement

| Site | Interpreter | JIT | Extra cost |
|---|---|---|---|
| non-tail call (frame push) | `new_top > reg_limit` check | `add; cmp; b.hi cold` in the callee prologue | **0**: the overflow check is needed anyway |
| non-self tail call | same check on window resize | same | 0 |
| self-tail-call back-edge (every loop; codegen emits forward jumps only [jit-ready §2.2]) | `event != 0` check | `ldr w9,[x21,#EVENT]; cbnz w9, cold` | 2 instrs |
| return to the driver / trampoline | check | check | — |
| `MayGc` helper return | services a pending event before resuming | same | — |

- **The interpreter's per-instruction poll goes away.** It costs +1.1–1.4% today [DIGEST §1.9]. Every loop
  passes through a call or a tail call, so the polls above bound straight-line code.
- **Collection is serviced only at the outermost driver**, or at a nested driver whose `no_gc_depth == 0`
  (§9.4).
- **Async interrupts in JIT code** come out nearly free: owner decision 9, default "yes".

### 9.3 Deterministic mode

- `PollKind::Tick` is selected by heap config for test lanes and SRFI 18's deterministic scheduler.
- Every poll site also does `ldr; subs; str; b.le` on `ticks`, and the quantum is fixed.
- GC events come from byte thresholds, which are deterministic for a deterministic program.
- Preemption therefore happens at the same points on every run, and the differential lanes stay
  byte-identical [threads-rec §2 item 7].
- The JIT reads `PollKind` from `GcAttrs` and emits exactly one poll sequence per site.

### 9.4 Nested Rust loops: what replaces `GcDeferGuard`

1. **Rooted boundaries.** Each nested driver whose callers have published their Rust-held values with root
   scopes may collect: library loading A, B, D, E, `across_reentry`, `run_apply_proc`, and `Step::Eval`
   import loading. This lets loads collect between top-level forms, which cut the `(nieper rbtree)` malloc
   peak from 211 to 118 MiB [libload §0.6].
2. **`NoGcRegion`.** This is the renamed `GcDeferGuard`. It increments `Mutator::no_gc_depth` and remains in
   only two places:
   - desugarer expansion point C (mid-form literals and raw-bits memos) until syntax-case brings an
     `ExpansionContext` with a literal pool [libload §7];
   - the tree-walker trampoline.

   Inside a `NoGcRegion` the poll sees the event and defers. Allocation overdrafts (§5.3), and the next poll
   outside collects. This is the GCLocker failure mode (pitfall 2), confined to two places and measured by a
   counter of deferred polls.
3. **Green-thread preemption** is likewise deferred while `reentry_depth > 0`, because a thread cannot be
   suspended inside a Rust re-entry [threads-rec §1.5].

---

## 10. Continuations and stacks

### 10.1 Register stack and frame format (Stage 4)

- **One reserved, non-relocating region per green thread.** The main thread gets 8 GiB of VA, the others
  64 MiB by default; each is lazily committed with a guard page.
- **Frames are interleaved with their windows and position-independent:**

```
fp+0   ret       raw  resume address in the caller (JIT fragment or interpreter-resume trampoline)
fp+8   link      raw  bits 0–31: caller distance in bytes (fp − caller_fp); bits 32–47: argc; 48–63: flags
fp+16  code      raw  *const CodeDesc of this frame
fp+24  pc        raw  bits 0–31: this frame's bytecode pc at its last safepoint; 32–63: stub kind
fp+32  closure   TV
fp+40  r0 … r(n−1)  TV (or raw slot, per map)
```

- **The header is 40 B, as today's `CallFrame` is** [jit-ready §0]. A 6-register frame (fib's mean window)
  is 88 B, against about 137 B of RSS per level measured today [DIGEST §1.7]; 10 M such levels fit in
  880 MB of the reservation.
- **Results are delivered in `x0` to the resume address.** The caller's resume code stores the value to its
  statically known `dst`. `return_reg` leaves the callee frame; delimited re-pointing becomes "the outermost
  captured frame's `ret` is the invoke site's resume point".
- **JIT `Return` (S1):** `ldp x9, x10, [fp]; sub fp, fp, w10, uxtw; br x9`, with the value in `x0`. That is
  3 instructions.
- **Depths become byte offsets from the stack base.** Prompts, handlers and winds record `fp − base`.
  Relocation is integer arithmetic, as today's depth-integer discipline already is [cont-repr §0.2].

### 10.2 Capture: Design A first (Stage 4), C′ later (Stage 11)

- **Stage 4, Design A.**
  - `call/cc` allocates a `T_STACK_CHUNK` and `memcpy`s the frames `[base, fp_top + frame_size)`. Frames are
    position-independent, so no fix-ups are needed.
  - Dead slots in the copy are cleared per map, and `deliver` is cleared (the 296 MB lesson, pitfall 18).
  - It also allocates a `T_CONT` holding winds, handlers and prompts as arrays.
  - Invoke copies back.
  - This deletes `continuation_store`, `delimited_continuation_store` and the VM's `trace_weak_ids` and
    `sweep_weak`.
  - The toy measured 5.8× lower capture overhead than today [cont-repr §2.2].
  - Chunks of 8 KiB or more go to the LOS and are remembered whole; smaller ones are nursery-born and
    immutable.
- **Stage 11, C′** [cont-repr §3.1]:
  - **Capture** freezes only frames above the *frozen watermark* into a new immutable chunk linked to the
    previous chain. Capture becomes O(frames resumed since the last freeze); the toy measured 57 ns at
    depth 1000.
  - **Invoke** grafts the chain and thaws lazily through an **underflow stub frame**.
  - **Overflow** freezes all but the top *k* frames and slides them down, which keeps 10 M-deep recursion
    in a fixed buffer.
  - **Winds, handlers and prompts** become persistent heap lists (Chez `winders`, Gambit `denv`).

### 10.3 GC interaction: one watermark, implemented as a return barrier

- **Minors scan only frames above each green thread's watermark.** Frames below it were scanned and fixed up
  at the previous GC. Their young referents were all promoted, and a suspended frame cannot be written.
- **The GC installs the barrier itself**, at the watermark frame, by swapping that frame's `ret` for
  `WATERMARK_TRAMPOLINE` and saving the original in the green thread (Loom / JEP 376 style
  [jit-ready §4]).
- **Normal returns pay nothing.** A return into the watermark frame lands in the trampoline, which lowers the
  watermark by one frame (re-installing itself on the next frame down) and jumps to the saved `ret`.
- **Helpers that write into suspended frames reset the watermark to the stack base:** reinstatement, `append_
  delimited`, and hole delivery.
- **Capture copies the saved `ret`, never the trampoline.**
- This is the deeprec fix: 11.5 M registers scanned per minor today, about 12–15 ms [demographics §7.2].
- **The C′ frozen watermark is the same mechanism** in Stage 11 [cont-repr §6 Stage 4]. That is why it lands
  with the nursery (Stage 9), not later.

### 10.4 JIT interaction

- **S1 (default baseline).** Each bytecode function compiles into Cranelift fragments with
  `CallConv::Tail`, split at non-tail call sites.
  - Every call and return is a `return_call_indirect`, so native depth stays constant and the GC never meets
    a native frame.
  - Escapes and reinstatement are "the helper returns a new target" with no unwinding.
  - Resume entries are the fragments; the descriptor's `resume[k]` table maps return point `k` to a
    fragment.
- **S2 (measured alternative).** Native `bl`/`ret` for JIT→JIT calls, with the same VM frames mirrored.
  - Escapes, reinstatement and native-depth overflow use an `abandon_to_driver` stub. It restores the
    driver's saved SP, FP and callee-saved registers. Only Cranelift frames lie in between, because helpers
    return a status before abandoning. The driver then continues from VM frames through resume entries
    (Guile's vRA/mRA [cranelift-gc §3.4]).
  - The watermark trampoline then also needs the native return address, which is why S1 is the default.
- **The GC contract is identical under S1 and S2.** The choice is kill criterion K5.
- **Multi-shot `call/cc`:** chunks are immutable and thaw copies. **`dynamic-wind`:** travel through stub
  frames is unchanged, since stubs are ordinary frames whose `ret` is a runtime trampoline. **Delimited:**
  a view of the chain with a floor, appended eagerly first [cont-repr §6 Stage 4].
- **The control-flow matrix (64 rows) is the gate** for Stages 4 and 11.
- **Cranelift `stack_switch` is not used:** it is x64-only and one-shot (pitfall 27).

---

## 11. Pluggability contract

### 11.1 What is pluggable and what is not

- **Pluggable:** collectors, meaning plans that sit behind one fixed object model and one fixed set of fast
  paths described *as data*. This is Whippet's `gc-attrs.h` model [jit-ready §4].
- **Selection is static.** A cargo feature picks the `Plan` type. Within a plan, per-heap *policies* change
  slow paths only.
- **No fast path ever goes through `dyn`.** The allocation and barrier fast paths are byte-identical across
  policies. A non-generational policy simply never arms a log bit, so the barrier falls through.

### 11.2 Rust surface

```rust
/// The JIT ABI prefix of every mutator. Field offsets are part of the ABI and asserted in tests.
#[repr(C)]
pub struct MutatorAbi {
    pub ap: Cell<usize>,                 // 0x00
    pub alloc_limit: Cell<usize>,        // 0x08
    pub meta_bias: usize,                // 0x10  constant per heap
    pub remset_cur: Cell<*mut usize>,    // 0x18
    pub remset_soft: Cell<*mut usize>,   // 0x20
    pub event: AtomicU32,                // 0x28
    pub ticks: Cell<i32>,                // 0x2C
    pub reg_top: Cell<*mut TaggedValue>, // 0x30  current green thread's fp (published at safepoints)
    pub reg_limit: Cell<*mut TaggedValue>,// 0x38 0 when an event is posted
    pub current_thread: Cell<*mut GreenThread>, // 0x40
    pub status: Cell<u32>,               // 0x48  escape/OOM status for helpers
    pub barrier_mode: u8,                // 0x4C  read by slow paths only (0 gen, 1 incr-update, 2 SATB)
}

#[repr(C)] #[derive(Clone, Copy)]
pub struct GcAttrs {                     // per heap; the JIT reads it at compile time
    pub alloc: AllocKind,                // BumpPointer { ap_off: 0x00, limit_off: 0x08, granule: 16, max_inline: 256 }
    pub barrier: BarrierKind,            // None | FieldLogGranuleByte { bias_off, shift: 4, log_bit0: 6,
                                         //   cur_off, soft_off, value_filter_bit: Some(2) } | Call
    pub poll: PollKind,                  // LimitFold { event_off, reg_limit_off } | Tick { ticks_off, .. }
    pub can_move: bool, pub can_pin: bool,
    pub initializing_stores_need_barrier: bool,   // false for every shipped plan
}

/// Implemented by each collector. Exactly one is compiled in (cargo feature).
pub unsafe trait Plan: Sized + 'static {
    type Policy: Copy;                                   // e.g. GenImmixPolicy
    type MutatorExt: Default;                            // plan-private, after MutatorAbi
    fn attrs(policy: &Self::Policy) -> GcAttrs;
    fn refill(m: &mut Mutator<Self>, size: usize, kind: AllocKind) -> Result<(usize, usize), Oom>; // never collects
    fn alloc_old(m: &mut Mutator<Self>, size: usize, pinned: bool) -> Result<NonNull<u64>, Oom>;   // remember-whole
    fn remset_overflow(m: &mut Mutator<Self>);           // flags an event, grows commit; never collects
    fn pin(h: &HeapCore<Self>, v: TaggedValue) -> bool;
    fn collect(h: &mut HeapCore<Self>, roots: &mut dyn RootProvider, req: GcRequest) -> GcOutcome;
}

/// Implemented by Patina (VM, tree-walker, interpreter API). Slot-based: every visit can rewrite.
pub trait SlotVisitor {
    fn slot(&mut self, s: &mut TaggedValue);             // updatable edge
    fn pinned(&mut self, v: TaggedValue);                // non-updatable edge: target is pinned for this GC
    fn range(&mut self, s: &mut [TaggedValue]);
    fn frame_stack(&mut self, base: *mut u64, top: *mut u64, watermark: *mut u64); // descriptor-driven walk
}
pub trait RootProvider { fn roots(&mut self, kind: RootPass, v: &mut dyn SlotVisitor); }
pub trait HostPayload: 'static {
    fn trace(&mut self, v: &mut dyn SlotVisitor);        // may report pinned edges (tree-walker)
    fn finalize(&mut self) {}                            // no Scheme, no allocation (R4)
    fn external_bytes(&self) -> usize { 0 }              // feeds the trigger
}
```

`RootProvider` is called with `RootPass::{Minor, Major}`. The `dyn` there is on the GC path, not a fast
path, and it keeps root registration open for debugger hooks and green-thread schedulers
[tree-walker §6].

### 11.3 Plans shipped

| Plan / policy | Purpose | `GcAttrs` |
|---|---|---|
| `GenImmix` / `Generational` | production, VM heaps (Stage 9 on) | FieldLog, can_move |
| `GenImmix` / `NonMovingFullHeap` | VM default until Stage 9; tree-walker heaps forever; the generational kill-switch | barrier `None` for the JIT (nothing is armed); can_move = false |
| `GenImmix` / `Torture { evac_all, poison, minor_every_safepoint, major_every: u32 }` | differential and zeal lanes | FieldLog, can_move |
| `Null` (feature `gc-plan-null`) | Epsilon: allocate only; the "off" lane and the allocation speed-of-light | None |

Today's mark-sweep cannot run on the new object model. Its oracle role passes to `NonMovingFullHeap`, which is
the simplest correct collector on the new representation.

### 11.4 How a future collector plugs in, and what is excluded

- **Incremental marking.**
  - It sets `barrier_mode` to incremental-update, which uses the same armed bits: armed fields on marked
    objects log, and remark rescans the log.
  - SATB instead needs `value_filter_bit: None`. The JIT reads the attrs, so switching mode invalidates
    compiled code once.
  - Stacks are darkened on resume (OCaml fibers [ocaml-gambit §0 item 4]).
- **Parallel marking or evacuation.**
  - MARK becomes `fetch_or` under the `threaded` feature.
  - Forwarding installs `FWD_MARKER` by CAS.
  - Mark segments become stealable. Whippet's live evacuation race is the warning here [whippet §1.4].
- **MMTk-backed plan.** Possible only if MMTk gains multiple instances per process and an object model
  that keeps pairs headerless, for example through side-metadata type bits. Today it fails both
  [immix-mmtk §10]. Its `BumpPointer{cursor, limit}` maps onto `ap`/`alloc_limit`. Its object unlog barrier
  would be `BarrierKind::ObjectUnlog`, a new attr the JIT must learn.
- **Fallback barrier.** `BarrierKind::Call` lets any plan ship before the JIT learns its barrier. It is a
  leaf call per store, acceptable because leaf calls are not safepoints.

**Excluded, explicitly:**
- load or read barriers of any kind (pitfall 11);
- conservative scanning of native or VM stacks (#423);
- collection inside allocation or barrier slow paths;
- GC patching of machine code;
- Cranelift user stack maps (§8.2);
- concurrent relocation;
- process-global GC state (pitfall 23).

---

## 12. Threading readiness

**The split** [threads-rec §1.5, §2]:
- **`Mutator`** is the carrier: the JIT's `x21`, holding `MutatorAbi` plus the allocation buffer,
  remembered-set buffer, remember-whole list, root stack, safepoint state and `no_gc_depth`.
- **`GreenThread`** holds the register stack and its watermark, the dynamic environment (parameterization,
  current ports, handler stack), the winds and prompts, and the re-entry state.
- On a switch the mutator copies the thread's `reg_top`/`reg_limit` into the ABI. The switch is O(1), because
  frames stay in the thread's own stack.

**N-ready now, at zero single-thread cost:**
- per-mutator allocation buffers, remembered sets and remember-whole lists;
- the safepoint protocol written as request → acknowledge (at a poll, or immediately when in a safe region)
  → collect → release. With one mutator the acknowledgement is immediate;
- `enter_safe_region`/`leave_safe_region` around blocking I/O, no-ops today;
- the one event word, with the owner-only `reg_limit` reset protocol;
- metadata read-modify-writes by mutators (barrier disarm, `pin`, HASH) through one `MetaByte` accessor:
  plain today, `fetch_and`/`fetch_or` under `threaded`;
- slot access through the funnel, interning through one function;
- no runtime state in `thread_local!` (the current ports move);
- code installation through one function;
- a **two-mutator test mode** on one OS thread.

**Deferred:** OS-thread carriers, publication fences (in the funnel and after JIT allocation groups, behind
`threaded`), parallel marking, atomic forwarding, and nursery chunk handout under a lock (uncontended today).

**Open owner question** (DIGEST decision 7): is shared-memory parallelism a goal? My default is no for the
SRFI 18 release. Nothing above changes if the answer becomes yes.

---

## 13. Heap sizing, pacing and observability

**Triggers, all in bytes.**
- **Minor:** `nursery_used ≥ nursery_target`, or the remembered set past its soft limit.
- **Major:** `mature_bytes + los_bytes + external ≥ mature_limit`. Also forced by `(gc)`, by `EMFILE`, by
  descriptor pressure (open fds > 75% of `RLIMIT_NOFILE`), and by code-release pressure (≥ 64 minors with
  descriptors awaiting release).
- **`external`** is the sum of `HostPayload::external_bytes`. Port buffers count 8 KiB each, plus JIT code
  bytes. The continuation byte blindness disappears because chunks are heap objects [cont-repr §0.1].

**Nursery sizing.**
- Initial target 4 MiB.
- After each minor, estimate the next pause as `promoted_bytes / copy_rate`. Halve the target if it exceeds
  1 ms; double it, up to 64 MiB, if survival is falling and pauses are under 0.25 ms.
- In-place mode is entered above 30% survival and left below 15%.
- At JIT allocation rates of 0.3–4 GB/s [demographics §4.1], 4 MiB means a minor every 1–13 ms.

**Mature sizing, MemBalancer style** [DIGEST §3.12]:

```
L = live bytes after the last major (marked + LOS live + external)
g = promotion rate (bytes/s, smoothed), s = major marking speed (bytes/s, smoothed)
E = sqrt(c · L · g / s)                       (c = 1.0 default; owner decision 15)
mature_limit = L + clamp(E, max(8 MiB, 0.25·L), 3·L)
```

- MemBalancer reports 16% less memory at constant GC time, or 30% less GC time
  [Kirisame et al. OOPSLA'22, DIGEST §3.12].
- The clamp guards against its "hyperactive squirrel" failure.
- **Free reserve:** after each major, keep at least `nursery_target + max(2.5% of heap, 4 blocks)` of free
  blocks. This is the evacuation reserve and Wingo's livelock fix (pitfall 15).
- **Hard limit:** `max_heap`, giving a Scheme out-of-memory error (§5.3).
- **Decommit:** §4.4.

**Observability.**
- **Per-collection record:** kind, reason, policy; pause µs by phase (handshake, roots, trace, weak,
  epilogue, metadata-clear); bytes allocated, promoted, copied, marked, freed, decommitted; remembered-set
  entries; survival; in-place flag; descriptors pending release.
- **Outputs:** `PATINA_GC_LOG=path` (CSV per collection) and `PATINA_GC_TRACE=1` (JSON phase timestamps),
  replacing the never-read `last_pause_micros`; MMU over 1/10/100 ms windows and p50/p95/p99/max pause from
  the log (Larceny `gc_mmu_log.c` style); a bundled `(gc-statistics)` extension primitive.
- **Debug counters:** deferred polls in a `NoGcRegion`, overdraft bytes, pinned objects, in-place-promoted
  blocks, hash extensions.

---

## 14. Testing and verification

| Instrument | What it catches | Lane |
|---|---|---|
| `Torture { evac_all, poison, minor_every_safepoint, major_every: 1000 }` | lost roots, unupdated slots, stale `ap`/`fp` caches. Every movable object moves at every major. From-space blocks are filled with `GC_POISON` and, in debug builds, mapped `PROT_NONE` until the next GC. Dead frame slots are poisoned | `run_gc_differential.sh` mode `torture`, both backends, debug + release |
| Heap verifier (`PATINA_GC_VERIFY=1`) | before and after each GC: every heap-bit word points at a START granule in a live block whose kind matches the tag; header types valid; **remembered-set completeness** (every mature slot holding a nursery reference is either unarmed and in the set, or its holder is on the remember-whole list); no log bit armed in the nursery | debug lane, torture lane |
| Null plan | GC-off reference output | differential mode `off` |
| `NonMovingFullHeap` | simplest-collector oracle against `Generational` | differential mode `nonmoving` |
| ABI tests | `MutatorAbi` offsets, `GcAttrs` contents, layout offsets match the generated constants | `cargo test -p patina-gc` |
| Miri | the collector crate's unit tests over a `Vec`-backed `Reservation` (Miri cannot `mmap`); tagged addresses use exposed provenance | CI job on `patina-gc` |
| Two-mutator mode | buffer retire and refill, per-mutator remembered sets and acknowledgements | `cargo test --features two-mutator-test` |
| Control-flow matrix (64 rows) + `escape_from_primitive.rs` | continuation, wind and frame changes | every stage, both backends |
| Hygiene matrix (139 shapes) | identifier representation (Stage 6) | every stage |
| `run_suite_oracles.sh` + `DIVERGENCES.tsv` | semantic drift (flonum `eq?`, port `eq?`, rebinding) | every stage |
| GC benchmark set (Stage 0) | throughput, pause, MMU, RSS | interleaved A/B per stage |

**The GC benchmark set** is the demographics set [demographics §2]:
- Larceny-derived programs run from the local Larceny checkout (LGPL, so not vendored, the same pattern as
  `run_larceny_tests.sh`);
- in-repo originals: `deeprec`, `libload`, `eqtable`, `samedepth1000`, `escape1000`, `pingpong1000`,
  `ctakdeep`, `abort100` and `portchurn` (the 100 k open test).

Metrics: wall time, GC time, pause p50/p95/max, MMU(1/10/100 ms), max RSS, allocation and promotion bytes.

**Method:** interleaved main/branch/main runs, ≥ 5 rounds, medians, with a geomean and a per-workload
table. That is the project rule.

**Each stage proves itself** by:
- (a) all lanes green on both backends;
- (b) differential lanes byte-identical across `off`, `default`, `stress`, `torture` and (from Stage 3)
  `nonmoving`;
- (c) its stated A/B numbers in §15;
- (d) the verifier clean in debug.

---

## 15. Migration plan

**Rules for every stage.**
- Issue first: each stage is a GitHub issue with sub-issues, and the PRD gets one line per stage.
- New markdown files only with owner approval. Otherwise this design updates `docs/GC_DESIGN.md` and
  supersedes `PRD/future/GC_STAGE5_PRD.md`.
- Gates:
  - `run_chibi_tests.sh` and `run_chibi_tests_tree_walker.sh` green;
  - `cargo test --all --lib --tests`;
  - control-flow matrix 64/64 on both backends;
  - hygiene matrix scores unchanged;
  - `run_gc_differential.sh` (release and debug-poison);
  - `run_suite_oracles.sh` with any new `DIVERGENCES.tsv` row approved;
  - `patina-compat check-smoke`;
  - interleaved A/B with no geomean regression > 1% unless the stage declares a budget.

| # | Stage | Scope (crates) | Acceptance and measurements | Effort (eng-weeks) | Value on its own |
|---|---|---|---|---|---|
| 0 | **Measure** | scripts, `core:heap/gc.rs` (pause log), bench programs | baseline table; zero overhead when off | 1–2 | every later claim is measurable |
| 1 | **Tag ABI v2 + self-tagged flonums** | `core:tagged_value.rs`, `core:numeric.rs`, `core:heap`, writer, primitives (numeric), VM inline ops, tree-walker numeric paths | fibfp/mbrot/nucleic ≥ 1.3× faster; flonum allocations −85% on them; others ±1% | 2–3 | float code; frozen tag ABI |
| 2 | **`Mutator` + `Cx` + store funnel** | new `core:mutator.rs`; `prim:registry.rs`; codemod over `patina-primitives` (shimmed over `RefCell`), then `patina-frontend`/`patina-macros`; remove `vector_slice_mut`; GC accounting and `event` off `Heap` | ±1%; one `RefCell` borrow per primitive call | 4–6 | JIT ABI object exists; threads-rec items 1 and 7 |
| 3 | **Block heap + layouts + non-moving mark-region**, as a strangler: 3a pairs, 3b cells / flonum boxes / bignums, 3c vectors, 3d strings / bytevectors, 3e records + RTDs (canonical identity), 3f symbols + interner, 3g promises / values / misc | new crate `patina-gc` (reservation, blocks, metadata, bump, mark-region, lazy sweep, byte trigger, decommit); `define_layouts!` in core; the collector marks arenas and blocks in one trace during the transition | per sub-stage: lanes green. End: hypothetical-layout bytes ≈ 0.5× today's [demographics §4.4]; queue3/gcold max pause ≤ ½; post-load sweep gone; RSS after `libload` ≤ 0.4× | 12–16 | memory halved, high-water sweep gone, RSS returned, no `RefCell` on hot accessors |
| 4 | **Code descriptors, entry blocks, closures, register stack, frames, continuations A, traced code liveness** | `vm:types`, `vm:runtime/{execution_state,control,vm_state}`, compiler (maps at safepoints only, result-in-register delivery); tree-walker procedures as foreign closures | matrix 64/64; capture at depth 1000 ≥ 3× cheaper and byte-accounted; fib/tak call path faster (no `Rc` operations); `finished_forms_release_code` green | 8–12 | weak tables deleted; call path; the JIT frame format exists |
| 5 | **Global cells (variant R) + namespaces + heap teardown + `Owned` handles** | `core:environment.rs` → namespace; `core:library.rs`; VM `LoadGlobal`/`StoreGlobal`; tree-walker global reads; interpreter API | fib/tak `LoadGlobal` −; defects 1 and 2 fixed; 318-heap test processes free their reservations | 5–7 | one-load globals (two under R); leak fixed |
| 6 | **Drop-free object model**: host payloads, ports R1–R6, identifiers as ids + inline provenance (with the front-end), unread provenance stores deleted | `core:heap`, `prim:io`, `core:vfs`, `fe:desugarer`, `patina-macros` | libload malloc peak −300 MiB; 100 k opens succeed under `ulimit -n 1024`; port `eq?` fixed | 6–9 | memory; R1–R6; nursery-eligible syntax |
| 7 | **Rooted boundaries** (loads collect between forms) | `rt:library_loader.rs`, `vm:control.rs` (`across_reentry`), desugarer entry points | `(nieper rbtree)` peak ≤ 120 MiB; `NoGcRegion` only at point C and in the tree-walker | 3–4 | bounded load memory |
| 8 | **JIT ABI spike** (branch, not merged) | Cranelift prototype of about 20 opcodes over `MutatorAbi`/`GcAttrs` | answers K3 (barrier variants, `tbz` selection), K5 (S1 vs S2), allocation grouping | 3–4 | de-risks Stages 9–11 and the JIT |
| 9 | **Generational:** copying nursery, armed field-log barrier, remember-whole, hash extension, in-place promotion, watermark return barrier | `patina-gc` (minor), VM return path, funnel arming; tree-walker heaps stay `NonMovingFullHeap` | **M5** on the GC set: generational vs barrier-on/minors-off vs non-moving (K2); M2 interpreter barrier tax ≤ 1%; deeprec minors scan only new frames | 10–14 | allocation-heavy throughput, lower mean pauses |
| 10 | **Opportunistic mature defrag** | `patina-gc` | fragmentation livelock test passes; ≤ 3% throughput cost | 3–5 | no fragmentation failure mode |
| 11 | **C′ continuations + persistent dynamic state + deep-bound `parameterize`** | VM control, core continuation types, `lib/scheme/base/parameters.scm` | matrix 64/64; capture at depth 1000 O(1) (≤ 1 µs); ctakdeep ≤ 2× ctak | 8–12 | O(1) capture; SRFI 18 prerequisite |

**Totals:** about 65–94 engineer-weeks, roughly 15–22 months for one engineer. Stages 5 and 6 can overlap
with 3 and 4 in a second worktree. Stage 9 depends on 2–7 (every holder updatable, Drop-free nursery, no
raw-bits persistent maps). Stage 11 depends on 4 and 9.

### The first three PRs, concretely

**PR 1 — "GC benchmark set, pause/MMU log, interleaved A/B runner"** (issue: "no GC or pause benchmark
exists").
- Adds `scripts/run_gc_bench.sh`, which runs the Larceny GC and R7RS subset from
  `~/Project/reference/larceny` and skips it loudly when absent. Also adds
  `crates/patina-tests/bench_programs/gc/*.scm` for the in-repo originals and `scripts/gc_ab.py`
  (interleaved main/branch/main, medians, MMU from the log).
- Adds `PATINA_GC_LOG` to `core:heap/gc.rs` (CSV: seq, kind, reason, pause, mark, sweep, live slots,
  freed) and exposes `last_pause_micros`.
- The same PR series files the DIGEST §1.11 defects as issues: embedder use-after-free, interpreter drop
  leak, port `eq?`, `EMFILE`, rebinding divergence (`DIVERGENCES.tsv` row), and the docs drift.
- **Acceptance:** differential lanes byte-identical; zero cost with the log off (A/B ±0.5%); a baseline
  table attached to the tracking issue.
- About 1 week.

**PR 2 — "Tag ABI v2 and self-tagged flonums"** (issue: "boxed flonums are 19.8% of allocations").
- `core:tagged_value.rs`:
  - new tags (§2.1); `VmClosure` gets tag `101`; chars move to the `0x41` immediate kind;
  - strings move under tag `110`, using a temporary arena-selector bit (bit 40) inside index-based
    references until Stage 3d;
  - `try_flonum_imm(f64) -> Option<TV>` and `flonum_imm_value`.
- `core:heap` `make_real`/`get_real` become immediate-first, and the canonical boxes are allocated at heap
  creation.
- Numeric tower dispatch, `values_eqv`, `equal-hash`, `datum_writer.rs`, the VM `Add`/`Sub`/`Mul`/`Lt`
  float paths, and the tree-walker numeric paths are updated.
- **Acceptance:** all gates; fibfp/mbrot/nucleic ≥ 1.3× (interleaved); the immediate share of float results
  ≥ 85% on those three (K1); non-float geomean ±1%; `(eq? 1.5 1.5)` change checked against the oracles and
  recorded if they disagree.
- 2–3 weeks.

**PR 3 — "`Mutator` context: GC accounting and the event word move off `Heap`; primitives take
`&mut Cx`"** (issue: "the JIT and SRFI 18 need a per-carrier context").
- Adds `core:mutator.rs`: `#[repr(C)] MutatorAbi` with today's fields mapped (`ap`/`alloc_limit` reserved
  and unused until Stage 3), plus `event`, `ticks`, `no_gc_depth` and the root stack.
- `allocs_since_gc`, `gc_threshold`, `gc_pending` and `gc_defer_depth` move from `Heap`
  (`core:heap/mod.rs:379-417`).
- The VM safepoint reads `event` (`vm:runtime/vm_state.rs:1195-1212` [V]).
- `HeapHandler = fn(&mut Cx, &[TV])` with a `cx.heap()` shim over the `RefCell`; the codemod covers
  `patina-primitives` only. `vector_slice_mut` is removed (`vector_set` funnel).
- **Acceptance:** all gates; A/B ±1%; the ABI offset test passes.
- 3–4 weeks.

---

## 16. Risks, mitigations and kill criteria

| # | Risk | Mitigation | Kill criterion (measured, interleaved A/B on the GC set + repo bench) → change of course |
|---|---|---|---|
| K1 | Self-tagging covers too few floats, or taxes non-float code | Canonical immortal zero/inf/NaN boxes; JIT constant folding | Immediate share < 70% of float results on fibfp/mbrot/nucleic, **or** non-float geomean > +1% → revert to 16 B boxed flonums in the nursery and keep tags `010`/`011` reserved |
| K2 | Generational loses (Wingo's nboyer result; queue3/deeprec) | In-place promotion above 30% survival; watermark; switchable policy | M5: `Generational` not ≤ 0.97× `NonMovingFullHeap` geomean wall time **and** not ≤ 0.5× p95 pause → ship `NonMovingFullHeap` (JIT barrier `None`), keep generational as an opt-in policy |
| K3 | Barrier cost | Static elision; tbz filter; inline slow path | M2 interpreter tax (all objects armed) > 1%, or M3 JIT field-log > 3% over `None` on vecsort/hashtab → evaluate `BarrierKind::Card` with value-only spaces (barrier-res runner-up); the attrs make that a one-module switch |
| K4 | Strangler mixes two heaps badly | One combined trace; per-kind sub-stages | Any sub-stage that cannot keep the differential lanes byte-identical within two attempts, or mixed-trace pauses > +10% → flag-day conversion of the remaining kinds behind a cargo feature |
| K5 | S1 trampolined returns too slow | S2 with `abandon_to_driver` | Spike: S1 > 8% slower than S2 on fib/tak/nboyer → S2 baseline; GC contract unchanged |
| K6 | VA reservations fail on CI (Linux overcommit, map count) | RW + `MAP_NORESERVE`, one VMA per heap | Reservation failures in CI → 4 GiB default with chained reservations and a per-chunk metadata bias (+1 load in the barrier) |
| K7 | Pinning and in-place promotion fragment the mature space | Defrag (Stage 10); counters | > 20% of minors promote in place on workloads with < 15% survival → revisit thresholds or pin sources |
| K8 | Tier-2 loses from "no values in registers across calls" | Raw window slots for unboxed values | Spike shows > 15% loss against a stack-map prototype on call-heavy float code → reconsider native maps *only* for frames proven not to capture (would need a walker) |
| K9 | Schedule overrun | Stages 0–7 are valuable alone | Stage 3 at 2× its estimate → stop the collector at `NonMovingFullHeap` (Candidate A); the ABI still serves the JIT |
| K10 | 6.25% metadata overhead | Lazy commit; nursery metadata `memset` | Metadata-clear > 5% of major pause, or RSS > hypothetical × 1.15 → move LOG bits to a dense bitmap (1.56%); the barrier gains 1 instruction |
| — | Hidden references (Julia's lesson, pitfall 24) | The §8.6 inventory is the checklist; torture + poison | — |
| — | Unverified Cranelift details: pinned `x21` composing with `CallConv::Tail` on aarch64; `tbz` selection | Spike (Stage 8) | If the pinned register does not compose, pass the `Mutator` as the first fragment argument (Wasmtime style); there is no other impact |

---

## 17. Owner decisions (DIGEST §6), with my defaults

| # | Decision | Default proposed | Consequence of the alternative |
|---|---|---|---|
| 1 | Tree-walker role | **Settled:** kept, may lag, `NonMovingFullHeap`, pinning roots, host payloads | Heap tree-walker frames would cost 6–12 weeks and become the dominant barrier site (12.48 M `define`s on nboyer [barrier-res §2.3]) |
| 2 | Rebinding semantics | Variant **R** in Stage 5 (today's semantics, two loads), then **C** (chibi/Chez/Racket; one load; six pinned tests flip, `DIVERGENCES` row removed) | Staying on R costs the JIT one dependent load per global and keeps shadow-bit machinery |
| 3 | When `define` rebinds; mid-program imports | Expansion time, as all oracles do (p1b, p8) under C | Following today's behavior keeps Patina the outlier [global-cells §2] |
| 4 | Value encoding | **§2: 61-bit fixnums, self-tagged flonums, raw addresses, heap bit 2** | NaN-boxing: +1 instruction per heap load and narrower fixnums. Offsets: +1 add per access, no gain at 8 B slots |
| 5 | Strings and bignums | UTF-32 inline now (O(1) `string-set!`); bignums as inline limbs | UTF-8 + index: smaller strings, rewrite of 252 heap mentions in `strings.rs`/`characters.rs`; `num-bigint` payloads would force foreign objects (old-only) |
| 6 | Pauses vs throughput | **Settled:** throughput first, bounded STW. Targets: minor ≤ 1 ms p95, major ≤ 10 ms per 50 MB live | A latency target would need incremental marking (`barrier_mode` reserved) |
| 7 | Shared-memory parallelism | Not a goal for SRFI 18 v1; M:1 green threads with N-ready interfaces (§12) | Yes → M:N program later (4–9 months [threads-rec §4]); this design is unchanged |
| 8 | JIT frame model and tiers | S1 baseline, S2 measured; tier 2 without native stack maps; tier 3 excluded | Native stack maps need a frame walker plus deopt for `call/cc` |
| 9 | Async interrupts in JIT code | **Yes**: free at calls, 2 instructions at back-edges | No → drop the back-edge poll (about 0.3–1.9% saved in tight loops [cranelift-gc §4]) |
| 10 | Identity hash | Side-metadata HASH state + extension word on move (§6.9) | Pin-on-hash fragments the nursery under eqtable (500 K hashed pairs); native GC-rehashed tables would mean rewriting SRFI 69/125 in Rust |
| 11 | Weakness scope | SRFI 124 now; SRFI 254 guardians and transport cells after Stage 9 (one fixpoint); strong symbol table | A weak symbol table adds an epilogue step; weak SRFI 125 tables add a GC-processed table kind |
| 12 | Port finalization | Yes, R1–R6 (§6.6); GC timing observable; lanes exclude it | No → descriptor exhaustion stays (fails at 1,021 opens) |
| 13 | Embedding API | `Owned` + scoped `with`; mandatory teardown; many heaps per process | Raw `TaggedValue` returns stay use-after-free under any moving collector |
| 14 | Dependencies (MMTk) | **No MMTk**; in-tree `patina-gc`; MMTk-shaped seams (§11.4) | MMTk: one instance per process vs 318 heaps; headered pairs (+50%); worker threads vs determinism [immix-mmtk §10] |
| 15 | Footprint | MemBalancer `c = 1.0`, decommit on, reservation 32 GiB, `max_heap` 75% of RAM | Smaller `c` → less memory, more GC time |
| 16 | Front-end sequencing | Identifiers-as-ids and inline provenance in Stage 6, **before** Stage 9; lazy scope propagation independent | Without them the nursery cannot hold syntax (Drop payloads), and libload keeps 316 MiB of provenance |
| 17 | syntax-case timing | After Stage 7; brings `ExpansionContext` (literal pool, epoch memos) that retires the last `NoGcRegion` at point C | Earlier: expansion needs rooting before the funnel exists |
| 18 | Continuation end state | C′ (Stage 11) | Stop at A: O(depth) capture remains (24 µs at depth 1000 today; about 5× less under A) |
| 19 | Divergences | Record flonum `eq?` (if the oracles differ), port `eq?` fix, rebinding (C) in `DIVERGENCES.tsv` | — |
| 20 | JIT code memory | Entry blocks; W^X toggled only in `install_code`; per-descriptor freeing after majors | `cranelift-jit` `JITModule` cannot free individual functions; we drive `cranelift-codegen` into our own code reservation |
| 21 | Budget order | **Representation first** (Stages 0–6), collector algorithms after (9–11) | Collector-first spends effort on an algorithm the representation then invalidates [immix-mmtk §10] |
