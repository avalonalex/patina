# FFI System Design

**Status**: Proposed
**Date**: 2026-03-20
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

Extend Patina by writing Rust crates that register new Scheme primitives. Leverages the existing `PrimitiveFn` / `TaggedHandler` / `HOTaggedHandler` infrastructure. Works identically on both tree-walker and VM backends with zero backend-specific code.

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

New `HeapObjectData` variant for opaque Rust objects:

```rust
// patina-core/src/heap/mod.rs
HeapObjectData::Foreign {
    type_name: &'static str,          // e.g. "sqlite-connection"
    data: Box<dyn Any>,               // the actual Rust object
    finalizer: Option<fn(&mut dyn Any)>,  // optional cleanup
}
```

Scheme-visible as `#<foreign sqlite-connection>`. Primitives:
- `(foreign? obj)` → `#t` if foreign object
- `(foreign-type obj)` → `"sqlite-connection"` (string)

### 1.2 Plugin Crate API (`patina-plugin`)

New thin crate re-exporting core types + providing `LibraryBuilder`:

```rust
// patina-plugin/src/lib.rs
pub mod prelude {
    pub use patina_core::{TaggedValue, SharedHeap, HeapIndex};
    pub use patina_runtime::{Arity, EvalError, Environment};
    pub use patina_primitives::{
        PrimitiveFn, TaggedHandler, HOTaggedHandler, ApplyContext,
    };
    pub use crate::LibraryBuilder;
}

pub struct LibraryBuilder {
    library_name: Vec<String>,
    primitives: Vec<PrimitiveFn>,
}

impl LibraryBuilder {
    pub fn new(name: &[&str]) -> Self;

    /// Register a heap-only primitive (most common)
    pub fn define(
        &mut self,
        name: &'static str,
        arity: Arity,
        handler: TaggedHandler,
    );

    /// Register a higher-order primitive (needs eval callback)
    pub fn define_ho(
        &mut self,
        name: &'static str,
        arity: Arity,
        handler: HOTaggedHandler,
    );

    /// Finalize: returns primitives for registration
    pub fn build(self) -> Vec<PrimitiveFn>;
}
```

### 1.3 Plugin Registration

Plugins expose a registration function. The host binary collects them:

```rust
// In a plugin crate (e.g. patina-sqlite):
use patina_plugin::prelude::*;

pub fn register(builder: &mut LibraryBuilder) {
    builder.define("sqlite-open", Arity::Exact(1), sqlite_open);
    builder.define("sqlite-exec", Arity::Range(2, 3), sqlite_exec);
    builder.define("sqlite-close", Arity::Exact(1), sqlite_close);
}

fn sqlite_open(heap: &SharedHeap, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError> {
    let path = heap.borrow().get_string_as_utf8(args[0]);
    let db = rusqlite::Connection::open(&path)
        .map_err(|e| EvalError::IOError(e.to_string()))?;
    let ptr = heap.borrow_mut().alloc_foreign(
        "sqlite-connection",
        Box::new(db),
        Some(|data| { data.downcast_mut::<rusqlite::Connection>(); /* drop */ }),
    );
    Ok(ptr)
}
```

Host binary registers plugins at startup:

```rust
// In patina-repl or user's binary:
fn register_plugins(registry: &mut PrimitiveRegistry, heap: &SharedHeap) {
    // Static plugin list — compiled into binary
    let mut builder = LibraryBuilder::new(&["myapp", "sqlite"]);
    patina_sqlite::register(&mut builder);
    for prim in builder.build() {
        registry.register(prim);
    }
}
```

### 1.4 Scheme-Side Usage

```scheme
(define-library (myapp sqlite)
  (import (scheme base))
  ;; Primitives auto-available from Rust plugin registration
  (export sqlite-open sqlite-exec sqlite-close))

;; User code:
(import (myapp sqlite))
(define db (sqlite-open "test.db"))
(sqlite-exec db "CREATE TABLE t (id INTEGER, name TEXT)")
(sqlite-close db)
```

### 1.5 Why This Works for Both Backends

Both tree-walker and VM dispatch primitives through the same path:

```
PrimitiveRegistry.apply_tagged(qualified_name, args, &dyn ApplyContext)
  → PrimitiveHandler::Heap(handler)(heap, args)
  → PrimitiveHandler::HigherOrder(handler)(ctx, args)
```

No backend-specific code is needed. The `ApplyContext` trait abstracts over tree-walker's `Evaluator` and VM's `VmApplyContext`.

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
| bytevector → `void*` | Pointer to raw data (pinned for call duration) |
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
├── Cargo.toml          # depends on libloading, libffi, patina-plugin
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

---

## Phase Plan

### Phase A: Foreign Object Foundation

**Goal**: `HeapObjectData::Foreign` + Scheme predicates

| Task | Crate | Effort |
|---|---|---|
| Add `Foreign` variant to `HeapObjectData` | patina-core | S |
| `alloc_foreign()`, `get_foreign()`, `get_foreign_mut()` on `Heap` | patina-core | S |
| Display as `#<foreign TYPE_NAME>` | patina-core | S |
| `foreign?` and `foreign-type` primitives | patina-primitives | S |
| `is_procedure()` returns `false` for foreign objects | patina-core | S |
| Tests | patina-tests | S |

**Deliverable**: Foreign objects can be created from Rust, stored in Scheme variables, type-checked, and displayed.

### Phase B: Plugin API

**Goal**: `patina-plugin` crate + `LibraryBuilder`

| Task | Crate | Effort |
|---|---|---|
| Create `patina-plugin` crate with `LibraryBuilder` | patina-plugin | M |
| Plugin registration hook in `StandardPipeline` | patina-pipeline | S |
| Plugin registration hook in `Interpreter` | patina-interpreter | S |
| Example plugin: `(patina example)` with trivial primitives | patina-plugin | S |
| Integration test: load plugin library from Scheme | patina-tests | S |
| Documentation: how to write a plugin | docs/ | S |

**Deliverable**: External Rust crates can define new Scheme libraries with native primitives. Both backends work.

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
| Tests: libm sin/cos/sqrt, libc getpid/strlen | patina-tests | M |

**Deliverable**: Scheme code can call C functions from system libraries with basic type support.

### Phase D: Dynamic C FFI — Advanced (Future)

**Goal**: Structs, callbacks, arrays

| Task | Effort |
|---|---|
| `ffi:struct` type descriptors with field layout | L |
| `ffi-struct-ref` / `ffi-struct-set!` | M |
| `ffi-callback` (Scheme closure → C function pointer via libffi closures) | L |
| Bytevector pinning for array/buffer passing | M |
| `ffi-sizeof`, `ffi-alignof` | S |
| Memory management: `ffi-malloc`, `ffi-free` | S |

---

## Architectural Decisions

### D1: Foreign objects use `Box<dyn Any>` in heap

**Chosen**: `HeapObjectData::Foreign { type_name, data: Box<dyn Any>, finalizer }`

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

### GC (Future)

When GC is added, foreign objects will need to participate:
- `Foreign` objects: register `finalizer` as GC destructor callback
- `ForeignPointer`: no GC interaction (raw pointers are not traced)
- Callbacks (Phase D): prevent collected closures from being called by C

This is deferred but the `finalizer: Option<fn(&mut dyn Any)>` field is designed with GC in mind.

### WASM Target (Future)

Layer 1 (Rust plugins) works unchanged in WASM — it's just compiled Rust.
Layer 2 (Dynamic C FFI) is not available in WASM (no `dlopen`). The `patina-ffi` crate would be gated behind `#[cfg(not(target_arch = "wasm32"))]`.

---

## Open Questions

1. **Plugin `.sld` discovery**: Should plugin `.sld` files be auto-generated, or must the user write them? Current lean: user writes `.sld` that just lists exports; the primitive implementations are registered by Rust.

2. **Foreign object identity**: Should two foreign objects wrapping the "same" C resource be `eq?`? Current lean: no — `eq?` compares heap indices, so each `alloc_foreign` call produces a distinct identity. Users who need identity should keep a single Scheme reference.

3. **Thread safety**: `Foreign { data: Box<dyn Any> }` is `!Send` (because `dyn Any` is `!Send`). If Patina ever adds threads, foreign objects would need `Box<dyn Any + Send>`. Current lean: defer; make it `Box<dyn Any>` now, gate on a feature flag later.

4. **Error reporting for FFI calls**: Should C function call failures (segfault, etc.) be caught? Current lean: no — like Racket's `ffi/unsafe`, dynamic FFI is explicitly unsafe. Document clearly.
