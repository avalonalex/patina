# The GC contract seen from a Cranelift JIT, by tier and feature

**Sources and citations.** The authority is `PRD/GC_PRD.md` at branch `gc-prd`, HEAD `f82e8e8`. That commit changes no
Rust source since `28a94f8`, so the source line numbers below are also `main`'s. **§n:L** means section n, line L of
`PRD/GC_PRD.md`. Source paths are crate-relative: `core:` is `crates/patina-core/src/`, `vm:` is
`crates/patina-vm/src/`, `prim:` is `crates/patina-primitives/src/`.

**Clause IDs** come from the sibling file `prd-contract.md` (the PRD's §18.6 additions, also numbered C1–C17 there, are
written "addition C*n*"):
- C1–C19 are the collector's promises.
- V* are obligations on the VM interpreter; J1–J16 are obligations on JIT code.
- R* are obligations on Rust code, and E* on embedders.
- K-1–K-10 are obligations on anyone adding an object kind.
- N* are the N-mutator rules.

K*n* without a hyphen is a PRD kill criterion (§20). The sibling `today.md` describes the contract as built.

**Labels for Cranelift claims.**
- **[cg §x]**: checked in `PRD/study/gc/research/cranelift-gc.md` section x. That study read the source of Cranelift
  0.136.1 / Wasmtime 49.0.1 on 2026-09-30.
- **[rev]**: checked against Cranelift source only by the design review (`PRD/study/gc/design/REVIEW_DISPOSITIONS.md`
  row JIT-9) or cited in DESIGN, and **not** confirmed in `cranelift-gc.md`.
- **[I]**: my inference.
- **gap**: the PRD says nothing on the point.

No Cranelift checkout exists on this machine (`~/Project/reference` has none), so nothing here was re-read upstream.

**Status.** No JIT PRD exists. Until one does, §11.2, §13, §14 and stage 6 are the GC's whole contract with the JIT
(§1:170-172).
- Decisions 8 (frame model), 9 (async interrupts) and 20 (code memory) are **Proposed** (§2:207, :208, :220).
- The JIT-facing part freezes only after stage 6, a 3–5-week spike on an unmerged branch (§19:1875). Stage 6 also
  decides K6 (flonums), K7 (call and poll shape) and K11 (fragments against native call/ret).
- "Tier 2" exists only as one sentence in §11.2:855-857, plus K12 (§20:1951). Everything this file says about an
  optimizing tier is the contract applied to it, marked [I] where the PRD is silent.

---

## 0. Summary

**One rule carries almost everything:** J1 + J4 + J5.
- J1: in every tier, at every non-`Leaf` call, every Scheme value is in a VM register-stack frame, tagged.
- J4: JIT code publishes before a `Transfer` and reloads after it.
- J5: `(code, pc)` is the resume truth, and `ret` is only a cache.

Between suspension points a JIT may do anything, and the collector never sees it. At a suspension point the stack is
exactly what the interpreter would have left. From that:
- the collector needs no stack maps;
- nothing ever unwinds a native frame;
- deoptimization is "pick another resume entry";
- OSR is "enter at a suspension point";
- evacuation (stage 8) is invisible to emitted code.

| # | Tier / feature | Relies on (collector side) | Must obey (JIT side) | Cranelift | Where decided | Verdict |
|---|---|---|---|---|---|---|
| 1 | **Tier 1: baseline fragments** (§3.1) | C1, C2, C19, V4's maps, the shared 40 B frame header | J1–J9, J12, J14; V3, V8 | needs `CallConv::Tail`, `return_call_indirect`, the pinned register; avoids stack maps, frame walker, unwinder | stage 6; K7, K11 | the contract is built for this |
| 2 | **Tier 2: inlining** (§3.2) | C2, J5, §9.7 code liveness | materialize inlined frames at each suspension point; bytecode liveness there; `WATCHED` guards; keep loop polls | none extra (no debug tags) | K12 | fits; cost is materialization |
| 3 | **Tier 2: unboxed floats** (§3.3) | C2 (boxing never collects), immortal canonical boxes | re-tag or box before every suspension point; invariant W | none | K6 (before stage 6), K12 | fits within a block; K12 is the hatch |
| 4 | **Tier 2: register allocation across calls** (§3.4) | J3 (`Leaf` calls keep SSA) | nothing crosses a `Transfer` or a Scheme call | user stack maps would not keep references in registers anyway [cg §2.1] | K11, K12 | impossible across Scheme calls by design |
| 5 | **Tier 2: escape analysis, scalar replacement** (§3.5) | C2, unbarriered initializing stores | materialize once where reachable at a suspension point; barrier after any later suspension | none | — | fits |
| 6 | **Tier 2: allocation sinking and grouping** (§3.6) | C2, C3, `GcAttrs.alloc` | not past a suspension point that sees the object; user-sized `try_alloc` not past a visible effect | none | — | fits |
| 7 | **Inline caches; global-cell guards (`WATCHED`)** (§3.7) | C8 (NMS never moves), C12, C13 step 5 | J6, J10; IC = one traced word; no patching | no `patchable` calls | 4b, C, 6 | fits; one gap on IC barriers |
| 8 | **Tier-up and OSR** (§3.8) | J5, V4 (maps at pc 0 and at every resume pc) | tier-up is a `Transfer` writing `desc.entry` | `return_call_indirect` | 6 | trivial by construction |
| 9 | **Deoptimization and invalidation** (§3.9) | J5, C12 epoch freeing | re-derive `ret`; publish (already owed) | no deopt metadata | 6 | trivial by construction; two gaps |
| 10 | **Native call/ret (S2)** (§3.10) | — (outside the contract) | abandonment, depth cap, watermark rework | SP-reset stub outside Cranelift, or `try_call` + an unstable unwinder [cg §2.5] | K11 (> 8%) | a separate design |
| 11 | **Continuations and green-thread switches** (§3.11) | V11 capture `memcpy`, invariant W, J5 | capture and switch are `Transfer`s; reload `reg_top`/`thread` after | avoid `stack_switch` [cg §2.6] | 4e, 9 | fits; JIT frames are VM frames |
| 12 | **Debugger hooks pin the tier** (§3.12) | invalidation path, `RootSet::register` | J13; V6 | no DWARF, no debug tags | 3 (flag), 6 | fits |
| 13 | **Code memory** (§3.13) | C12, F4, M3, the §17.3 limits | J11; W^X only in `install_code` | avoid `cranelift-jit`'s `JITModule` | decision 20; JIT track | fits; interface frozen at 6 |
| 14 | **Exceptions (`try_call`)** (§3.14) | raise is `Transfer`; `Mutator.status` | J3 status path; raise pcs mapped | `try_call` not needed | — | fits; one gap (Rust panics) |
| 15 | **Multiple carriers** (§3.15) | N1, N4, N6; reserved ABI words | rule 4(f); handshake for invalidation | none | decision 7 (not now) | 2–4 weeks, or 6–10 to retrofit |

Section 4 lists the collector changes the JIT does not see. Section 5 covers what changes if the JIT team wants
something the PRD excludes. Section 6 covers what today's code would demand of the same JIT. Section 7 lists the gaps.

---

## 1. The JIT-facing clauses in one place

**What the JIT relies on (collector → JIT)**

| Clause | What it buys the JIT | PRD |
|---|---|---|
| C1 | Collection happens only at a poll, and only with the driver's `&mut Heap`. No collection sees a half-built frame, so JIT native frames are transient and own no roots | §9:528-532; §12:1025-1027 |
| C2 | Allocation never collects. Allocation sites are not suspension points: no publish and no reload around them, and initializing stores take no barrier. `ap`/`limit` live in SSA | §8:499-503, :508-511; B2 §4:288 |
| C3 | A user-sized `Oom` comes back as a status and is retried once at the call's return pc. The constructors stay `Leaf` | §8:523; §11.2:863 |
| C5 | `(gc)` is a `Transfer` that collects at its call | §9.5:593 |
| C7, C8 | Nothing moves before stage 8. The LOS, NMS, immortal space and pinned blocks never move | §9.4:574-582; §7:466-467 |
| C12, C13 | Code is live while any of its descriptors is marked. Bodies are freed by epoch. Weak inline-cache entries are cleared at epilogue step 5, and code is released at step 8 | §9.7:618-639; §9.9:670, :675 |
| C19 | A non-generational heap reports `BarrierKind::None`, so the emitter writes no barrier | §10:758-760 |
| frame format | One 40 B header for interpreter frames, JIT frames and captured copies. Maps exist at every suspension point, produced by the bytecode compiler, so the JIT emits none of its own | §11.1:807-833 |
| `return_into` | Every write into a suspended frame goes through one choke point (stage 7's watermark) | §11.1:839-843 |
| polls | The poll slow path collects only with `&mut Heap` and `no_gc_depth == 0`, so a poll inside JIT code entered beneath a `Cx` defers by itself; whether a nested driver beneath a `Cx` may enter JIT code at all is unstated (gap 6) | §12:1036-1041 |

**What the JIT must obey (JIT → collector)**

| Clause | Content | PRD |
|---|---|---|
| J1 | Tagged values in VM frames at every non-`Leaf` call, in every tier. No Cranelift user stack maps, native walker, unwinder or `stack_switch`. No derived pointer live across a suspension point | §11.2:851-857 |
| J2 | Fragments with `CallConv::Tail` and `return_call_indirect`. The result is the first `Tail` argument of the caller's resume entry (`x2` / `rdi`) | §11.2:853-855; §11.1:821-823 |
| J3 | Helper classes `Leaf` (optionally `NoAlloc`) and `Transfer`. `ap`/`limit` stay in SSA only across `NoAlloc` | §11.2:859-864; §8:511 |
| J4 | Publish dirty values, `pc` and `ap` before a `Transfer`; reload after it | §11.2:864 |
| J5 | `(code, pc)` is the truth and `ret` a cache, re-derived on reinstatement from `desc.resume[pc]` | §13:1077-1082 |
| J6 | Embed an address only in four cases: immortal; a descriptor of the body's own unit; `WATCHED`-guarded; an NMS object the unit's constants or link table reference. Inline-cache words are traced. Never embed the `Mutator`; never patch code at GC | §11.2:866-872 |
| J7, J8 | The `Mutator` lives in `x21`/`r15`, set by an entry trampoline. `GcAttrs` is read as data at compile time | §14:1217-1261; §11.2:874-877 |
| J9, K-9 | Inline sequences. Every word of an inline-allocated object is written, padding as fixnum 0 | §8:505-512; §10:720-735 |
| J10 | A store into a `WATCHED` cell takes a `Transfer` cold path that deoptimizes first | §10:762-765 |
| J11 | Code memory rules | §9.7:635-639 |
| J13 | Hooks pin the interpreter tier | §11.2:879-883 |
| J14 | Back-edge polls (2 instructions) | §12:1021 |
| J15 | Address-dependency loads, under `threaded` only | §18.2:1664 rule 4(f) |
| J16 | The list of exclusions | §14:1288-1293 |
| V3 | Window initialization at every push | §11.1:829 |
| V8 | The frame-entry poll runs after the header, arguments and window are complete | §12:1020 |
| K-3 | Invariant W | §6:375-380 |
| R13 | The owner-post protocol (the barrier cold block) | §12:997-1003 |
| R14 | A `Leaf` never enters a safe region | §12:1044-1048 |

---

## 2. Cranelift features: needed, optional, avoided

| Feature | Role in Patina | Checked | Notes |
|---|---|---|---|
| `CallConv::Tail`, `return_call`, `return_call_indirect` | **Needed** (J2): every call and return between fragments | [cg §2.4]: caller and callee must share a tail-capable convention; the indirect form takes a native address; Tail is "not ABI-stable" | Rust enters through a generated trampoline because Cranelift never saves the pinned register, which is callee-saved for Rust (§11.2:874-877); [I] Rust also has no `Tail` convention to call one with. Tail calls are not safepoints and nothing is live after one (test `needs_stack_map_and_tail_calls`) [cg §2.1] |
| `enable_pinned_reg`, `get_pinned_reg` / `set_pinned_reg` | **Needed** (J7): the `Mutator` | [cg §2.7]: `x21` on aarch64, `r15` on x64; one register only; Wasmtime itself does not use it | "Cranelift never saves the pinned register, yet it is callee-saved for Rust" (§11.2:874-877) is **[rev]** only (`aarch64/abi.rs:1411-1417`) |
| Results in `x2` (aarch64) / `rdi` (x64) | **Needed** (J2) | **[rev]** (`aarch64/abi.rs:178-189`, Tail arguments start at `x2`) | Consistent with [cg §2.5], which puts `try_call` payloads in `x0`/`x1`. Stage 6 measures the cost (DESIGN.md:1658) |
| `set_cold_block` | **Needed**: allocation refill, barrier cold block, poll cold path | [cg §2.3] (Wasmtime's inline bump uses it) | — |
| `readonly` + `can_move` loads | Optional: hoist `meta_bias` "once per fragment" (§10:721) | [cg §2.3] (Wasmtime marks its heap-base load `readonly` and `can_move` so LICM hoists it; `can_move` is the MemFlags bit that permits hoisting, not "the heap may move") | Valid only because the reservation never moves (§7:443-450) |
| User-defined alias regions | Optional: lets Cranelift forward VM-register loads and stores between suspension points | [cg §2.7] (user-defined since 2026-05, `ir/memflags.rs:25-55`); alias analysis is effective only at `opt_level=speed` [cg §2.1] | A tier 1 at `opt_level=none` loses this forwarding. Patina's own SSA cache of VM registers does not need it [I] |
| `PreserveAll` convention | Candidate for the barrier slow path; the spike measures it against the call-free inline cold block (§10:734-735) | [cg §2.7]: clobbers no registers, register arguments only, no returns, no tail calls | — |
| Single-bit test lowered to `tbz`/`tbnz` | The barrier filter and `LOG` test (§10:725-728) | **[rev]** (`aarch64/lower.isle:3255-3268`) | — |
| **User stack maps** (`declare_*_needs_stack_map`) | **Avoided** (J1; §14:1290) | [cg §2.1]: every non-tail call is a safepoint, including calls known never to collect; trap polls are not expressible; the spiller runs only if a value is declared, so it costs nothing when unused; `JITModule` does not surface maps | §3.4 and §5.2 explain why they would not help |
| `preserve_frame_pointers`, frame walking | **Avoided** (no native walker) | [cg §2.2, §2.7] | Needed only by the excluded designs (§5.2) |
| `try_call`, the unwinder | **Avoided** (§3.14) | [cg §2.5]: all registers clobbered at `try_call` sites; the unwinder is `wasmtime-internal-unwinder`, self-described "INTERNAL" | Only S2's abandonment could use it (§3.10) |
| `stack_switch` | **Avoided** (§14:1291) | [cg §2.6]: experimental, lowered only on x64 (no aarch64 lowering), one-shot | Useless for multi-shot `call/cc`; wrong for green threads on arm64 |
| `patchable` call metadata | **Avoided** (no patching outside `install_code`, §14:1289-1290) | [cg §2.7] | K7's fallback patches Patina's own entry blocks inside `install_code`, not Cranelift calls (§20:1946) |
| Debug tags, `sequence_point` | **Avoided**: deoptimization uses state already published | [cg §2.7] ("the closest Cranelift gets to deoptimization metadata") | Needed only by the excluded deopt-at-capture route (K12, §5.2) |
| Cross-function inlining in Cranelift | Optional for tier 2 | [cg §2.7] (since Wasmtime 36) | Scheme-level inlining happens before CLIF (§3.2) |
| `cranelift-jit`'s `JITModule` | **Avoided** (decision 20) | [cg §2.7]: finalized code is made RX with `region::protect`; no stack maps surfaced [cg §2.1] | "No `MAP_JIT`, frees only whole modules" (§3:272; §2:220; DESIGN.md:2480) is **[rev]**, not in `cranelift-gc.md`. So Patina drives `cranelift_codegen::Context` and installs `MachBuffer` output itself |
| BTI landing pads | "if enabled" (§9.7:638) | **not checked anywhere in the study** | Stage 6 lists "BTI" (DESIGN.md:1658) |

---

## 3. By tier and feature

### 3.1 Tier 1: baseline fragments (S1)

Each bytecode function compiles to Cranelift functions ("fragments") in `CallConv::Tail`, split at each non-tail
Scheme call. Each return point becomes an entry, recorded in `desc.resume[pc]`. Calls and returns are
`return_call_indirect`, so the native stack never holds more than the trampoline, the current fragment and one helper
([cg §6.1] S1; §11.2:853-855).

The PRD's budget for each operation (§1:161-165):

| Operation | Cost |
|---|---|
| allocation | `add; cmp; b.hi` per basic block |
| barrier | 4 instructions for heap values, 1 for immediates, nothing while `BarrierKind::None` |
| call | a tag test and 2 dependent loads (procedure word 0 → descriptor, then `desc.entry`) |
| global | 2 loads under variant R, 1 under variant C |
| poll | free at frame entry, 2 instructions at back-edges |

| | |
|---|---|
| **Relies on** | **C2**: the slow path refills, overdrafts and posts but never collects, so `ap`/`limit` stay in SSA and allocation sites publish nothing (§8:499-503). **C1**: no collection with a half-built frame (§12:1025-1027). **V4**: the bytecode compiler maps the return pc of every call, including every inline primitive opcode whose slow path calls from that pc. It also maps the pc after every instruction that can raise, pc 0, and every `Transfer` helper site, with a dense `pc → u16` index (§11.1:830). The JIT reuses these maps and emits none. **C19/J8**: emit what `GcAttrs.barrier` says: nothing in stages 5–6. **§12:1036-1037**: deferral under `NoGcScope` is decided inside the poll helper, so a fragment entered beneath a `Cx` needs no special poll code (whether one may be entered there at all is gap 6) |
| **Must obey** | **J1/J4**: before every non-`Leaf` call, store each dirty SSA-cached register to its slot, store `meta` (this suspension point's pc) and `ap`, then reload after. **J2**: deliver results as the first `Tail` argument. **V3**: every push writes an immediate into every non-parameter slot (§11.1:829). [I] Fixnum 0 is the all-zero word, so paired zero stores suffice. **V8**: the callee prologue `ldr x9,[x21,#0x30]; add; cmp; b.hi cold` runs after the header, arguments and window are complete (§12:1020); the cold path tells `&stack-exhausted` (within 1 MiB of the cap, §11.1:803-805) from a posted event (`reg_limit == 0`, §12:1026-1027). **Frame header**: `ret`, `link`, `code`, `meta`, `closure` (§11.1:810-817). A tail call copies `link` and `ret` (§11.1:839). `Return` reads `ret` from the frame, never from a register, so stage 7's trampoline swap works (§11.1:836-838). [I] `ret == 0` (an interpreted caller, §11.1:811) routes to the interpreter trampoline. **J6**: constants come by one load through the descriptor in the frame's `code` word (§6:422-427). **J7**: never embed the `Mutator`. **J9/K-9**: write every word of an inline-allocated object, padding as fixnum 0, and apply tags with `add`, never `orr` (§8:507-511; review row JIT-7). **J3**: keep `ap`/`limit` in SSA across `NoAlloc` helpers only; a debug assertion and a call-graph test enforce it (§8:511-512; §16:1347). **J12**: the heap pointer comes from the driver's `&mut Heap` (§11.3:896-898). **J14**: a back-edge poll on self tail calls (§12:1021) |
| **Cranelift** | `CallConv::Tail` and `return_call_indirect` [cg §2.4]; the pinned register [cg §2.7]; `set_cold_block` [cg §2.3]. Alias regions are optional [cg §2.7]. No user stack maps (zero cost when nothing is declared, [cg §2.1]), no frame pointers, no `try_call`, no `stack_switch` |
| **Gates** | The stage-6 spike measures: per-fragment prologue cost; the 2-load call (K7: more than 3% slower than a 1-load entry → entry blocks patched inside `install_code`); the limit-fold poll and `Transfer`-return check; allocation grouping; the inline barrier against a `PreserveAll` stub; the return-barrier trampoline; the cost of `x2` results; S1 against native call/ret (K11); a float microkernel (K6) (DESIGN.md:1658; §19:1875; §20:1946) |
| **Today's code would need** | Section 6 |

### 3.2 Tier 2: inlining

The PRD says tier 2 "publishes **tagged values only**, so the same `(code, pc)` means the same thing whichever tier
wrote the frame" (§11.2:856-857). DESIGN spells out that this includes "materializing inlined frames" (DESIGN.md:776-782).
K12's first response to a tier-2 loss is "more `Leaf` helpers and inlining" (§20:1951).

| | |
|---|---|
| **Relies on** | **C2**: allocation inside an inlined body is not a suspension point. **J5**: a materialized callee frame resumes through its own descriptor's `resume[pc]` or the interpreter trampoline. **§9.7:620-624**: a frame's `code` word keeps the callee's unit alive by ordinary marking |
| **Must obey** | **Materialize at suspension points** [I, from J1 and DESIGN]. At every non-`Leaf` call inside inlined code, write the full chain of VM frames the interpreter would have. Each frame needs a header (`code` = that function's descriptor, `meta` = its pc) and every slot that function's map marks live at that pc. So **tier-2 liveness at a suspension point is bytecode liveness**: dead-code elimination may not drop a value the bytecode map keeps, because an interpreter resuming the frame after capture or deoptimization would read it. **Inlining a body with no suspension point** (accessors, arithmetic, predicates) needs no materialization, and is the cheap, common case [I]. **Keep the callee's descriptor reachable**: put it in the caller unit's constants or link table. Then J6 case (4) allows embedding it, and §9.7 keeps the callee's unit alive while the body exists. Obtain the reference through `CodeStore::escape`, which covers "any other holder" (§9.7:628-631). **Guard**: under C, on a `WATCHED` cell (J10). Under R, `WATCHED` on both record and cell, or the 2-load path (§11.6:976). **Polls**: inlining turns calls into jumps. Every loop it creates must keep a back-edge poll, because polls bound latency only when "every loop passes through a closure call or tail call" (§12:1014-1016) [I] |
| **Cranelift** | Nothing beyond tier 1. Debug tags and `sequence_point` ("preserving virtual frames across an inlining transform", [cg §2.7]) are not needed, because frames are materialized by explicit stores |
| **Gaps** | Gap 1 in §7 (`Tick` accounting differs by tier); gap 8 (bytecode liveness is only implicit in J1) |

### 3.3 Tier 2: unboxed floats (and untagged integers)

| | |
|---|---|
| **Relies on** | **Self-tagged flonums** (§5:340-348): every double with \|d\| in [2⁻²⁵⁵, 2²⁵⁷) is an immediate, so re-tagging an in-band double costs a few instructions (K12 says "re-tagging is 1–6 instructions", §20:1951). **Immortal canonical boxes** for ±0.0, ±∞ and NaN (§5:343-345), embeddable under J6 case (1). **C2**: boxing any other out-of-band double is a `Leaf` allocation (§11.2:863, "flonum boxing") that never collects, so it adds no suspension point |
| **Must obey** | **J1/K-3**: no raw f64 bits in a frame at a suspension point, nor in a continuation (invariant W, §6:375-380). Untagged integers are shifted back to 61-bit fixnums; overflow promotes through the `Leaf` bignum helper (§11.2:863) |
| **Depends on** | **K6** (§20:1945), decided before the stage-6 freeze (§2:198). If self-tagged flonums do not pay, the encoding reverts to 16 B boxes with `010`/`011` reserved. Then every published double is an allocation, tier 2's unboxing gains more inside a block and pays more at every call, and K12 is likelier to fire |
| **Hatch** | **K12**: tier-2 measurements showing more than 15% loss on call-heavy float code → more `Leaf` helpers and inlining first. Native stack maps only through a separately approved deopt-at-capture design that amends §14 (§20:1951; B5 §4:291). §5.1 covers what raw slots would change |
| **Cranelift** | Nothing special (f64 SSA values, bitcasts and rotates) |

### 3.4 Tier 2: register allocation across calls

| | |
|---|---|
| **Inside the contract** | Values live in SSA across **`Leaf`** calls, using ordinary caller- and callee-saved registers (J3; §11.2:863). Nothing lives across a **`Transfer`** (J1/J4). Under S1 nothing lives across a **Scheme call** either: the fragment ends at the call, and the return lands in a new fragment (§11.2:853-855) |
| **What it would take** | Register residency across a Scheme call means native frames holding Scheme values across a suspension point. §14:1291-1292 excludes this. S2 (§3.10) does not provide it, because S2 still publishes at every non-`Leaf` call |
| **Why the exclusion costs little for references** | With Cranelift user stack maps, a declared reference live across a safepoint is spilled to a stack slot at its definition, and every use is rewritten to a reload [cg §2.1]. It "can never stay in a callee-saved register across a call" [cg §2.1, cost model]. And "all non-tail call instructions are considered safepoints", including calls known never to collect [cg §2.1]. So native maps would replace VM-frame stores with native-slot stores for references, and would **add** spills around every `Leaf` call (refill, barrier slow path, bignum promotion) that the contract now keeps spill-free [I]. The only values that could stay in callee-saved registers are non-references (raw doubles, untagged integers). Those then need deoptimization metadata at capture. That is exactly the case K12 measures: call-heavy float code |

### 3.5 Tier 2: escape analysis and scalar replacement

| | |
|---|---|
| **Relies on** | **C2**: removing an allocation never removes a safepoint. **Initializing stores take no barrier** (§8:501-503; `GcAttrs.initializing_stores_need_barrier = false`, §14:1254) |
| **Must obey** [I, from J1, invariant W and §9.8] | **Materialize** a scalar-replaced object at any suspension point where a live slot reaches it: bump-allocate, write every word (K-9), and publish the reference. **Materialize once.** Identity (`eq?`; `identity-hash` sets `HASHED` on an address, §9.8:646-652) means a later suspension point reuses the first copy. **Barrier after a later suspension.** A materialized object may be old after any later suspension point, because a minor promotes survivors in place (§9.2:553-554). §10's static elision covers only "initializing stores into objects allocated with no safepoint since" (§10:745-746). **Opaque uses**: `reference-barrier` is a `Leaf` `NoAlloc` use (§9.5:594). Hashing, `make-ephemeron` and any heap store are escapes. **Under `threaded`**: a store may skip publication only while its holder "has not escaped since allocation" (rule 4(c), §18.2:1664), so the analysis's escape points must match publication's |
| **Cranelift** | None; this happens in Patina's IR before CLIF |

### 3.6 Tier 2: allocation sinking and grouping

| | |
|---|---|
| **Relies on** | **Grouping** is already tier 1's sequence: one `add; cmp; b.hi` per basic block's summed allocation (OCaml's Comballoc, §8:509-510), with offsets from `GcAttrs.alloc` (`max_inline: 256`, §14:1244). **C2** |
| **Must obey** [I] | **Not past a suspension point that sees the object.** Sink past a suspension point only if no live slot references the object there; otherwise materialize, as in §3.5. **User-sized constructors stay put.** `make-vector`, `make-string`, `make-bytevector`, string-port growth and `read-string` call `try_alloc` "before any visible change" and are retried after a collection at the call's return pc (C3; §8:523). The review relies on that order: "`try_alloc` precedes any visible effect, so the retry is idempotent" (REVIEW_DISPOSITIONS.md C2-3). So they may not be sunk past, or grouped with, a visible effect. **Large objects**: over 256 B goes through helpers, over 8 KiB to the LOS (§8:518-519) |
| **Side effect** | Allocation volume changes when collections happen, since triggers are bytes (§15:1297-1300). GC timing is declared observable (decision 12, §2:211). So JIT on and off may flush dead ports and break ephemerons at different moments, which the byte-identical lanes do not observe [I] |
| **Under `threaded`** | With `FenceAtAllocation`, one fence per allocation group (§18.6:1774 lists `dmb ishst` per group as a placement); stage 6 chooses the placement |

### 3.7 Inline caches and global-cell guards (`WATCHED`)

| | |
|---|---|
| **Relies on** | **C8**: NMS objects (descriptors, record types, symbols, cells, binding records) never move (§7:466). But they are **mortal** (M1, SD3) and "a reused address could satisfy a stale type test" (§11.2:870-871), so liveness, not position, is the hazard. **C13 step 5**: weak inline-cache entries are rekeyed or dropped in the epilogue (§9.9:670). **§9.7:628-633**: an IC obtains descriptor references only through `CodeStore::escape`, which sets the unit's `escaped` bit; IC entries are "strong, or weak and cleared at epilogue step 5". **Variant C** gives one-load globals (§11.6:976; §19:1874) |
| **Must obey** | **J6**: an IC is **one traced word** (§11.2:871; rule 6, §18.2:1666), checked by a load, a compare and a branch. It is **never patched code**: patching outside `install_code` is excluded (§14:1289-1290). K7's fallback allows only entry blocks patched inside `install_code`. **Embedding**: only the four J6 cases. A cell behind a re-pointable record (variant R) stays under case (3) (§11.2:869-870; §17.2:1453). **J10**: once code depends on a cell's value, the cell's flags carry `WATCHED`. A store into it takes a `Transfer` cold path that invalidates dependent bodies and deoptimizes the executing fragment before stale code can observe the new value (§10:762-765). In Rust that path is `Cx::store`'s `check_watched` (§10:779). Under R, `WATCHED` goes on both record and cell, or the JIT uses two dependent loads (§11.6:976). Under C, a per-binding `cell.value == expected` guard remains in the interpreter, and JIT sites rely on `WATCHED` (§11.6:978-983) |
| **Cranelift** | No `patchable` calls; ordinary loads |
| **Today** | `LoadGlobal` clones an `Rc<Environment>` through `frame_globals` under a heap borrow (`vm:runtime/vm_state.rs:1357-1365`). A shadowed inline primitive falls back to a by-name lookup (`vm:runtime/control.rs:3467-3489`) |
| **Gaps** | Gap 3 in §7 (barriers on IC words); gap 2 (a runtime change to `GcAttrs`) |

### 3.8 Tier-up and on-stack replacement

| | |
|---|---|
| **Relies on** | **J5**: frames are tier-independent, so a frame published by tier 1 can be resumed by tier 2 at any pc that has a resume entry. **V4**: pc 0 is always a suspension point (§11.1:830), and a self tail call re-enters at pc 0 through the frame-entry check (§12:1021). Codegen emits forward jumps only, so every loop passes through some frame-entry poll at pc 0 (a closure call or tail call) or a `Transfer` return (§12:1014-1016); **OSR at a loop head is therefore "enter the new `desc.entry` at pc 0"** [I]. Mid-body OSR is "enter `desc.resume[pc]`" at any published suspension point |
| **Must obey** | Tier-up is a **`Transfer` helper**: it "writes `desc.entry` and answers the new entry" (§13:1078; §11.2:864). `desc.entry` is a raw descriptor field (§6:422-423), never a barrier target (§6:379-380). A tier-2 body need not provide every resume entry; a missing one falls back to tier 1 or the interpreter (J5). JIT code bytes count as external bytes (§15:1298). Past `PATINA_JIT_CODE_MAX`, tiering stops without error (§17.3:1487) |
| **Cranelift** | `return_call_indirect` to the new entry |
| **Gaps** | Gap 5 in §7 (`desc.entry` ordering across carriers) |

### 3.9 Deoptimization and invalidation

| | |
|---|---|
| **Mechanism** | Invalidation comes from a `WATCHED` store, a tier-down or an attached hook. It "marks dependent bodies invalid, deoptimizes the executing fragment and re-derives the `ret` of every frame of the affected descriptors in every green thread's stack; replaced bodies are freed by epoch" (§13:1079-1081). The executing fragment is inside a `Transfer` (the invalidating store is one), so it has already published; the helper answers `Target(lower-tier entry at this pc)`. **Deoptimization costs nothing beyond publication the contract already requires.** Frames suspended in any stack need only their cached `ret` rewritten |
| **Relies on** | J5; **C12**: bodies are freed "only when the driver sees no JIT activation on the native stack, after the fix-up walk" (§9.7:635-637); V11 (captured copies hold no `ret`); the zeal mode `jit-invalidate` (§9.7:638-639; §14:1273) |
| **Must obey** | The shadowed-inline-primitive deoptimization call is a `Transfer` at a mapped pc (§11.1:830; §11.2:864). An inline primitive's fast path keeps its guard: under R the shadow bitsets, which stay (§11.6:975), or under C the cell guard (§11.6:981-982) |
| **Cranelift** | No deoptimization metadata (debug tags, [cg §2.7]) |
| **Gaps** | Gaps 6 (nested-driver activations) and 7 (the watermark-swapped `ret`) in §7 |

### 3.10 Native call/ret (S2), the K11 alternative

| | |
|---|---|
| **PRD position** | Outside this contract (§11.2:853-855; decision 8, §2:207; §14:1291-1292). If the spike finds fragments more than 8% slower in cycles than a native call/ret estimate on fib/tak/nboyer, "a **separate native call/ret design**" opens, "priced and gated on its own" (K11, §20:1950) |
| **What that design must add** (K11's own list) | Native call/ret only between non-`Leaf` events. Every `Transfer` helper, poll or collection abandons the native stack back to the driver through an SP-reset stub written outside Cranelift, and frames resume through their `(code, pc)` resume entries. A native depth cap that falls back to fragments. The watermark check on the driver's resume path. Gates: the matrix, 10 M-deep recursion, the two-mutator lane, zeal-minor |
| **What changes for the GC** [I] | J1 itself need not change: S2 still publishes before every non-`Leaf` call, because the callee's frame-entry poll can collect. What changes: **(a) Watermark.** A native `ret` returns through the link register, not the frame's `ret`, so the stage-7 trampoline swap does not apply to native returns. K11 already answers this: every `Transfer`, poll or collection abandons the native stack to the driver, so after any minor no native frame survives, and the watermark is checked on the driver's resume path (§20:1950). **(b) Code freeing.** "No JIT activation on the native stack" now holds only after abandonment or return to the driver, so replaced bodies wait longer. **(c) Depth.** 10 M frames at about 48 B per level need about 480 MB of native stack [cg §5], so a cap is mandatory. **(d) Green threads.** Every switch abandons the native stack. **(e) Embedding.** Abandonment must also restore the pinned register the trampoline saved |
| **Cranelift** | The abandonment alternative is `try_call` at the entry trampoline plus the unwinder [cg §6.1 S2], which clobbers every register at `try_call` sites and relies on `wasmtime-internal-unwinder` (unstable, "INTERNAL") [cg §2.5]. K11 instead names an SP-reset stub outside Cranelift; the jitfirst proposal's `abandon_to_driver` restores the driver's SP, FP and callee-saved registers (`proposal-jitfirst.md:1022-1027`) |

### 3.11 Continuations and green-thread switches with JIT frames

| | |
|---|---|
| **Relies on** | **V11 / design A** (§13:1059-1064): capture `memcpy`s `[base, top]` into a `T_CONT` allocated through `try_alloc`, clears dead slots through the maps plus the `call/cc` `dst` hole, zeroes every `ret` and clears `WM` flags. The copy is value-only (invariant W), so the core traces it word by word, and the frame and map formats stay private to the VM. **J5**: reinstatement re-derives `ret` from `desc.resume[pc]`, so "a multi-shot continuation never jumps into discarded code" (§13:1081-1082). **§9.7:621-622**: frame `code` words in captured copies keep their units alive by marking. Multi-shot copies out on every invoke; delimited capture relocates by byte offsets (§13:1084-1086). **Green threads** (§18.1:1613-1619): the `Mutator` (carrier) owns `ap`/`limit` and the store buffer; a `GreenThread` owns its register stack and `ThreadGcState`. A switch saves `reg_top` and calls `set_limit` (§12:999-1001) |
| **Must obey** | Capture, continuation invocation, wind operations and abort are `Transfer`s (§11.2:864). A switch is a `Transfer` answering another thread's resume entry. After any `Transfer`, reload `reg_top` (0x28) and `thread` (0x48), never cache them across one [I, from J4]. Continuations "refer to no carrier" (rule 6, §18.2:1666). Preemption is deferred while `reentry_depth > 0` (§12:1042) |
| **Cranelift** | Avoid `stack_switch` [cg §2.6]: x64-only, one-shot, and in Wasmtime a 2 MiB stack per continuation that is never freed. No walker and no unwinder |
| **Later** | C′ (frozen chunks thawed through an underflow stub frame) reuses this frame format and watermark (§13:1088-1090). It comes only after the baseline JIT and only if K15 allows |
| **Today** | `capture_full` clones the whole register `Vec` and frame `Vec` (`vm:runtime/execution_state.rs:239-261`) into weak side tables. Those are sound only because "every store touch (capture, invoke) is confined to one instruction dispatch and nested loops defer collection" (`vm:runtime/vm_state/gc_roots.rs:21-24`), a rule any JIT capture helper would inherit |

### 3.12 Debugger hooks pin the tier

| | |
|---|---|
| **Mechanism** | Attaching a `StepTracer`, breakpoint, watchpoint or `DebugHook` happens only at a top-level form boundary and sets a per-heap **tier-policy flag**. Nothing tiers up, no JIT body is entered, and running fragments leave through the `Transfer` invalidation path. Detaching clears the flag at the next form boundary (§11.2:879-883; J13) |
| **Relies on** | Invalidation (§3.9); invariant 4: raw register readers read only at suspension points, where every word is a value or an immediate, and `DEAD_SLOT` renders as `#<dead>` (§11.1:832). That holds per instruction only in the interpreter, which is why hooks pin it. `RootSet::register` (`DebugHook: RootProvider`, §19:1894). Descriptor references through `CodeStore::escape` (§9.7:628-631). `PinToken` for debugger event payloads (§7:484-486) |
| **Must obey** | A paused hook that evaluates Scheme re-enters beneath a `Cx`, so it runs under `NoGcScope` and its allocation is counted by K16 (§11.2:882-883). Debugger writes into suspended frames go through `return_into` (§11.1:840-842) |
| **Cranelift** | No DWARF and no debug tags |

### 3.13 Code memory (`MAP_JIT`, freeing, W^X)

| | |
|---|---|
| **Decision 20** (proposed) | Patina's own `MAP_JIT` reservation. W^X toggled only in `install_code`. Replaced bodies freed by epoch, batched with one W^X toggle. Size-segregated slabs with a coalescing list, under a cap. BTI landing pads if enabled. Stage 6 freezes only the interface (`install_code`, per-unit free); the allocator and cap belong to the JIT track's first merged stage (§2:220; §9.7:635-639; §17.2:1452; §19:1881) |
| **Relies on** | **C12**: a body goes with its unit. A unit that never escaped is released when its form finishes, with no collection; an escaped unit waits for a complete major (§9.7:626-633). Code release is epilogue step 8, majors only (§9.9:675). Under generational mode the backstops bound how long that waits: a major after 256 minors or max(1 GiB, 64·L) of nursery allocation (§9.2:561-562). **F4**: finalizers run after the pause, Rust-only. **M3**: JIT code is charged as external bytes and enters L, the trigger and `max_heap` (§15:1298; §17.2:1440), reported per owner as `jit-code-bytes` (§17.2:1462). **Limits**: under `RLIMIT_AS` the JIT reservation is sized before the heap's (§17.3:1473-1474); past `PATINA_JIT_CODE_MAX`, tiering stops without error (§17.3:1487) |
| **Must obey** | Machine code changes only in `install_code` (§14:1289-1290); icache maintenance happens there (K7, §20:1946). Gated by the `eval-lambda` and `eval-redefine` steady-state rows (§17.2:1452) |
| **Under N carriers** | Linux `mprotect` is process-wide, so a dual-mapped `memfd` code reservation (RW and RX views) is needed before OS-thread carriers. macOS `MAP_JIT` toggling is per thread [M, probe `mapjit.c`] (§18.1:1644-1645). Reuse of freed code waits until every carrier passes a quiescent point (`quiesce_epoch`, 0x68, §14:1238) that executes an `isb`, since arm64 cores may hold stale prefetched instructions (`followup/parallelism/itemize.md:464-470`) |
| **Cranelift** | Avoid `JITModule` (decision 20): it uses `region::protect` [cg §2.7], and "no `MAP_JIT`, frees only whole modules" is **[rev]** (§3:272; DESIGN.md:2480). Drive `cranelift_codegen::Context` and install the emitted bytes into Patina's reservation. BTI is not checked in the study |
| **Note** [I] | Immortal-space addresses (J6 case 1) are per heap, so a body is valid only for its heap; there is no code sharing across heaps or isolates. `threads-patina-cost.md` item 11 (splitting code from constants) is the prerequisite if that is ever wanted |

### 3.14 Exceptions (`try_call`)

| | |
|---|---|
| **Inside the contract** | `raise` is a `Transfer` (§11.2:864). The pc after every instruction that can raise is a suspension point with a map. `raise_step_stub` "stays over the erroring frame while handler code runs, collects and captures" (§11.1:830). Handlers, winds and prompts are VM data on the green thread (§18.1:1616-1617). A `Leaf` error comes back as `Mutator.status` (0x58, §14:1236), after which the fragment takes a `Transfer` path to raise it; an `Oom` takes the `Transfer` path to collect and retry (J3; §8:523). `&stack-exhausted` (§11.1:803-805), `&heap-exhausted` (§17.3:1497) and `&interrupt` (§12:1011-1012) are all raised at polls. **Nothing unwinds native frames** (§13:1082) |
| **Cranelift** | `try_call`/`try_call_indirect` and `resume_to_exception_handler` are not needed [cg §2.5]. Using them would clobber every register at each `try_call` site, which costs nothing extra because no Scheme value crosses a non-`Leaf` call anyway [cg §2.5, I]. It would also add a dependency on the "INTERNAL" unwinder crate |
| **Gaps** | Gap 4 in §7 (Rust panics crossing JIT frames) |

### 3.15 Multiple carriers (only if decision 7 flips)

| | |
|---|---|
| **Cost** | 2–4 engineer-weeks for a JIT built to this contract; 6–10 to retrofit one "built ad hoc for today's VM (raw `Rc<CodeObject>`s, patched caches, a single context)" (§18.3:1704; `itemize.md:464-471`) |
| **Already N-ready** | The `Mutator` per carrier in `x21`/`r15` through the entry trampoline; no embedded `Mutator`; code installed through one function; IC words as single traced words updated with relaxed stores; `ldclrb` to disarm a granule under `threaded` (§10:733); sub-word stores that are the same machine code (§18.4:1725); reserved `fenced_ap`, `quiesce_epoch` and `handshake` words (§14:1237-1239) |
| **New obligations** | **Rule 4(f)**: JIT code may load heap references with address dependencies instead of acquire loads, "under a documented exception forbidding value speculation and equality substitution of loaded references" (§18.2:1664). [I] This constrains tier 2: after an IC guard `x == expected`, it may not substitute `expected` for `x` in later dependent loads. The payoff is a JIT tax of 0–4% on arm64 instead of 3–60% with acquire loads (§18.4:1726, :1733). **Publication** follows `GcAttrs.publication`, chosen by the stage-6 spike (§14:1251-1252; §18.6:1774). **Invalidation and debugger stops** must deoptimize fragments on other carriers through a per-carrier handshake (`itemize.md:469`) |
| **Invisible to the JIT** | Stage P's parallel marking (§14:1283-1286) |

---

## 4. Collector changes the JIT does not see (and the ones it does)

| Change | What emitted code sees | Why |
|---|---|---|
| Stage 7, sticky generations | `GcAttrs.barrier` becomes `GranuleLog`, and the emitter writes the 4-instruction sequence with a call-free cold block (§10:720-735). `watermark` becomes `ReturnBarrier`, which tier 1 already honours by returning through `ret` and copying `ret` on tail calls (§11.1:835-839) | J8; C19 |
| Stage 8, evacuation | **Nothing** | J4 reloads every reference after each `Transfer`. Derived pointers never cross a suspension point (J1). No movable address is embedded (J6). IC words are traced and updatable, and rekeyed at step 5. Stage 8 is also when "nothing moves" (C7) stops being true, so it is the first stage to exercise these rules |
| K4's copying nursery | Nothing beyond stage 7 | It is composed into `MarkRegion` and never armed (§14:1280-1281); moving is covered as for stage 8 |
| Stage P, parallel marking | Nothing | It lives inside `collect`, with roots on the collecting thread (§14:1283-1286) |
| Incremental marking (only on a latency goal) | Slow paths only through `barrier_mode`, except SATB, which "would need `value_filter_bit = None` and a JIT recompile" (§14:1281-1282) | `GcAttrs` |
| Variant C | Globals become 1 load under `WATCHED` (§11.6:976; §19:1874) | — |
| Pacing, limits, decommit, finalization | Nothing | Slow paths and the poll helper only |
| The tree-walker | Never JIT-compiled; its heaps stay whole-heap and non-moving (decision 1, §11.4:938-940) | — |

---

## 5. If the JIT team wanted what the PRD excludes

### 5.1 Raw unboxed doubles (or untagged integers) in published frames at calls

**Status.** Excluded: "raw slots in published frames" (§14:1291). The jitfirst proposal had them, marking raw slots in
each per-safepoint map (`proposal-jitfirst.md:803`). The review rejected that: "the map is keyed by `(desc, pc)` and
the header records no tier" (row JIT-2), and raw slots in captured continuations "contradict invariant W" (row SEM-9).

**The least invasive amendment** [I] keeps the collector proper untouched and confines raw slots to live stacks:

| Area | Change |
|---|---|
| Frame header (V2) | A frame must record which body (tier) wrote it, in `link` flags or `meta`. One `(code, pc)` no longer means one layout, so J1's tier-independence and J5's "re-derive `ret` and resume in any tier" are lost: tier-down, deoptimization and debugger attach must **convert** a tier-2 frame (re-tag, or box through `Leaf` allocations) before a lower tier resumes it |
| Maps (V4) | Two bitsets per `(body, pc)`: live-tagged and live-raw |
| Root provider, clearing, verifier (V5, V7) | Each must consult the raw bitset. Invariant 3's safety argument breaks: today "a wrong map can then retain garbage … but never expose a freed referent" (§11.1:831). A raw slot can be neither cleared (it is live) nor traced, so a wrong raw bit becomes a missed root or a double traced as an address: memory-unsafe, where today it is only retained garbage |
| Capture (V11) | Capture already walks each frame's map to clear dead slots (§13:1059-1061). It would also convert raw slots to tagged values, boxing out-of-band doubles. That allocates during capture, but through the no-collect fast path. `T_CONT` then stays value-only, invariant W holds there, and `ObjectModel::trace` for `T_CONT` stays word by word with no VM dependency in `patina-gc` (§14:1107) |
| Raw readers (V6) | Tracer, watchpoints and `--dump` need tier-aware decoding |
| Unaffected | Barriers (frame stores are never barriered), the watermark (raw slots are not references), evacuation (raw slots need no update), embedding |

**Route.** K12 (more than 15% loss on call-heavy float code) first adds `Leaf` helpers and inlining. Anything further
is "a separately approved deopt-at-capture design that amends §14's exclusions" (§20:1951; B5 §4:291).

### 5.2 Native stack maps (Cranelift user stack maps)

**Status.** Excluded (§14:1290-1291; B5).

Native maps only matter if native frames survive across suspension points, which presupposes S2 or long-lived tier-2
native frames (§3.10). The changes:

| Area | Change |
|---|---|
| Walker and registry | A native frame walker over FP chains (`preserve_frame_pointers` [cg §2.2, §2.7]). A code registry from return address to map, registered and unregistered with code freeing. `JITModule` does not surface maps, so Patina must read `MachBuffer::user_stack_maps()` itself [cg §2.1] |
| Slot visitor (§14:1122-1130) | `SlotVisitor::slot` takes a `HeapSlot`, whose pointer derives from the per-heap base (§10:790), so native stack slots need a new entry. Reporting them through `pinned` instead pins their blocks and defeats evacuation for those referents (C8) |
| Continuations (§13) | Capture must deoptimize native frames into VM frames, which needs the full frame state, not just GC references (debug tags; "unproven outside Wasmtime" [cg §7 q7]). The alternative, copying native stacks, means relocating absolute FP chains, which Cranelift does not support [cg §3.1, §5]. Design A's `memcpy` no longer suffices |
| Green threads | Each needs its own native stack, or a switch must deoptimize every native frame. `threads-patina-cost.md` §2.8: "Each of those costs months" |
| Watermark | Hijacking native return addresses, as JEP 376 does |
| Depth | A cap or very large stacks (10 M frames is about 480 MB [cg §5]), with per-carrier native stack limits (rule 7, §18.2:1667) |
| Cost model | Every `Leaf` call becomes a Cranelift safepoint, because all non-tail calls are [cg §2.1]. Declared references are spilled around helpers that never collect, which the current contract avoids |
| Text to amend | §11.2, §13, §14's exclusions, B5, K12, decision 8 |

### 5.3 Embedding movable (or merely mortal) constants in machine code

**Status.** Excluded outside J6's four cases (§11.2:866-872; §14:1292).

Two separate hazards apply:
- **Death and reuse.** This exists even before stage 8: an object dies and its address is reused, so "a reused
  address could satisfy a stale type test" (§11.2:870-871).
- **Movement**, from stage 8 (§9.4).

| Option | What it costs the contract |
|---|---|
| Code as a root container, patched at GC | Code bodies become a `RootProvider` over relocation records. Embedded references must be strong, or weak with invalidation, and treated as old→young edges in every minor (remember-whole). Evacuation must **patch code at GC**, which is excluded. Each patch toggles W^X (per thread on macOS, process-wide `mprotect` on Linux), needs icache maintenance, and under N carriers a handshake. HotSpot's nmethod oop tables with entry barriers are the precedent (`understand/jit-readiness.md` §4) |
| Pin each referent (`PinToken`) | Per-block pin counts block evacuation of every block holding an embedded constant (§7:484-486); fragmentation is watched by K3 |
| **What the PRD does instead** | Allocate the object in the NMS through `alloc_old` (K-7) and reference it from the unit's constants: that is J6 case (4), added for exactly this (§22:2032). Otherwise, one load from the descriptor's traced constants |

### 5.4 Load (read) barriers

**Status.** Excluded (§14:1288; §3:273, where ZGC- and Shenandoah-style barriers cost "about 5.4% average"; decision
6, throughput first).

They are only worth having for concurrent marking or compaction. The changes:

| Area | Change |
|---|---|
| Hot loads | Every `car`, `cdr`, `vector-ref`, closure-variable and cell load gets a check and a slow path. Loads are about 10× more frequent than barrier-site stores; `ReadCell` plus `LoadClosure` alone are 28% of nboyer's dispatches (`understand/jit-readiness.md` §2.3) |
| Encoding | Colored pointers conflict with 4-bit heap tags and raw addresses (§5) |
| Capability model | **C1 and the `'gc` brand collapse.** Collection would run while mutators hold values, so "a `Value<'gc>` cannot be alive while a collection runs" (§11.3:893-894) and gc-arena's "mutation XOR collection" (§3:243) no longer hold |
| Determinism | C6 goes unless concurrent work is scheduled deterministically |
| Allocation and barriers | Allocate-black and SATB (`value_filter_bit = None`) |
| Tables | `GcAttrs` gains a load-barrier kind, and every emitter and Rust twin a load sequence |
| Precedent | Wasmtime replaced its reference-counting collector (DRC, which emits read barriers) as the default with a barrier-free copying one in 46.0; DRC "was the default until 46.0" [cg §2.3] |

### 5.5 Other excluded shapes, briefly

| Shape | Why excluded |
|---|---|
| Allocation that may collect (Wasmtime's `gc_alloc_raw` does [cg §2.3, §6.2 item 4]) | Every allocation site becomes a suspension point that needs publish, map and reload, and 230–890 Rust functions that hold values across allocation lose their soundness (§8:500). B2 and K16: "allocation itself never becomes a collection point" (§20:1955) |
| Code patching for ICs or call targets | Outside `install_code` it is excluded (§14:1289-1290); K7 allows patched entry blocks only inside `install_code` |
| `stack_switch` for green threads or generators | [cg §2.6] (§3.11) |
| Conservative scanning of JIT or Rust stacks | Excluded (§14:1288-1289); the #423 tests forbid it (§3:256) |

---

## 6. What today's design would require of the same JIT

Today a JIT could be a Sparkplug-like template compiler over VM frames in which **every heap operation is a call into
Rust** and the register-file base is reloaded after every call. It would remove dispatch overhead, which is 66–76% of
samples in Track P's profiles (`understand/jit-readiness.md` §1), and leave every heap path in Rust.

The only rules today's code states for a compiled tier are in `vm:runtime/control.rs`:
- :27-30: "must use the same guard/safe-point discipline and the existing `VmState` root provider, and publish all
  live Scheme values before servicing a safe point";
- :81-82: "must participate in this protocol, not catch the sentinel";
- :104: "Native Rust stack frames are not captured".

J1 and J4 are the formal versions of the first rule.

| Obstacle | Source | What a JIT on it must do | Removed by |
|---|---|---|---|
| Every heap access goes through `SharedHeap = Rc<RefCell<Heap>>` | `core:heap/mod.rs:51` | call a borrowing `extern "C"` helper for every `car`, `cdr`, `vector-ref` and cell read, each with a borrow-panic path; no inline heap access at all | 3 (`Cx`, `GcDriver` token); 5e deletes the `RefCell` (§11.3:898; §19:1863, :1872) |
| Values are arena **indices** (`pair(i) = i << 3 \| 011`; `heap_index = bits >> 3`), under 3-bit tags | `core:tagged_value.rs:376-378`, `:410-413`; tags `:76-84` | `car` is a base load, a shift, an add and a load; the base must be reloaded (next row) | 5: raw tagged addresses, 4-bit heap tags (§5) |
| Arenas are growable `Vec`s that **relocate** on any push; vectors and strings are `Vec<Vec<…>>` | `core:heap/mod.rs:304-316`; `alloc_pair` pops a free list or pushes, `:703-714` | no loop-invariant base; reload after every allocation or helper; two indirections per vector element; no inline bump allocation | 5: one reservation, blocks, bump allocation (§7, §8) |
| A 72 B Rust enum with `Rc`/`RefCell`/`Vec` payloads and an unspecified discriminant layout | `core:heap/mod.rs:143-215`: `Real(f64)` :146, `Procedure(Rc<…>)` :159, `Record { Rc<RTD>, Rc<RefCell<Vec>> }` :163-165, `Parameter` :174, `Promise(Rc<RefCell<…>>)` :178, `MutableCell(RefCell<TaggedValue>)` :190, `VmClosure { Vec, Rc<Environment> }` :205-212 | type tests and field loads only through helpers. `ReadCell` (11.7% of nboyer dispatches) goes through a `RefCell`. A closure call matches the enum, because closures carry `TAG_OBJECT` and `TAG_CLOSURE` is never minted (`get_vm_closure_code_id`, `core:heap/mod.rs:1331`). Every flonum is a heap object | 4g, 5 (`declare_layouts!`; no `Drop` payload; self-tagged flonums subject to K6) |
| `eq?` compares the `Rc` behind two slots for `Procedure`, `RecordType` and `Record` | `core:heap/mod.rs:2117-2143` | `eq?` is a helper call, not one compare | 4a (canonical identity), 5 |
| The register file is a `Vec` resized on frame push and tail replace; frames are a separate `Vec<CallFrame>` | `vm:runtime/execution_state.rs:17-23`, `:55-75`, `:100-110` | no frame pointer in a register across a call; reload the base after every call or helper | 4d: a fixed reservation with interleaved frames (§11.1) |
| `CallFrame.code: Rc<CodeObject>`; `closure: Option<HeapIndex>` | `vm:types/mod.rs:41-60` | reference-count writes in every JIT call and return; a closure index that only `visit_object_index` roots | 2 (closure as a value), 4d (header), 4e (traced code liveness) |
| Collection is decided per dispatched instruction, and every loop takes a `GcDeferGuard` | `vm:runtime/vm_state.rs:1196-1206`, `:1287-1309`, `:1151` | poll at every bytecode boundary (+1.1–1.4%, §12:1027-1028), or prove which sites suffice; nested loops never collect | 3: frame-entry polls, `NoGcScope` (§12) |
| A global read is a heap borrow, an `Rc<Environment>` clone and a cache probe | `vm:runtime/vm_state.rs:1357-1365` | every `LoadGlobal` is a helper | 4b (cells, link tables), C (one load) |
| A shadowed inline primitive deoptimizes by name lookup | `vm:runtime/control.rs:3467-3489`; `vm:runtime/vm_state.rs:1374-1393` | read the shadow bitsets per site, plus a by-name slow path | 4b keeps the bitsets; C adds per-binding guards; JIT sites use `WATCHED` |
| Continuations are deep clones in weak side tables under the one-dispatch rule | `vm:runtime/execution_state.rs:239-261`; `vm:runtime/vm_state/gc_roots.rs:21-24` | a capture helper inherits the one-dispatch rule; captured frames keep code alive by `Rc` | 4e (design A) |
| Code release depends on sweep reporting each dead closure's code id | `vm:runtime/vm_state.rs:533-573` | free JIT bodies through the same sweep report, which breaks under any collector that skips dead objects | 4e (§9.7) |
| `TailCall` stages arguments in a Rust buffer | `vm:runtime/vm_state.rs:1601-1630` | enforce "no half-built frame" itself | 3, 4d (§12:1025-1027) |
| The primitive ABI is `fn(&SharedHeap, &[TaggedValue])`, with no helper class | `prim:registry.rs:14` | treat every primitive as may-do-anything (publish and reload), or audit each by hand | 3 (`Prim` over `Cx`; `#[helper(class)]`) |
| There is no context object; the poll word is an `Rc<Cell<bool>>` | `core:heap/mod.rs:389-395` | no pinned-register context; reach the poll word through an `Rc` | 3 (the `Mutator` ABI, §14:1217-1239) |

**Conclusion.** Every prerequisite is a redesign stage the plan does anyway:
- 2 (closure as a value);
- 3 (`Mutator`, `Cx`, frame-entry polls, helper classes);
- 4b (cells), 4d (stack and header), 4e (heap continuations, traced code liveness);
- 5 (raw addresses, layouts, no `Rc`/`RefCell`), then C (one-load globals).

The PRD's position is decision 21: representation first (§2:221). A JIT built ad hoc on today's VM would need 6–10
weeks of retrofit to become multi-carrier later (§18.3:1704).

---

## 7. Gaps and tensions (JIT-specific)

1. **`Tick` accounting is not tier-invariant.** Under `PollKind::Tick`, which exists only for SRFI 18's deterministic
   scheduler, "poll sites also decrement `ticks`" (§12:1031-1034). Inlining, and loops formed by inlining, change the
   set of poll sites, so preemption points could differ between tiers. Tier-up is counter-driven and so
   deterministic, but a JIT-on run could interleave threads differently from a JIT-off run. The PRD does not say
   whether ticks must count bytecode frame entries in every tier.
2. **`GcAttrs` must be constant for a heap's lifetime once JIT code exists.** Per-heap policy is "a runtime field read
   only by slow paths" (§9:530-531). Yet a non-generational heap reports `BarrierKind::None` and "JIT code emits
   nothing" (§10:758-759), and SATB needs a recompile (§14:1282). Flipping a live heap's policy would therefore have to
   invalidate every body [I]. The PRD says only that `attrs()` is per heap and "the JIT reads it" (§14:1149) and that
   emitters switch on it at compile time (§14:1260); it never says what a flip does to installed bodies (`followup/contract/ANSWER.md`
   §4, A1).
3. **Barriers on inline-cache words.** Descriptors take "no barrier … after creation" (§10:757-758) and are "written
   only by initializing stores" (§6:427-428). But IC words are traced and updated at run time (§11.2:871;
   §9.7:632-633), and the PRD does not say where they live. If they live in a descriptor's constants, they may hold
   only NMS or immortal references (descriptors, record types, cells) or immediates, so that no old→young edge goes
   unlogged. Otherwise IC stores need the barrier.
4. **Rust panics crossing JIT frames.** The PRD says nothing. [cg §6.2 item 12] advises catching them at the
   runtime-call boundary, since Cranelift frames carry no unwind registration. `Leaf` errors already come back as a
   status; a panic in a helper should become a fatal status or an abort, never an unwind through a fragment.
5. **Ordering of `desc.entry` across carriers.** Tier-up writes `desc.entry` as data (§11.2:872; §13:1078). Rule 4
   covers heap stores that publish objects, and `quiesce_epoch` covers reuse of code memory, but nothing states the
   ordering of a code-pointer write that another carrier reads (also `prd-contract.md` §11 gap 4).
6. **"No JIT activation on the native stack" under nested drivers.** A `Transfer` helper that answers `Continue`
   returns into its fragment. If it ran a nested driver beneath a `Cx` (point C, residual `apply_proc`, a paused
   debugger's evaluation), the outer fragment's activation stays on the native stack throughout. "The driver"
   (§9.7:636) must mean the outermost driver, or there must be a per-carrier count of JIT activations.
7. **Invalidation versus the watermark trampoline.** Invalidation re-derives the `ret` of affected frames (§13:1080).
   Stage 7 swaps one frame's `ret` for a trampoline and saves the real target as `(code, pc)` (§11.1:836-838).
   Re-derivation must skip that frame, or update only the saved target, or the barrier is lost.
8. **Bytecode liveness binds tier 2.** J1's "the same `(code, pc)` means the same thing whichever tier wrote the
   frame" (§11.2:856-857) implies that tier 2 must publish every slot the bytecode map marks live, even values its own
   analysis finds dead. Only DESIGN.md:776-782 mentions materializing inlined frames; the PRD's §11.2 does not.
9. **Cranelift facts verified only by the review.** Four claims rest on the review (`REVIEW_DISPOSITIONS.md:43`) or
   DESIGN, not on `cranelift-gc.md`:
   - the pinned register is not saved by Cranelift prologues (§11.2:874-877);
   - `Tail` results arrive in `x2`;
   - `JITModule` lacks `MAP_JIT` and frees only whole modules (§3:272);
   - single-bit tests lower to `tbz`/`tbnz`.

   BTI is checked nowhere. The stage-6 spike should re-confirm all five against the Cranelift version it pins.
