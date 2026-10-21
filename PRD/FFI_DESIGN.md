# FFI System Design

**Status**: Proposed
**Date**: 2026-03-20
**Revised**: 2026-10-12, under the collector's contract (GC redesign stage 2, #694): see [Garbage Collection](#garbage-collection); 2026-10-21, §1.2–§1.5 for the host primitives of stage 3 (#710)
**Phase**: Phase 3 (post-VM, alongside `syntax-case`)

---

## Motivation

Patina is R7RS-complete but has no mechanism for users to extend it with native code. Every new capability (database access, networking, graphics, etc.) must be added as a built-in primitive and the interpreter recompiled. An FFI system would allow:

1. **Rust plugin authors** to package new primitives as reusable crates
2. **Scheme users** to call arbitrary C libraries at runtime without touching Rust
3. **Library ecosystem growth** without changes to the Patina core

---

## Design Overview: Two-Layer FFI

### Layer 1: Rust Plugin API (compile-time)

Extend Patina by writing Rust crates that register new Scheme primitives through the interpreter (`register_primitive`, `register_resumable`, #710), into the same registry the built-in primitives use. Works identically on both tree-walker and VM backends with zero backend-specific code.

### Layer 2: Dynamic C FFI (runtime)

Call arbitrary C shared libraries from Scheme at runtime via `libloading` (dlopen) + `libffi` (calling conventions). A `(patina ffi)` Scheme library provides Racket/Chez-style `ffi-lib` / `ffi-procedure` forms.

---

## Reference Implementations Studied

| Implementation | Approach | Key Mechanism |
|---|---|---|
| **Chibi-Scheme** | Stub compiler: `.stub` → C wrappers → `.so` → `dlopen` | `chibi-ffi` generates C glue; `sexp_init_library` entry point |
| **Chez Scheme** | `libffi` (portable bytecode) + native codegen; `foreign-procedure` | Runtime `dlopen` + libffi for calling conventions |
| **Gauche** | Dynamic FFI: C header parsing + `dlopen` + hand-rolled x86-64 ABI | `load-foreign` parses `.h`, resolves symbols by signature |
| **Racket** | `ffi/unsafe`: `ffi-lib` opens `.so`, `_fun` type descriptors | Most complete pure-Scheme FFI API; uses libffi internally |
| **Steel** (Rust Scheme) | Embedded Rust API + `cdylib` plugin loading via `libloading` | No C FFI; register Rust fns via `FromSteelVal`/`IntoSteelVal` |
| **Rhai** (Rust scripting) | Embedded Rust API + proc-macro `#[export_module]` | `engine.register_fn()` with trait-based auto-marshalling |

**Key takeaway**: C-based Schemes use stub generation or libffi. Rust-based interpreters (Steel, Rhai) all use an embedded Rust API as the primary extension mechanism. No Rust-based interpreter has a full dynamic C FFI yet. Patina can be the first to offer both layers.

---

## Layer 1: Rust Plugin API

### 1.1 Foreign Heap Object

New `HeapObjectData` variant for opaque Rust objects. The heap object holds an id, never the Rust object:

```rust
// patina-core/src/heap/mod.rs
HeapObjectData::Foreign {
    type_name: &'static str,          // e.g. "sqlite-connection"
    id: HostId,                       // the Rust object, in the heap's `HostPayloadTable`
}
```

The Rust object (`Box<dyn Any>`, with an optional finalizer) is a host payload. It lives in the heap's
`HostPayloadTable`, which stage 4f builds, and is registered in the finalization registry as `FinalKind::Foreign`.
Under the new heap no heap object owns a Rust value (`PRD/GC_PRD.md` §7, §9.6, §14). Its finalizer follows the rules
in [Garbage Collection](#garbage-collection).

Scheme-visible as `#<foreign sqlite-connection>`. Primitives:
- `(foreign? obj)` → `#t` if foreign object
- `(foreign-type obj)` → `"sqlite-connection"` (string)

### 1.2 The Plugin API

No crate of its own: a plugin depends on `patina-interpreter`, whose re-exports are what it names, and on
`patina-core` for the context and its values (stage 3, #710):

```rust
use patina_core::{Cx, Value};
use patina_interpreter::{
    Arity, Backend, EvalError, HostPrimitives, Interpreter, PrimClass, PrimSpec, RegisterError,
    // for a primitive that calls a procedure:
    ApplyContext, CallArgs, ResumableResume, ResumableStart, Step,
};
```

- **A primitive is a `Prim`:** `for<'gc> fn(&mut Cx<'gc>, &[Value<'gc>]) -> Result<Value<'gc>, EvalError>`, as a
  built-in one is (`PRD/GC_PRD.md` §11.3).
  - It reads, allocates and stores through the context the machine makes for its call. Beneath that context nothing
    collects and nothing calls Scheme.
  - Its arguments are values of that context, so it cannot keep one past the call (#709's compile-fail cases). What
    it keeps is plain data, or a host payload (§1.1).
- **A primitive that calls a procedure is resumable.**
  - `start` answers `Step::Call { callee, args, state }`, and the machine makes the call. `resume` runs with the
    `state` and what the call returned.
  - So a continuation captured in the callee and re-entered after the primitive returned resumes it, as chibi and
    Gauche resume such a call (#471).
- **`class` is `Leaf` or `Transfer`.** A `Leaf` never blocks; a `Transfer` may, and a resumable primitive always is
  one. The JIT treats every host primitive as allocating, so no class says `NoAlloc` (§11.5).

### 1.3 Plugin Registration

A plugin exposes a registration function, which the host calls on its interpreter:

```rust
// In a plugin crate (e.g. patina-sqlite):
pub fn register<B: Backend + HostPrimitives>(interp: &mut Interpreter<B>) -> Result<(), RegisterError> {
    let lib = ["myapp", "sqlite"];
    let blocking = |f, arity| PrimSpec { f, arity, class: PrimClass::Transfer };
    interp.register_primitive(&lib, "sqlite-open", blocking(sqlite_open, Arity::Exact(1)))?;
    interp.register_primitive(&lib, "sqlite-exec", blocking(sqlite_exec, Arity::Range(2, 3)))?;
    interp.register_primitive(&lib, "sqlite-close", blocking(sqlite_close, Arity::Exact(1)))?;
    Ok(())
}

fn sqlite_open<'gc>(cx: &mut Cx<'gc>, args: &[Value<'gc>]) -> Result<Value<'gc>, EvalError> {
    let path = cx
        .get_string_contents(args[0])
        .ok_or_else(|| EvalError::TypeError("sqlite-open: expected a string".into()))?;
    let db = rusqlite::Connection::open(&path).map_err(|e| EvalError::IOError(e.to_string()))?;
    // §1.1's foreign object, which stage 4f's host payloads bring. No finalizer: dropping the
    // connection closes it, after the collection that proves the object dead or at teardown.
    Ok(cx.alloc_foreign("sqlite-connection", Box::new(db), None))
}
```

Each of these blocks on the file system, so each is a `Transfer`. A host that keeps a large buffer for a small
object charges it through `Interpreter::external_bytes()`, so the heap collects as often as the buffers ask.

The host binary registers its plugins at startup:

```rust
let mut interp = patina_interpreter::VmInterpreter::new_vm();
patina_sqlite::register(&mut interp)?;
```

### 1.4 Scheme-Side Usage

Registration exports each primitive from its library, so a program imports the library itself, with no
`define-library` to write:

```scheme
(import (scheme base) (myapp sqlite))
(define db (sqlite-open "test.db"))
(sqlite-exec db "CREATE TABLE t (id INTEGER, name TEXT)")
(sqlite-close db)
```

`RegisterError` refuses three things:
- a library of the bundled namespaces (`scheme`, `srfi`, `patina`);
- a library loaded otherwise, or one a search path finds;
- a name the library already exports.

A program that imported the library before a later registration reaches only what was registered then.

### 1.5 Why This Works for Both Backends

- **Both machines implement `HostPrimitives`.** A registration appends the primitive to the machine's registry, and
  the library's namespace binds it under a qualified name that resolves to that entry.
- **A call then dispatches as a built-in primitive's does,** by the entry's index, through the context the machine
  makes (`ApplyContext::call_cx`, `step_cx`). That includes the VM's `CallPrimitive` fast path.
- **A resumable primitive's calls run as frames of the machine:** the VM's `resume_stub`, the tree-walker's
  `ResumePrimitive` continuation.
- **Each machine picks its control primitives** (`call/cc`, `force`, `apply` and the rest) by the qualified name of
  the binding they came from. So a plugin's primitive of the same name, in a library of its own, is its own (#710).

---

## Layer 2: Dynamic C FFI

### 2.1 Scheme API Design

Inspired by Racket's `ffi/unsafe` and Chez's `foreign-procedure`:

```scheme
(import (patina ffi))

;; Load a shared library
(define libm (ffi-lib "libm"))        ; or "libm.dylib", platform-resolved

;; Bind a C function with type signature
(define c-sin
  (ffi-procedure libm "sin"
    (list ffi:double)     ; argument types
    ffi:double))          ; return type

;; Call it
(c-sin 1.5708)  ; → ~1.0

;; Constants
(define libc (ffi-lib #f))  ; default: current process (libc symbols)
(define c-getpid (ffi-procedure libc "getpid" '() ffi:int))
(c-getpid)  ; → PID

;; Strings
(define c-strlen
  (ffi-procedure libc "strlen" (list ffi:string) ffi:size-t))
(c-strlen "hello")  ; → 5

;; Foreign pointers
(define c-malloc
  (ffi-procedure libc "malloc" (list ffi:size-t) ffi:pointer))
(define c-free
  (ffi-procedure libc "free" (list ffi:pointer) ffi:void))
```

### 2.2 Type System

Scheme-side type descriptors (symbols), mapped internally to `FfiType` enum:

```rust
// patina-ffi/src/types.rs
pub enum FfiType {
    Void,
    Bool,
    Int8, Uint8, Int16, Uint16,
    Int32, Uint32, Int64, Uint64,
    SizeT,            // platform-sized
    Float, Double,
    Pointer,          // opaque void*
    CString,          // null-terminated char*, auto-converts strings
}
```

Scheme symbol mapping:
| Symbol | FfiType | C type |
|---|---|---|
| `ffi:void` | `Void` | `void` |
| `ffi:bool` | `Bool` | `_Bool` / `int` |
| `ffi:int8` | `Int8` | `int8_t` |
| `ffi:uint8` | `Uint8` | `uint8_t` |
| `ffi:int16` | `Int16` | `int16_t` |
| `ffi:uint16` | `Uint16` | `uint16_t` |
| `ffi:int` / `ffi:int32` | `Int32` | `int32_t` |
| `ffi:uint32` | `Uint32` | `uint32_t` |
| `ffi:int64` | `Int64` | `int64_t` |
| `ffi:uint64` | `Uint64` | `uint64_t` |
| `ffi:size-t` | `SizeT` | `size_t` |
| `ffi:float` | `Float` | `float` |
| `ffi:double` | `Double` | `double` |
| `ffi:pointer` | `Pointer` | `void*` |
| `ffi:string` | `CString` | `char*` |

### 2.3 Marshalling Rules

| Scheme → C | Rule |
|---|---|
| fixnum → integer types | Extract i64, narrowing-checked |
| flonum → `float`/`double` | Extract f64, cast |
| `#t`/`#f` → `int` | 1/0 |
| string → `char*` | Copy to temporary `CString`, freed after call |
| bytevector → `void*` | Pointer to its data, which stays put for the call (pinning, under [Garbage Collection](#garbage-collection)) |
| foreign pointer → `void*` | Extract raw pointer |
| `'()` / `#f` → pointer | NULL |

| C → Scheme | Rule |
|---|---|
| integer types → fixnum | Widen to i64 |
| `float`/`double` → flonum | f64 |
| `int` (bool) → `#t`/`#f` | nonzero = #t |
| `char*` → string | Copy into Scheme string (caller keeps C pointer) |
| `void*` → foreign pointer | Wrap in `HeapObjectData::ForeignPointer` |
| `void` → unspecified | `TaggedValue::UNSPECIFIED` |

### 2.4 Implementation Crate

```
crates/patina-ffi/
├── Cargo.toml          # depends on libloading, libffi, patina-interpreter
├── src/
│   ├── lib.rs          # Library builder for (patina ffi)
│   ├── types.rs        # FfiType enum + parsing from Scheme symbols
│   ├── marshal.rs      # TaggedValue ↔ C value conversion
│   ├── call.rs         # libffi CIF construction + invocation
│   └── library.rs      # FfiLibrary (wraps libloading::Library as Foreign)
```

### 2.5 Rust Dependencies

| Crate | Purpose | Notes |
|---|---|---|
| `libloading` | `dlopen`/`dlsym` wrapper | Standard, ~50M downloads, no native deps |
| `libffi` | Runtime calling-convention dispatch | Wraps C libffi; requires system libffi or bundled build |

### 2.6 Safety Model

Dynamic C FFI is inherently unsafe. Incorrect type declarations cause UB. The design mitigates this:

1. **Type checking at binding time**: `ffi-procedure` validates the type descriptor list
2. **Arity enforcement**: generated closures check argument count
3. **String lifetime**: temporary `CString` copies are freed after the call returns
4. **No raw pointer arithmetic**: `ffi:pointer` values are opaque; only passable to other FFI calls
5. **Documentation**: clear warnings that `(patina ffi)` is "unsafe" like Racket's `ffi/unsafe`
6. **The collector's contract**: what C holds across a call is rooted and stays put, and a callback collects like any
   program ([Garbage Collection](#garbage-collection))

---

## Phase Plan

### Phase A: Foreign Object Foundation

**Goal**: `HeapObjectData::Foreign` + Scheme predicates

| Task | Crate | Effort |
|---|---|---|
| Add `Foreign` variant to `HeapObjectData`, holding its payload's id (§1.1) | patina-core | S |
| `alloc_foreign()`, `get_foreign()`, `get_foreign_mut()` on `Heap` | patina-core | S |
| Finalization through the registry, under F4, F5 and F7 ([Garbage Collection](#garbage-collection)) | patina-core | S |
| Its trace arm, named field by field, and its sentinel test (AGENTS.md, "New heap object type") | patina-core | S |
| Display as `#<foreign TYPE_NAME>` | patina-core | S |
| `foreign?` and `foreign-type` primitives | patina-primitives | S |
| `is_procedure()` returns `false` for foreign objects | patina-core | S |
| Tests, the first of the FFI lane ([Garbage Collection](#garbage-collection)) | patina-tests | S |

**Deliverable**: Foreign objects can be created from Rust, stored in Scheme variables, type-checked, and displayed.

### Phase B: Plugin API

**Goal**: the host-primitive API, which stage 3 of the GC redesign built (#710, §1.2–§1.5)

| Task | Crate | Effort |
|---|---|---|
| `register_primitive` and `register_resumable`, exporting from a host's library | patina-interpreter | done (#710) |
| `HostPrimitives` on both machines | patina-vm, patina-tree-walker | done (#710) |
| Integration tests: a host primitive, a resumable one re-entered, the refusals | patina-tests | done (#710, `host_primitives.rs`) |
| Example plugin crate with trivial primitives | a new example | S |
| Documentation: how to write a plugin | docs/ | S |

**Deliverable**: External Rust crates can define new Scheme libraries with native primitives. Both backends work.
The legacy `StandardPipeline` takes no registrations; a host moves to `Interpreter`.

### Phase C: Dynamic C FFI — Core

**Goal**: `(patina ffi)` with basic types

| Task | Crate | Effort |
|---|---|---|
| Create `patina-ffi` crate | patina-ffi | M |
| `FfiType` enum + parsing from Scheme symbols | patina-ffi | S |
| `ffi-lib` primitive (wraps `libloading::Library` as Foreign) | patina-ffi | M |
| `ffi-procedure` primitive (constructs libffi CIF, returns closure) | patina-ffi | L |
| Marshalling: integers, floats, booleans, void | patina-ffi | M |
| Marshalling: strings (CString temporaries) | patina-ffi | M |
| Marshalling: pointers (opaque void*) | patina-ffi | S |
| `ForeignPointer` heap object variant (raw `*mut ()`) | patina-core | S |
| Register as `(patina ffi)` Scheme library | patina-runtime | S |
| Tests: libm sin/cos/sqrt, libc getpid/strlen, in the FFI lane | patina-tests | M |

**Deliverable**: Scheme code can call C functions from system libraries with basic type support.

### Phase D: Dynamic C FFI — Advanced (Future)

**Goal**: Structs, callbacks, arrays

| Task | Effort |
|---|---|
| `ffi:struct` type descriptors with field layout | L |
| `ffi-struct-ref` / `ffi-struct-set!` | M |
| `ffi-callback` (Scheme closure → C function pointer via libffi closures): a driver entry, its procedure held by a global handle ([Garbage Collection](#garbage-collection)) | L |
| The callback event-loop probe in the FFI lane, for K16 | S |
| Bytevector pinning for array/buffer passing: a `PinToken` from stage 8, or the large-object space | M |
| `ffi-sizeof`, `ffi-alignof` | S |
| Memory management: `ffi-malloc`, `ffi-free` | S |

---

## Architectural Decisions

### D1: Foreign objects hold a `Box<dyn Any>` as a host payload

**Chosen**: `HeapObjectData::Foreign { type_name, id }`, whose id reaches the `Box<dyn Any>` and its optional finalizer
in the heap's `HostPayloadTable` (§1.1). Revised 2026-10-12: the box was a field of the heap object, but under the new
heap no heap object owns a Rust value (`PRD/GC_PRD.md` §7), and finalization is the registry's, not the sweep's (§9.6).

**Alternatives considered**:
- Type-erased `*mut ()` + manual vtable — more efficient but unsafe and error-prone
- Generic `HeapObjectData<T>` — impossible, `HeapObjectData` must be object-safe for the heap vector

**Rationale**: `dyn Any` gives safe downcasting via `downcast_ref::<T>()`. The `Box` is only allocated once per foreign object creation, not per access. The `type_name` field enables meaningful display and error messages without downcasting.

### D2: Compile-time plugin registration (not runtime cdylib)

**Chosen**: Plugins are Rust crates linked at compile time via `patina_plugins![]` or manual registration in the host binary's `main()`.

**Alternatives considered**:
- Runtime loading of Rust `.so` plugins via `libloading` — requires `abi_stable` or `#[repr(C)]` ABI boundary; fragile across compiler versions
- WASM plugins — interesting but heavy; more appropriate for Phase 5+

**Rationale**: Compile-time linking is simple, safe, and idiomatic Rust. The main cost (recompilation) is acceptable because plugin authors already have a Rust toolchain. Runtime Rust plugins can be added later as an opt-in feature.

### D3: libffi for dynamic calling conventions (not hand-rolled)

**Chosen**: Use the `libffi` crate for runtime function call dispatch.

**Alternatives considered**:
- Hand-rolled x86-64 ABI dispatch (like Gauche) — platform-specific, massive maintenance burden
- No dynamic FFI, only Rust plugins — limits usability for non-Rust users

**Rationale**: libffi is battle-tested, cross-platform (x86, ARM, RISC-V), and handles variadics, struct returns, etc. The system dependency is acceptable — it's available on all major platforms and can be bundled.

### D4: Racket-style API (not Chez-style macros)

**Chosen**: Procedural API — `ffi-lib`, `ffi-procedure` are regular procedures that return callable closures.

**Alternatives considered**:
- Chez-style `foreign-procedure` macro — requires compile-time knowledge of types; doesn't work with runtime-computed type lists
- Chibi-style stub files — doesn't make sense for a Rust-based interpreter

**Rationale**: A procedural API is more flexible (types can be computed at runtime) and doesn't require special compiler support. It also composes well with the library system.

### D5: ForeignPointer is separate from Foreign

**Chosen**: Two distinct concepts:
- `Foreign { data: Box<dyn Any> }` — Rust-managed objects (database handles, etc.)
- `ForeignPointer(*mut ())` — Raw C pointers from FFI calls

**Rationale**: Foreign objects are safe (Rust-managed, type-checked via `Any`). Foreign pointers are inherently unsafe (raw addresses from C). Keeping them separate prevents accidentally treating a raw C pointer as a safe Rust object or vice versa.

---

## Interaction with Existing Systems

### Library System

Plugins register primitives under a qualified library name (e.g., `"myapp.sqlite/open"`). The existing `LibraryRegistry` + `.sld` file system works unchanged. A plugin's `.sld` file just needs the primitive names in its `(export ...)` clause — the Rust code has already installed the implementations.

### Macro System

No interaction. FFI primitives are regular procedures, not special forms.

### Garbage Collection

The collector runs on both backends, on by default. Its redesign (`PRD/GC_PRD.md`, tracked on #647) sets the rules a
plugin and the C FFI live by. A plugin that breaks one frees a value it still uses, or keeps a heap alive after its
interpreter.

These are the redesign's stage-2 rules (#694). Stage 3 changes two things, and its update of this document rewrites
§1.2–§1.5 to match:
- it registers plugins through `register_primitive`;
- it gives a primitive `Cx` in place of `&SharedHeap` (`PRD/GC_PRD.md` §11.5, §19).

| Rule | Content | `PRD/GC_PRD.md` |
|---|---|---|
| Values past a call | A `TaggedValue` is valid for the call it was passed to: nothing collects inside a call that does not hand control back to the machine. What a plugin keeps past the call, in its own state or in a C library's user data, is an `Owned` handle (`Heap::hold`, #605), which roots its value until the handle drops. It is never a `TaggedValue` or a `HeapIndex`, which a later collection frees and reuses. Nor is it the `SharedHeap`, whose `Rc` would keep a dropped interpreter's heap alive (#604). A handle holds its heap weakly: one kept past teardown holds nothing, and dropping it does nothing | §11.3, §11.5 |
| No encoding | A plugin never sees `HeapIndex` or how a value is encoded, and the prelude does not export `HeapIndex` | §11.5 |
| Calls back into Scheme | A plugin primitive that calls a procedure the program passed it hands the call to the machine, as a resumable primitive's `Step::Call`, and is resumed with the result (AGENTS.md, "A procedure that calls back into the program"). `define_ho`'s handlers call from Rust instead, on a nested loop that cannot collect (#622). Stage 3's `register_resumable` replaces them | §11.5 |
| `Foreign` finalization | The Rust object is dropped, or its finalizer run, after the collection that proves its `Foreign` dead and before Scheme resumes, and at teardown (F5). Its finalizer runs no Scheme and allocates nothing (F4). It also never blocks (F7): one that may, such as `sqlite3_close`, is handed to the blocking-I/O helper pool or runs in a safe region, never in the poll slow path, where it would stall every green thread. F7 binds from stage 9; until then a finalizer that blocks stalls only the program's one thread | §9.6 |
| Pinning | A buffer lent to C stays put for the call. Nothing moves before evacuation (stage 8), and the large-object space never moves; it holds every bytevector over 8 KiB and most buffers lent to C. From stage 8 an FFI call pins a buffer in the small-object space with a `PinToken`, which keeps its block out of evacuation. Across a call that may call back, and past the call, an `Owned` handle holds the buffer as well: the handle roots it, and the pin keeps it in place | §7, §9.4 |
| Callbacks as driver entries | A synchronous C→Scheme callback is a driver entry, like `Interpreter::call`. The FFI call that may call back is a `Transfer`: it marshals its arguments first, and holds only `Owned` handles and `PinToken`s across the C call. The callback enters the machine behind a continuation barrier, since a continuation captured inside it cannot be reinstated once the C frame has returned. A C event loop that calls Scheme for ever therefore collects like any program. Until callbacks are built that way, one runs beneath the FFI primitive under `NoGcScope`, a K16 site, in the interpreter tier only | §11.3 |
| Global handles for callbacks | The procedure behind a callback is held by an `Owned` handle in the libffi closure's user data, the JNI global-reference model. The handle roots the procedure, and what the procedure reaches, for as long as C holds the function pointer. Freeing the callback drops the handle | §11.3, §11.5 |
| `ForeignPointer` | A raw C address: never traced, never followed | — |

**The FFI lane.** Phase A's tests start a lane of their own, which Phases C and D extend.
- **Both backends, under `PATINA_GC_STRESS`.** Stress collects often enough that a value an FFI path forgot to root is
  freed while the path still holds it. The stale-reference checks of debug and `gc-check` builds turn its next use into
  a panic (#621).
- **Finalization tests.** They observe finalization, so they stay outside the byte-identical differential lane
  (`PRD/GC_PRD.md` §9.6).
- **Phase D's K16 probe.** Phase D adds the probe K16 names for FFI callbacks: a C event loop that calls a Scheme
  callback many times, each call allocating garbage. It reads `GcStats`' high-water marks per site, with K16's line
  as the pass mark (`PRD/GC_PRD.md` §15, §20):
  - the bytes allocated since the last serviced poll;
  - the bytes allocated under one `NoGcScope`.

  K16's line is the larger of 64 MiB and 25% of the pacing target. A callback under `NoGcScope` crosses it once the
  loop runs long enough, and K16's remedy is the driver entry above.

### WASM Target (Future)

Layer 1 (Rust plugins) works unchanged in WASM — it's just compiled Rust.
Layer 2 (Dynamic C FFI) is not available in WASM (no `dlopen`). The `patina-ffi` crate would be gated behind `#[cfg(not(target_arch = "wasm32"))]`.

---

## Open Questions

1. **Plugin `.sld` discovery**: Should plugin `.sld` files be auto-generated, or must the user write them? Current lean: user writes `.sld` that just lists exports; the primitive implementations are registered by Rust.

2. **Foreign object identity**: Should two foreign objects wrapping the "same" C resource be `eq?`? Current lean: no — `eq?` compares heap indices, so each `alloc_foreign` call produces a distinct identity. Users who need identity should keep a single Scheme reference.

3. **Thread safety**: `Foreign { data: Box<dyn Any> }` is `!Send` (because `dyn Any` is `!Send`). If Patina ever adds threads, foreign objects would need `Box<dyn Any + Send>`. Current lean: defer; make it `Box<dyn Any>` now, gate on a feature flag later.

4. **Error reporting for FFI calls**: Should C function call failures (segfault, etc.) be caught? Current lean: no — like Racket's `ffi/unsafe`, dynamic FFI is explicitly unsafe. Document clearly.
