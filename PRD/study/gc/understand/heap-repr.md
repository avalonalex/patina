# Heap and value representation in Patina (as of `28a94f8`)

Scope: the value encoding, the heap layout, the object catalogue with measured sizes, the numeric tower, strings, records and hash tables, every place an identity escapes `TaggedValue`, the `RefCell` access pattern and the VM hot paths, and what three changes would cost: (a) raw pointers, (b) object headers, (c) a moving collector.

Conventions. **[V]** means verified in source, by measurement, or in a primary source. **[I]** means inference. File references are repo-relative, `file:line` at `28a94f8`. Sizes were measured on aarch64 with the pinned toolchain (rustc 1.97.1, release) by a throwaway crate at `PRD/study/gc/probes/heap-repr` that depends on the repo crates by path. Timings come from single runs of `target/release/patina` on this machine. That binary is one commit behind HEAD; the commit it lacks (#602) only encapsulated VM execution state.

---

## 0. Summary of key numbers

| Thing | Value | Source |
|---|---|---|
| `TaggedValue` | 8 B, `u64`, low-3-bit tag | `tagged_value.rs:57,70-84` [V] |
| `HeapObjectData` (every non-pair/vector/string object) | **72 B** per slot | measured [V] |
| Same enum with `Exception`, `Rational`, `Identifier` boxed | **40 B** | measured replica [V] |
| Pair | 16 B per slot, no header | `heap/mod.rs:307` [V] |
| Vector / string slot | 24 B `Vec` header in the arena, plus a separate malloc of 8 B per element or 4 B per char | `heap/mod.rs:310,313` [V] |
| Boxed flonum | 72 B slot for 8 B of data | [V] |
| `Environment` | 224 B, `Rc`-allocated, outside the heap | measured, `environment.rs:487` [V] |
| `CallFrame` | 40 B, holds `closure: Option<HeapIndex>` | measured, `types/mod.rs:41-61` [V] |
| `Heap` struct | 528 B | measured [V] |
| Loading `(scheme base) (srfi 1) (scheme hash-table)`, GC off | 156,104 pairs and 93,241 objects allocated, of which **92,414 are `Identifier`** | census tool [V] |
| Same load, after one `(gc)` | **94 pairs and 717 objects live**; arenas keep their high-water capacity (156k / 93k slots) | census tool [V] |
| Fixnum loop of 10M iterations | 0.27 s | [V] |
| Same loop over flonums (one box per iteration) | 0.61 s, 152 collections | [V] |
| Heap `borrow()` / `borrow_mut()` call sites outside tests | ≈501 / ≈248 | `rg` [V] |

---

## 1. `TaggedValue` encoding

`#[repr(transparent)] struct TaggedValue(u64)` derives `Copy, Eq, Hash` (`tagged_value.rs:55-57`). `Hash` and `Eq` are on the raw bits. The module comment in `lib.rs` still says "NaN-boxed"; it is not.

| Tag (low 3 bits) | Meaning | Encoding | Payload bits used |
|---|---|---|---|
| `000` | fixnum | `n << 3` (`:134-138`), arithmetic shift right to decode (`:146-149`) | 61-bit signed, `FIXNUM_MIN = -(2^60)`, `FIXNUM_MAX = 2^60-1` (`:117-119`) |
| `001` | special | `FALSE=0x01`, `TRUE=0x09`, `NULL=0x11`, `EOF=0x19`, `UNSPECIFIED=0x21`, `FORWARDED=0xF1` (environment import forward, **not** GC forwarding), `GC_POISON=0xF9` (debug sweep poison) (`:91-110`) | bits 3–7: 32 codes, 7 used |
| `010` | char | `(cp << 3) \| 2` (`:233-235`); decode uses `char::from_u32(..).expect` (`:245`), so there is a panic branch on the hot path | 21 bits |
| `011` | pair | `(u32 index << 3) \| 3` (`:376-378`) | bits 3–34 |
| `100` | vector | index into `vectors` | bits 3–34 |
| `101` | string | index into `strings` | bits 3–34 |
| `110` | closure | `TaggedValue::closure()` **is never called outside its own tests** (`rg` [V]). VM closures and CPS lambdas are both tag `111` objects (`control.rs:423-430` says so). The `is_closure()` checks at `tree-walker/eval/application.rs:47`, `primitives/predicates.rs:130`, `datum_writer.rs:320`, `heap/mod.rs:1606,1932` are dead branches. `MarkBits::is_marked` returns `None` for this tag, which `value_is_live` treats as live (`gc.rs:528-534`) | dead tag |
| `111` | object | index into `objects`; subtype is the enum discriminant | bits 3–34 |

Spare bits [V]: a heap reference uses bits 3–34, so **bits 35–63 (29 bits) are always zero**. A char leaves bits 24–63 free. Specials use five payload bits.

**Properties worth keeping for a JIT [V/I]:**
- Fixnum tag `000` means `add`/`sub` work on tagged values directly. The signed 64-bit overflow flag on the shifted operands is exactly 61-bit fixnum overflow, so a JIT can use `adds` + `b.vs`. The Rust code instead untags, calls `checked_add`, then range-checks (`:165-175`).
- `#f` is one constant (`0x01`), so truthiness is a single compare.
- One primary tag is dead and could be reassigned, for example to an immediate-float scheme (§11).

**Identity is the raw word [V].** `eq?` starts with `a.raw() == b.raw()` (`heap/mod.rs:2122`). It also treats as identical two *different* object slots that wrap the same `Rc` for `Procedure`, `RecordType` and `Record` (`:2126-2142`).

That second rule exists because the same logical object can occupy several slots. Example: `%record-type-of` allocates a fresh `RecordType` slot around the same `Rc<RecordTypeDescriptor>` on every call (`primitives/records.rs:110-124`). As a result, "slot index = identity" is **already false** for those three types, and `identity-hash` hashes them by `Rc::as_ptr` (`heap/mod.rs:2549-2555`).

---

## 2. The `Heap` struct, arenas, allocation and interning

`SharedHeap = Rc<RefCell<Heap>>` (`heap/mod.rs:51`). There is one per interpreter instance, cloned into every `Environment` (`environment.rs:489`), every `CompiledMacro`, `GcDeferGuard`, and so on. `Heap` is 528 B and also holds per-instance metadata that is not heap storage: features, command line, library availability (`:349-364`).

**Arenas** (`heap/mod.rs:304-316`) [V]:
- `pairs: Vec<(TV,TV)>`
- `vectors: Vec<Vec<TV>>`
- `strings: Vec<Vec<char>>`
- `objects: Vec<HeapObjectData>`

Because these are growable `Vec`s, **a slot's address changes whenever its arena reallocates**. That is the underlying reason references are indices and not pointers.

**Free lists** (`:367-376`) [V]: one `Vec<HeapIndex>` per arena, used LIFO.
- Allocation pops a free index or pushes onto the arena (`alloc_pair :703-714`, `alloc_vector :770-781`, `alloc_string_chars :831-842`, `alloc_object :1469-1480`).
- Sweep pre-marks indices already on the free list, then pushes every unmarked index in ascending order (`gc.rs:828-852`). Allocation therefore pops the highest free index first.
- **Arenas never shrink.** There is no truncate or shrink anywhere in `heap/` (`rg` [V]). After the library-load census in §0 the process keeps 93k object slots (about 6.7 MB at 72 B) and 156k pair slots (about 2.5 MB) to hold 717 live objects and 94 live pairs. RSS was 41.7 MB with those libraries loaded, against 12.0 MB with `(scheme base)` alone.

**Accounting is by object count, not bytes [V].** `note_alloc` adds 1 per allocation, whatever the size (`:581-584`). The collection threshold is `max(65_536, 2 × live slots)` (`gc.rs:1001-1003`). A 10M-element `make-vector` counts the same as one cons, and the malloc'd element buffers of vectors, strings, bytevectors, bignums and closure `free_vars` are invisible to the trigger.

**Tombstones** (`gc.rs:875-961`) [V]:
- Dead objects are overwritten with `HeapObjectData::Free`. This drops their `Rc` payloads eagerly, and that eager drop is what breaks closure ↔ `Environment` cycles (`gc.rs:6-7`).
- Dead vectors and strings become `Vec::new()`, freeing their buffers.
- Pairs are only poisoned in debug builds.
- Mark bits are per-arena side `BitSet`s sized at collection start (`gc.rs:50-136`); nothing lives in object headers.

**Interning** [V]:
- `symbol_table: HashMap<String, HeapIndex>` (`:319`).
  - The name is stored twice: once as the `String` key and once as `Rc<str>` inside `HeapObjectData::Symbol`.
  - Interned symbols are immortal: `GcVisitor::new` marks every entry on every collection (`gc.rs:456-458`). The stage-5 PRD names this, together with code-store constants, as a monotonic root cost.
- `core_syntax_table: HashMap<CoreForm, HeapIndex>` (`:332`), likewise immortal (`gc.rs:462-464`).
- Program parsers do **not** intern written identifiers. Each written identifier becomes its own `Identifier` object with `written: true` (`:1033-1039`). After loading three libraries only 16 interned symbols exist; the syntax traffic is 92k `Identifier` objects.

**Syntax provenance** [V]: `syntax_sources: HashMap<u64 raw_bits, Rc<SyntaxSource>>` (`:305`, `heap/source.rs`). It is not a root. Sweep prunes entries whose key is unmarked (`gc.rs:894-900`).

**Ids that are not heap references** [V]:
- `next_vm_continuation_id` (`:430`) mints `u64` ids for `VmContinuationRef`. The VM's weak side tables are keyed by these ids, not by address (`vm_state/gc_roots.rs`), so they are moving-safe.
- `VmClosure.code_id` is a `CodeObjectId(u64)`, a slot and generation into `code_store` (`code_object.rs:34-79`), not a heap reference.

---

## 3. `HeapObjectData`: the 28 variants

`HeapObjectData` is a `#[derive(Clone)]` Rust enum (`heap/mod.rs:143-229`) of 72 B. Payload sizes below were measured for each payload type.

| Variant | Payload (B) | Off-slot allocations per object | Holds `TaggedValue`s? | Notes |
|---|---|---|---|---|
| `BigInt(BigInt)` | 32 | `Vec<u64>` digits | no | `num-bigint` 0.4 |
| `Rational(BigRational)` | 64 | 2 digit vectors | no | one of three variants that force the 72 B size |
| `Real(f64)` | 8 | — | no | **boxed flonum: 72 B slot for 8 B** |
| `Complex{real,imag}` | 16 | — | 2 | parts are themselves boxed objects |
| `Symbol(Rc<str>)` | 16 | `Rc<str>`, plus the `String` intern key | no | immortal |
| `Bytevector(Vec<u8>)` | 24 | buffer | no | |
| `Exception{kind,message,irritants}` | 72 | `String` and `Vec`; `ExceptionKind::Custom(String)` | `irritants` | the largest variant |
| `Procedure(Rc<Procedure>)` | 8 | `Procedure` is 120 B; tree-walker `CpsLambda` holds `Rc<CpsExpr>` and `Rc<Environment>` | inside `CpsExpr` literals | also wraps primitives |
| `Port(Rc<Port>)` | 8 | `Port` is 40 B | no | needs finalization |
| `Macro(Rc<CompiledMacro>)` | 8 | `CompiledMacro` is 224 B | pattern/template literals; definition env | |
| `RecordType(Rc<RTD>)` | 8 | RTD is 48 B | no | re-wrapped per `%record-type-of` |
| `Record{record_type, fields: Rc<RefCell<Vec<TV>>>}` | 16 | `Rc` box (16 + 32 B) **and** the field `Vec` | fields | three allocations per record; `%make-record` also first builds a field *vector* (`records.rs:138-182`) |
| `Identifier{name,scopes,written}` | 64 | `Rc<str>` (shared); `ScopeSet` is `SmallVec<[ScopeId;3]>`, 40 B inline | no | 99% of load-time objects |
| `Continuation(Rc<CpsContinuation>)` | 8 | `CpsContinuation` is 208 B | many (env, `ContEnv`, winds…) | tree-walker |
| `Parameter{values: Rc<RefCell<Vec>>, converter}` | 24 | `Rc` + `Vec` | yes | |
| `Promise(Rc<RefCell<PromiseState>>)` | 8 | 40 B `Rc` box | 1 | `promise_update` rewrites the slot to share a box (`:1078-1087`) |
| `Library(Rc<Library>)` | 8 | `Library` is 152 B, `exports: HashMap<String, TV>` | exports, env | |
| `Values(Vec<TV>)` | 24 | buffer | yes | every non-singular `values` allocates one (`:1130-1136`) |
| `EnvironmentSpecifier{env,mutable}` | 16 | — | via env | |
| `PromptTag(Rc<PromptTag>)` | 8 | 24 B | no | |
| `LabelPlaceholder(usize)` | 8 | — | no | parser-transient |
| `MutableCell(RefCell<TV>)` | 16 | — | 1 | **72 B slot for an 8 B box**; mutated through `&Heap` |
| `Ephemeron(RefCell<Option<(TV,TV)>>)` | 32 | — | 2 (weak key) | broken through `&Heap` during mark (`:1237`) |
| `VmClosure{code_id, free_vars: Vec<TV>, globals: Rc<Environment>}` | 40 | `free_vars` buffer; an `Rc<Environment>` clone at creation | `free_vars` | the VM's only closure representation |
| `VmContinuationRef(u64)` | 8 | payload in `VmState` side table | via table | weak id |
| `VmDelimitedContinuationRef(u64)` | 8 | same | via table | |
| `CoreSyntax(CoreForm)` | 1 | — | no | immortal |
| `Free` | 0 | — | — | tombstone |

What follows from the table [V]:
- Boxing just `Exception`, `Rational` and `Identifier` would shrink the enum from 72 B to **40 B**: in a replica enum, `VmClosure` at 40 B keeps its discriminant in a niche.
- 16 of the 28 variants own `Drop` payloads (`Rc`, `Vec`, `String`, `BigInt`). The design therefore depends on sweep visiting dead objects to run destructors. A collector that never visits dead objects (copying, or bump-and-reset nursery) loses that unless it keeps a finalization list.
- Tracing dispatches on the enum (`gc.rs:695-794`). A per-variant comment warns that misfiling a value-bearing variant as a leaf is a use-after-free that the compiler cannot catch (`heap/mod.rs:137-141`).
- [I] Tracing a `VmClosure` calls `visit_env(globals)`, which costs one `FxHashSet` insert per closure marked (`gc.rs:539-559,782-789`), even though nearly all closures share one globals environment.

---

## 4. Numeric tower

- **Fixnums:** 61-bit immediates. Overflow promotes to `BigInt` (`numeric.rs:690-703`).
- **Bignums:** `HeapObjectData::BigInt(num_bigint::BigInt)`. The digits live in a malloc'd `Vec<u64>` outside the arena.
- **Rationals:** `BigRational` (`Ratio<BigInt>`), always big, even for `1/2`.
- **Flonums: always boxed** as a 72 B `Real(f64)` slot (`alloc_real`, `heap/mod.rs:896-898`). Every inexact arithmetic result allocates (`numeric.rs:766-776,863-876,964-972,1101-1103`).
- **Complex:** two `TaggedValue`s, each normally a boxed `Real`. `complex64_to_tagged` therefore allocates three objects (`numeric.rs:345-354`).
- **Copies on read [V]:** `extract_num_data` **clones** `BigInt`/`BigRational` on every arithmetic dispatch (`numeric.rs:418-435`).
- **Cost of a flonum add [I]:** by reading `numeric_add` (`numeric.rs:690-711`), one generic inexact add performs about eight enum lookups through `get_object`, for NaN, inexactness, complex and extract checks, before `alloc_real`.
- **Measured [V]:** the same 10M-iteration tail loop takes 0.27 s with a fixnum accumulator and 0.61 s with a flonum accumulator (+34 ns per iteration), with 152 collections at the 65,536-allocation floor.

Prior art: Chez boxes flonums too (`type-flonum #b010`, an 8 B datum, padded to 16 B so a forwarding address fits; `ChezScheme/s/cmacros.ss:824,1505-1513`). Chez also has a `PRESERVE_FLONUM_EQ` side bitmap so `eq?` on flonums survives copying (`c/types.h:173`). Self-tagging (§11) removes most flonum boxes.

---

## 5. Strings, bytevectors, vectors, records, hash tables

**Strings** [V]:
- `Vec<char>`: 4 B per char (UTF-32) for O(1) `string-ref` and `string-set!` (`heap/mod.rs:312-313,826-868`).
- A 24 B arena slot plus a separate malloc. `alloc_string(String)` re-encodes UTF-8 into `Vec<char>` (`:826-828`), and I/O converts back (`get_string_as_utf8 :871`).
- String tag `101` is a leaf in tracing (`gc.rs:490-492`).

**Bytevectors:** objects-arena `Vec<u8>`, so a 72 B slot plus a malloc. They are not a primary tag.

**Vectors:** `Vec<TaggedValue>`, so reaching an element needs two dependent loads: the arena slot (ptr, cap, len), then the buffer. `vector_slice_mut` hands out `&mut [TV]` to callers (`:816`; used at `vm_state.rs:2380`). That is a bulk-write API with no barrier hook.

**Records:**
- `Record{Rc<RTD>, Rc<RefCell<Vec<TV>>>}`, so field access goes slot → `Rc` → `RefCell` flag → `Vec` buffer.
- The primitives `%record-ref` and `%record-set!` (`records.rs:188-273`) are ordinary heap-borrowing primitives with no inline opcode.
- `define-record-type` is Scheme over these primitives.

**Hash tables: all implemented in Scheme, none native [V].**
- SRFI 69 is the base implementation: a record holding a vector of alist buckets (`lib/srfi/69/srfi-69-impl.scm:136-170`).
- SRFI 125 / `(scheme hash-table)` is a thin layer over SRFI 69 and SRFI 128 (`lib/srfi/125.sld:3`, `lib/scheme/hash-table.sld:1-6`).
- R6RS `(rnrs hashtables)` re-exports `(r6rs hashtables)`, Clinger's port over SRFI 69 (`lib/rnrs/hashtables.sld`, `lib/r6rs/hashtables.atop69.scm:168-169`).

Hash functions and their move-sensitivity [V]:

| Hash function | Defined at | Behaviour | Moving-safe? |
|---|---|---|---|
| `hash-by-identity` (all `eq?` tables, including R6RS `make-eq-hashtable` and SRFI 125 `make-eq-comparator`) | `srfi-69-impl.scm:118-120` | `(modulo (identity-hash obj) bound)`. The `identity-hash` primitive (`primitives/equality.rs:66-79`) calls `tagged_value_hash_identity` (`heap/mod.rs:2541-2561`), which is `mix(heap_index)` for heap values, or `Rc::as_ptr` for procedure, RTD and record | **No.** The result is baked into the bucket a key sits in. The design comment relies on "the collector does not move objects" (`heap/mod.rs:2532-2534`; `srfi-69-impl.scm:113-114`) |
| `equal-hash` (SRFI 125 `immutable-tables` registry, `lib/srfi/125.sld:137-138`; SRFI 128 default hash, `lib/srfi/128/128.body2.scm:119`) | `equality.rs:50-63` → `tagged_value_hash_depth` (`heap/mod.rs:2563-2675`) | structural for numbers, strings, symbols (by name), pairs, vectors and bytevectors; **falls back to `heap_index` for every other object** (`:2668-2670`), including records, procedures and the `<srfi-hash-table>` records used as keys in `immutable-tables` | **No** for non-structural objects |
| `eqv?` tables | `srfi-69-impl.scm:122-126` | use the structural `hash`, deliberately, as chibi does | yes; mutation-sensitive by design |

So a moving collector must either keep identity hashes stable (a hash field in the header) or make eq-tables native and GC-aware, as Chez's tlc rehash list does (§11).

---

## 6. `TaggedValue` identity and raw-index escapes: complete inventory

Usage counts outside tests [V]:

| API | Total uses | Outside `patina-core` |
|---|---|---|
| `heap_index()` | 60 | 1 (`vm/runtime/control.rs:426`) |
| `TaggedValue::object/pair(` | 15 | 0 |
| `.raw()` | 32 | 18 |
| `raw_bits()` | 32 | 6 |

Index arithmetic is well contained in `patina-core`.

| # | Where | What escapes | Lifetime | Required for a moving GC |
|---|---|---|---|---|
| 1 | `Heap::symbol_table` (`heap/mod.rs:319,981-990`) | `HeapIndex` | permanent root | update on move, or allocate symbols in a non-moving space |
| 2 | `Heap::core_syntax_table` (`:332,997-1005`) | `HeapIndex` | permanent root | same |
| 3 | `Heap::syntax_sources` (`:305`; `source.rs:15-61`) | `raw_bits` keys | until the key dies; pruned at sweep (`gc.rs:894`) | rekey on move, or move provenance into a field or side table of the syntax object |
| 4 | `SourceMap::locations` (`source_map.rs:61,179-185`) | `raw_bits` keys | per program, pruned via `take_gc_freed_bits` (`heap/mod.rs:647-661`) | same, plus a "moved" feed alongside "freed" |
| 5 | `GcFreedBits` buffer (`heap/mod.rs:401`; `gc.rs:861-873`) | raw bits of freed slots | between drains | would need to report moves, not just frees |
| 6 | `CallFrame.closure: Option<HeapIndex>` (`vm/types/mod.rs:50`; set at `control.rs:184`) | bare index, also copied into continuation snapshots | while the frame or snapshot lives | rooted via `visit_object_index` (`gc.rs:512`, `gc_roots.rs:153-160`); must become updatable, or a `TaggedValue` |
| 7 | `eq?`/`eqv?`/`equal?` fast paths (`heap/mod.rs:2122,2155,2240,2382`) | raw compare | instantaneous | fine if a moved object is never seen under two addresses (no GC mid-compare) |
| 8 | `identity-hash`, `equal-hash` fallback (`:2541-2561,2668-2670`) | heap index stored in Scheme buckets | **persistent, in user data** | stable hash word, or native GC-aware tables |
| 9 | `Rc::as_ptr` identity for Procedure, RTD, Record (`:2129-2140,2549-2552`) | Rust heap address | `Rc` lifetime | moving-safe while payloads stay `Rc`; must be replaced if records become inline GC objects |
| 10 | `PrimitiveCallMap.by_value` (`vm/compiler/primitive_calls.rs:216`, read at `pass5_codegen.rs:903`) | `raw_bits` of procedure values | one compilation unit | safe if no GC during compile; else rekey |
| 11 | Desugarer `map_syntax_identifiers_memo` replacements (`source.rs:85-177`) | `raw_bits` | one form; callers must "prevent collection" (`:82-84`) | same discipline |
| 12 | Transient cycle sets: `walk.rs:33`, `quasiquote.rs:119`, `desugarer/mod.rs:129`, `macro_expander/mod.rs:254`, `parser/mod.rs:624`, `eval.rs:27-40`, `datum_writer.rs:552-798` (`raw() as usize` as "addr"), `Heap::is_list/list_len/tagged_values_equal` (`:2365,2955,2982`) | raw bits | one call | safe under "no GC inside a primitive or parse" |
| 13 | `CodeObject.constants: Vec<TV>` inside `Rc<CodeObject>` (`code_object.rs:140`) | TVs in an immutable shared structure | while the code is loaded | needs `Cell`/`UnsafeCell` or a non-moving literal space. **Instructions are clean:** `LoadImmediate` and `*Imm` only ever embed immediates (`pass5_codegen.rs:455,699`) |
| 14 | Tree-walker `CpsExprKind::Literal` (`cps_expr.rs:215`), `CoreExprKind::{Literal,Quote,Import,Datum}` (`core_expr.rs:147-279`) | TVs in `Rc` trees | code lifetime | same as 13 |
| 15 | `CompiledMacro` `Pattern/Template::Literal` (`compiled_macro.rs:36,262`) | TVs in `Rc<CompiledMacro>` | macro lifetime | same |
| 16 | `Environment` slots: `Bindings.slots: SmallVec<[(Rc<str>, TV);3]>` (`environment.rs:208-216`) and `ScopedBinding.tagged_value` (`:14-23`) | TVs | env lifetime | `RefCell`, so updatable during GC |
| 17 | `Library.exports: HashMap<String, TV>` (`library.rs:30`) inside `Rc<Library>` | TVs | library lifetime | `pub` field in an `Rc`; needs interior mutability |
| 18 | VM `registers`, `scratch_args`, `pending_escape`, prompt/handler/wind stacks (`execution_state.rs:16-23`, `vm_state.rs:54,188`) | TVs | live | updatable `Vec`s; `WindRecord.handlers: Rc<[H]>` (`continuation.rs`) is shared and immutable |
| 19 | VM continuation snapshots `RefCell<FxHashMap<u64, Rc<VmContinuation>>>` (`vm_state.rs:196`; `registers` at `continuation.rs:79,173`) | TVs plus `CallFrame.closure` indices | weak by id | inside `Rc`, so needs `Rc::get_mut`-style uniqueness or `Cell` |
| 20 | Tree-walker `CpsContinuation`/`ContValue`/`ContEnv` (`cont_value.rs`, `continuation.rs`) | TVs in persistent `Rc` lists | continuation lifetime | immutable `Rc` graph |
| 21 | `Record.fields`, `Parameter.values`, `Promise` state, `MutableCell`, `Ephemeron` | TVs behind `RefCell` | object lifetime | updatable |

**The design doc's ruling.** `docs/GC_DESIGN.md` §3.4 lists items 1, 2/4, 3 (`eq?`), 6, 13, 15 and 16 and concludes "moving … ruled out permanently" (`PRD/ARCHIVE/GC_STAGE5_PRD.md`, Non-goals).

My reading:
- **[I]** Items 1, 3–6 and 10–12 are mechanical: they are rekeyable or already transient.
- **[I]** The structural blockers are item 8, because identity hashes live inside Scheme data, and items 13–15, 17, 19 and 20, because references sit in immutable `Rc`-shared Rust structures that the visitor sees by value (`GcVisitor::visit(tv: TaggedValue)`, `gc.rs:485`), with no way to write back.

---

## 7. Off-heap structures that reference the heap

These sit outside the arenas, are reached through `Rc`, and are traced specially by `GcVisitor` with dedup sets (`gc.rs:422-446`):

- `Environment` (224 B): globals, libraries, and **one per tree-walker call or `let`**. It holds a `SharedHeap` `Rc` clone and a parent chain. Alias and owner edges leave the chain (`gc.rs:536-559`). Imported slots hold `FORWARDED` and resolve through `RareTables.links` (`environment.rs:405-433,605-611`).
- `CpsContinuation` (208 B), `ContValue` chains, and `ContEnv` persistent lists. Without dedup their trace was exponential: 6.8 s at depth 26 (`gc.rs:570-582`; design §9.4).
- `CodeObject` (160 B) constants, plus `register_roots` stack maps (`code_object.rs:158`). These maps are already a precise per-pc liveness map for VM registers, applied at collection and at capture (`gc_roots.rs:42-64`). That is a ready seam for JIT stack maps.
- `CompiledMacro` (224 B), `Library` (152 B) and `Procedure::CpsLambda` (120 B).

**Edges run in both directions [V].** Heap objects own `Rc<Environment>` (`VmClosure.globals`, `EnvironmentSpecifier`, `CpsLambda.env`), and environments hold `TaggedValue`s back into the heap. Cycles are broken only because sweep tombstones dead objects and drops their `Rc`s (`gc.rs:5-7`; design §8). **[I]** Any collector that does not visit dead objects (copying, or bump-reset nursery) needs either a finalization list of `Rc`-owning objects or environments moved into the GC heap. Otherwise closure ↔ environment cycles leak through `Rc`.

---

## 8. The `RefCell` access pattern and the hot paths

**Borrow sites** (`rg 'heap\w*(\(\))?\.borrow(_mut)?\(\)'`, test directories excluded) [V]:

| Crate | `borrow()` | `borrow_mut()` |
|---|---|---|
| patina-primitives | 258 | 148 |
| patina-frontend | 140 | 45 |
| patina-macros | 39 | 11 |
| patina-vm | 29 | 21 |
| patina-tree-walker | 21 | 9 |
| patina-core | 4 | 11 |
| others (runtime, repl, compat, interpreter, tests) | 10 | 3 |
| **Total** | **≈501** | **≈248** |

Busiest files:

| File | Sites |
|---|---|
| `desugarer/mod.rs` | 49 |
| `parser/mod.rs` | 47 |
| `primitives/lists.rs` | 40 |
| `strings.rs` | 31 |
| `vectors.rs` | 27 |
| `ports.rs` | 25 |
| `vm_state.rs` | 24 |

About 280 of ≈304 registered primitives have the shape `fn(&SharedHeap, &[TaggedValue])` (`registry.rs:14`), and each takes its own borrow. By contrast, `HeapObjectData::` is named only 32 times outside core: 28 in `datum_writer.rs`, 2 in the parser, 1 in ephemeron, 1 doc comment. **The object layout is encapsulated; the borrow protocol is not.**

**Two kinds of mutation [V]:**
- Through `&mut Heap`: `set_car`/`set_cdr` (`:743-763`), `vector_set`/`vector_slice_mut` (`:804,816`), `set_vm_closure_free_var` (`:1394`), bytevector writes, and `promise_update`'s slot rewrite (`:1085`).
- Through `&Heap` plus an inner `RefCell`: `write_mutable_cell` (`:1273-1284`, which the VM calls under a *shared* borrow at `vm_state.rs:2407-2414`), record `fields.borrow_mut()`, parameter `values.borrow_mut()`, promise state, and `break_ephemeron`.

Write-site counts outside tests are small:

| Write API | Sites |
|---|---|
| `set_car` | 12 |
| `set_cdr` | 15 |
| `vector_set` | 14 |
| `vector_slice_mut` | 2 |
| closure free-var | 2 |
| `write_mutable_cell` | 2 |
| `promise_update` | 4 |
| parameter `values` | 3 |
| record `fields` | 1 |

A barrier would have to be inserted in both families, but the number of sites is tractable [V counts, I conclusion].

**VM hot paths** (`dispatch_one_instruction`, `vm_state.rs:1406ff`) [V]:
- **Every instruction:** `dispatch_frame` takes `frames.last_mut()`, does an `Rc::ptr_eq` on the cached code, and advances pc (`execution_state.rs:135-143`). Inline ops first test `is_primitive_shadowed` (a bitset word, `vm_state.rs:374-378`).
- **`Car`/`Cdr`** (`:2119-2142`): tag test, `heap.borrow()` (flag load, check, increment, decrement), bounds-checked `pairs[idx]`, a 16 B load, and a debug poison assert.
- **`VectorRef`** (`:2332-2350`): tag tests, borrow, `vectors[idx]` bounds check, a load of the `Vec` header, `.get(i)`, then the element: **three dependent loads plus two bounds checks**.
- **`LoadClosure`** (`:1451-1468`): `frames.last().closure`, borrow, `objects.get(idx)` (72 B stride), discriminant match, `free_vars.get(slot)` through the `Vec` buffer: **four dependent loads plus a discriminant check**. `StoreClosure` takes a `borrow_mut`.
- **`ReadCell`/`WriteCell`** (`:2395-2415`): borrow, `get_object`, discriminant match, inner `RefCell` flag, value.
- **`LoadGlobal`** (`:1492-1507`):
  - `frame_globals` (`:1356-1363`) costs a heap borrow, an object lookup, a match, and an **`Rc<Environment>` clone and drop on every global read**.
  - Then a cache probe (`code_object.rs:229-238`) and `slot_value`, which borrows the bindings `RefCell` and compares against `FORWARDED`. An imported binding takes a second hop through `RareTables.links`.
- **`Call`/`TailCall`** (`:1580-1640`; `control.rs:169-209`):
  - A borrow, then a discriminant check to get `code_id`, then `code_store[id]` with a generation check and an `Rc<CodeObject>` clone.
  - `push_frame` then resizes the shared register `Vec`, which may reallocate (`execution_state.rs:52-73`).
  - A variadic call conses its rest list under `borrow_mut`.
- **`MakeClosure`** (`:1538-1552`): collects `free_vars` into a new `Vec` (malloc), calls `frame_globals` (`Rc` clone), `borrow_mut`, and `alloc_object` (72 B write, count, threshold compare).
- **`Cons`:** `borrow_mut`, `note_alloc`, free-list pop or `Vec` push.

**[I] Consequences for a JIT.**
- None of these sequences can be emitted inline as written. The arena bases move when a `Vec` grows, the borrow flag is a Rust-only protocol, and allocation is a Rust function with a free list.
- JIT code would bypass `RefCell` altogether. That is sound only under the invariant gc-arena calls *mutation xor collection*: no Rust `&mut Heap` is live while compiled code runs, and collection happens only at safe points. Patina already enforces the collection half with outermost-only safe points and `GcDeferGuard` (`gc.rs:219-268,381-409`).

---

## 9. Assessment (a): raw pointers in `TaggedValue`

**What changes [V for the current state, I for the work]:**

1. **Stable addresses.** Replace the four growable `Vec` arenas with block or segment memory that never relocates on growth: Chez's 16 KB segments with a `seginfo` per segment, recording space, generation, card dirty bytes and mark masks (`ChezScheme/s/cmacros.ss:2142-2145`; `c/types.h:143-185`), or Immix blocks and lines. Today's per-arena `MarkBits` index arithmetic (`gc.rs:88-124`) becomes per-block side bitmaps keyed by address.
2. **Encoding.**
   - Keep fixnum `000` and the 3-bit primary tags on 8-byte-aligned objects. Use 16-byte alignment if a fourth bit is wanted, as Chez does for pairs and flonums.
   - Untagging folds into the load displacement: `car` = `ldur x, [p, #-3]`.
   - Spare high bits (16 on 48-bit VA) could carry a subtype for header-free type tests. **[I]** Do not use them for ZGC-style colored pointers in an interpreter or baseline JIT: that puts a barrier on every load. Generational ZGC itself keeps roots on the stack and in registers "colorless" (JEP 439, https://openjdk.org/jeps/439).
3. **A compressed alternative (a′).**
   - Reserve a single virtual-address cage and keep the 61-bit payload as a byte offset: address = `base + raw - tag`.
   - JIT cost is one add or a base-register addressing mode. V8 uses a 4 GB-aligned cage with 32-bit compressed pointers ("reduces V8 heap size up to 43%", https://v8.dev/blog/pointer-compression). Wasmtime's GC references are "a 32-bit index into the GC heap's underlying linear memory", chosen for sandboxing and cache density (https://bytecodealliance.org/articles/wasmtime-gc).
   - It keeps a cheap bounds check available in Rust and stays close to today's index model. Compressing *fields* to 4 B would cost the 61-bit fixnum range, so the gain is mainly a fixed base and safety, not density.
4. **Memory safety.**
   - Today a stale reference (a rooting bug) is a logic error: bounds-checked, debug-poisoned pairs (`heap/mod.rs:718-727`), `Free` tombstones (`:1484-1493`).
   - With raw pointers it is undefined behaviour. Keep a debug-mode object-start bitmap and poisoned free memory, and keep the differential GC lanes (`docs/GC_DESIGN.md` §11) as the guard.
5. **`Drop` payloads.** Sixteen variants own Rust resources (§3). These need a finalization list, or must become GC-native (see (b)).
6. **Blast radius.**
   - `heap/mod.rs`, `numeric.rs` (7,495 lines including `gc.rs`) and the one VM `heap_index()` use.
   - The ≈749 borrow sites can stay as they are during migration, since `Heap` methods keep their signatures. They become dead weight rather than wrong.

**Gain [I]:** car, cdr, vector-ref and closure slot each become one load after the tag test. It also unlocks inline bump allocation (§10) and JIT field access.

---

## 10. Assessment (b): object headers with a type and size word

**Today [V]:** no headers. The type is implied by the arena, or by the enum discriminant hidden inside a fixed 72 B slot. Size is implied by the arena or by a `Vec` length in a separate buffer. Heap walking relies on segregation by type.

**Recommended direction, by object [I, based on prior art]:**
- **Pairs and flonums stay header-free**, in segregated blocks, as in Chez: pair = car and cdr, 16 B (`cmacros.ss:1431-1433`); flonum = 8 B data padded to 16 B (`:1505-1513`). Gambit's `___HEADER(obj)` macro reads `BODY0(obj)[-1]` (`gambit/include/gambit.h.in:3229`), which suggests memory-allocated objects there, pairs included, carry a header word. **[I]** The header-free pair is one reason Chez's pairs are smaller.
- **Typed objects get one header word**, as Chez's `type-typed-object` header holds subtype and length (`cmacros.ss:875-902,1467-1469`) and OCaml's header holds wosize, 2 color bits and an 8-bit tag (`ocaml/runtime/caml/mlvalues.h:153-169`).
- **Bits a Patina header needs:**
  - subtype (8 bits)
  - length (≥32 bits)
  - GC state: mark (or a side bitmap), forwarded, pinned, logged/remembered, age (2–3 bits)
  - **2 identity-hash state bits**, the Bacon–Fink–Grove "unhashed / hashed / hashed-and-moved" scheme (ECOOP 2002; from memory, not re-fetched). JDK compact headers keep a 31-bit hash, 4 age bits, a self-forwarded bit and 22-bit class pointers in 64 bits (JEP 450, https://openjdk.org/jeps/450).
- **Records:** header = RTD pointer, then inline fields, as Chez does (`cmacros.ss:1578-1580`). The RTD carries a pointer mask for tracing (`:1691-1699`). This removes the three allocations per record and the `Rc`-identity special case in `eq?`. RTDs then have one identity, which fixes the `%record-type-of` re-wrapping.
- **Closures:** a code reference plus inline free variables, as Chez does (`cmacros.ss:1528-1530`). Globals belong on the code object or in a per-library context, not as an `Rc<Environment>` per closure. That removes the `Rc` clone in every `LoadGlobal` and the per-closure environment dedup during tracing.
- **Strings and bytevectors:** inline payloads after the header (Chez `string` uses 4-byte `string-char`, `:1544-1546`). Keeping UTF-32 preserves O(1) `string-set!`.
- **`MutableCell`:** an 8 B box plus header (16 B against today's 72 B).
- **External resources** (ports, libraries, `CompiledMacro`, tree-walker continuations, prompt tags) become "foreign" objects of header plus `Rc` pointer, registered on a finalization list swept each cycle. MMTk and Whippet both expect the embedder to provide tracing, size and forwarding over such a header. MMTk lets metadata (mark, forwarding bits, log bit, pin bit) be in-header or side (https://docs.mmtk.io/api/mmtk/vm/trait.ObjectModel.html). Whippet notes forwarding can live in an initial tag word "via a specific pattern for the low 3 bits" (https://raw.githubusercontent.com/wingo/whippet/main/doc/manual.md).

**Cost [I]:**
- The layout change is mostly inside `patina-core`, since the enum is named 32 times outside it. The 28-arm `trace_object_children` becomes header dispatch.
- Each primitive that pattern-matches today (`get_record`, `get_vm_closure*`, `get_promise` and so on) gets an unsafe accessor.
- Data supporting the size win (§0): a `MutableCell` drops from 72 B to 16 B, a flonum from 72 B to 16 B, or to 0 B if immediate, and a 3-field record from slot + `Rc` + `Vec` (about 72 + 48 + 24+16 B) to 32 B.

---

## 11. Assessment (c): a moving collector

**Prerequisites, from §6–7 [I]:**
1. **A visitor that can write back.** Today it is `visit(TaggedValue)` by value (`gc.rs:485`). It must become `visit(&mut TaggedValue)`, or `&Cell<TaggedValue>`, everywhere. That requires interior mutability for `TaggedValue`s now inside immutable `Rc` structures: code constants (`code_object.rs:140`), `CpsExpr`/`CoreExpr` literals, `CompiledMacro` literals, `Library.exports`, `WindRecord.handlers: Rc<[H]>`, VM continuation snapshots (`Rc<VmContinuation>`), and tree-walker `ContValue`/`ContEnv`.
   - Alternative: allocate everything those structures can reference (literals, constants, symbols, RTDs, primitives) in a **non-moving old or immortal space**, so only young objects move. But environment slots and `Library.exports` receive fresh data all the time and must be updatable regardless. They already sit behind `RefCell`.
2. **Identity hash.** A stable hash in the header (§10), so `identity-hash` and the `equal-hash` fallback (`heap/mod.rs:2541-2561,2668-2670`) stop depending on location. The alternative is native eq-tables that the GC rehashes. Chez queues "tlc" entries on `tlcs_to_rehash` and rehashes them after collection (`ChezScheme/c/gc.c:301,1734-1773`). That only works if the GC knows the tables, and Patina's are Scheme vectors.
3. **Rekey or remove the raw-bit maps:** `syntax_sources`, `SourceMap` and the freed-bits feed. `symbol_table`, `core_syntax_table` and `CallFrame.closure` (§6, rows 1–6) need updating or a non-moving home.
4. **Finalization for `Drop` payloads** in dead young objects (§3, §7): a per-space list of objects with `Rc` owners, drained after evacuation.
5. **Precise roots everywhere.**
   - The VM is already precise: the whole register `Vec` plus per-pc `register_roots` retirement (`gc_roots.rs:42-64`), and continuation snapshots are `Vec`s.
   - Collection happens only at outermost safe points, with Rust locals excluded by `GcDeferGuard`, the same "mutation xor collection" rule as gc-arena (https://raw.githubusercontent.com/kyren/gc-arena/master/README.md).
   - The transient raw-bit sets (§6, row 12) stay safe under that rule.
   - For JIT frames, Cranelift's user stack maps spill declared values to stack slots at safepoints and reload them afterwards, "so the stack can be updated to facilitate moving GCs" (https://docs.rs/cranelift-frontend/latest/cranelift_frontend/struct.FunctionBuilder.html; https://fitzgen.com/2024/09/10/new-stack-maps-for-wasmtime.html).

**Payoff evidence [V].** The library-load census shows 99.2% of 93,241 objects and 99.94% of 156,104 pairs dead at the first collection. Nearly all of the dead objects are syntax `Identifier`s. A copying nursery would evacuate about 800 objects and reclaim the rest by resetting a bump pointer. Today the mark-sweep collector sweeps all 249k slots, the arenas keep their high-water capacity for good (§2), and free-list LIFO allocation scatters later allocations across the old high-water range.

**Recommended middle path [I].**
- Move only the young generation (precise roots, small copy volume).
- Make the old space non-moving: mark-region or Immix with side mark bits, close to today's `MarkBits`.
- Optionally defragment the old space opportunistically, skipping pinned objects. Whippet's `mmc` has per-object pinning and conservative or precise roots (https://raw.githubusercontent.com/wingo/whippet/main/README.md).
- Objects referenced from structures that cannot be updated are either allocated old (literals, constants) or pinned.
- This also unlocks bump-pointer inline allocation for the JIT, and turns the measured +34 ns flonum box into a bump plus header store until flonums become immediates.

**Float self-tagging (orthogonal, high value) [V].** Melançon, Serrano and Feeley (OOPSLA'25, https://arxiv.org/abs/2411.16544) rotate the float bits left by 4 so that the high exponent bits become the tag. Three 3-bit tags carry most doubles immediately; in Gambit's variant tags `011/110/111` are floats and `010` is the boxed fallback, with `000` kept for fixnums. They report 89% of mandelbrot floats immediate and a 2.3× speedup on float-heavy Scheme benchmarks. The encode is "3 register-to-register operations and an easily predictable conditional jump".

Patina's primary tags are crowded: pair, vector, string, a dead closure tag, and object. Adopting this means moving vector and string under headers and retiring `110`, which (b) does anyway.

---

## 12. Constraints and lessons for the redesign

1. **Keep the safe-point discipline.** Collection happens only at outermost safe points, and no Rust local holds a `TaggedValue` across a collection (`gc.rs:359-409`; design §7). Every raw-bit transient set and every `&HeapObjectData` borrow is sound only because of it, and it is what makes JIT-without-`RefCell` and moving roots feasible.
2. **Keep the precise VM roots and the per-pc register maps** (`code_object.rs:158`). They are the prototype for JIT stack maps.
3. **Plan for `Drop`.** Today correctness, including cycle breaking, depends on sweep running destructors of dead objects. Any design that skips dead objects needs finalization lists, or fewer `Rc` payloads inside GC objects.
4. **Identity hashing must be designed in up front.** User data already stores location-derived hashes (SRFI 69/125/128, R6RS).
5. **Account by bytes, not object count**, and return memory. Arenas never shrink, and malloc'd buffers are invisible to the trigger.
6. **The object layout change is local to `patina-core`.** The borrow protocol (≈749 sites) is the wide migration; it can stay as a shim while the representation changes underneath.
7. **Barrier coverage is two families:** `&mut Heap` setters and `RefCell`-through-`&Heap` setters (§8). There are few sites, but `vector_slice_mut` hands out bulk mutable slices, which suits card marking better than field logging.
8. **Off-heap environments are first-class roots.** Global stores are heap stores; a generational design needs either a barrier on environment slot writes or a full rescan of environments at every minor collection. The latter repeats the code-store-constants problem the stage-5 PRD measured at 57% of root tracing.
9. **Reuse what is free:** the dead `110` tag, the 29 zero high bits in a heap reference, and the 25 spare special codes (for example a broken-weak-pointer marker or unbound marker).

---

## 13. Open questions

1. Should environments, and the tree-walker's per-call frames, become GC heap objects? Today an `Environment` is 224 B and allocated per call. Keeping them as `Rc` structures keeps them as roots that must be rescanned or barriered.
2. Should the tree-walker be held to the same representation? Its `CpsExpr`/`ContValue` `Rc` graphs are the hardest holders to make updatable. Freezing it on a non-moving old-space-only path is a possible compromise.
3. Is the 61-bit fixnum range a requirement, or would 31-bit fields with 4-byte compressed slots, the V8 trade, be acceptable? The answer decides whether option (a′) buys density or only safety and a fixed base.
4. Should eq-hash tables become native and GC-aware, as in Chez, or should headers carry a stable hash? The second is simpler while tables remain Scheme code.
5. String representation: keep UTF-32 for O(1) `string-set!`, or move to UTF-8 with an index? This affects inline layout and memory, since strings are 4 B per char today.
6. How should `CodeObject` constants live under a JIT: as old-space objects referenced from machine code (needing relocation or pinning), or loaded through a constant table pointer?
7. Are `num-bigint` digits acceptable as malloc'd foreign payloads, or should bignums become inline GC objects with in-house arithmetic?
