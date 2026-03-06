# VM Backend Design: Code Sharing Strategy

**Status**: Planning
**Created**: 2026-03-05
**Context**: Phase 1 complete. This document defines how the VM backend (`patina-vm`) will share
code with the tree-walker, what needs to move, and what stays backend-specific.

---

## 1. What We Already Have: TaggedValue is the Key Enabler

The TaggedValue NaN-boxing migration (Nov 2025) was precisely the foundation needed for a VM.
Every value is a uniform 8-byte `Copy` word. The VM instruction set will operate on a stack
of `TaggedValue`s and write `TaggedValue` results — no boxing/unboxing boundary between frontend
and backend.

**Already fully shared and VM-ready (no changes needed):**

| Component | Location | Notes |
|-----------|----------|-------|
| `TaggedValue` + `Heap` | `patina-core` | The value representation the VM stack operates on |
| `Environment` | `patina-core` | Variable scoping; VM uses it for globals and closures |
| `CoreExpr` IR | `patina-core` | The compilation source for the VM |
| `CpsContinuation` | `patina-core` | call/cc representation; reused by VM for first-class continuations |
| `DynamicWindRecord` | `patina-core` | dynamic-wind tracking; backend-agnostic |
| `Scope`/`ScopeSet` | `patina-core` | Hygiene; only needed at compile time |
| Lexer + Parser | `patina-frontend` | Produce TaggedValue AST; no changes needed |
| Macro expander | `patina-macros` | Produces expanded AST; no changes needed |
| Desugarer | `patina-frontend` | Produces `CoreExpr`; no changes needed |
| `ExprVisitor` | `patina-ir` | IR analysis/transform passes; VM compiler uses this |
| Library system | `patina-runtime` | `.sld` loading and caching; completely backend-agnostic |
| `Backend` trait | `patina-runtime` | VM implements the same trait as the tree-walker |
| `Interpreter<B>` API | `patina-interpreter` | Generic over backend; `Interpreter<VmBackend>` just works |

The full frontend pipeline (source → `CoreExpr`) is 100% shared. The VM receives `CoreExpr`
at the same handoff point that the tree-walker does today.

---

## 2. The One Coupling: `TaggedHandler` Takes `&Evaluator`

The only significant coupling is primitive handlers:

```rust
// In patina-tree-walker/src/eval/primitives/registry.rs
pub type TaggedHandler = fn(&Evaluator, Vec<TaggedValue>, bool) -> Result<EvalResult, EvalError>;
```

This means `PrimitiveRegistry` and all 296 primitive implementations currently live in
`patina-tree-walker` and depend on the tree-walker's `Evaluator`. A VM can't use them directly.

**Analysis of what primitives actually use from `Evaluator`:**

| Usage | Primitives | VM solution |
|-------|-----------|-------------|
| `evaluator.global_env.heap()` | ~290/296 | Change signature to accept `&SharedHeap` directly |
| `evaluator.apply(proc, args, ...)` | `list-sort`, `member`/`assoc`, `string-map/for-each`, `vector-map/for-each`, `call-with-values`, `make-parameter`, `force` | Move to Scheme OR use `ApplyContext` trait |

**Two distinct groups:**

**Group A — heap-only primitives (~290/296):**
Most arithmetic, string, vector, list, I/O, character, and type-predicate operations only
need the heap. These are trivially shareable by changing the handler signature.

**Group B — higher-order primitives (~6/296):**
These call `evaluator.apply()` to invoke a Scheme callback (e.g. `list-sort`'s comparator,
`string-map`'s mapper, `force`'s thunk). These have a fundamental dependency on how
procedure application works — they need a backend-specific dispatch mechanism.

---

## 3. Architecture: Three-Layer Primitive Strategy

### Layer 1: Shared primitive implementations → `patina-primitives` (new crate)

Move the 290 heap-only primitive implementations. The handler signature changes:

```rust
// Before (tree-walker specific)
pub type TaggedHandler = fn(&Evaluator, Vec<TaggedValue>, bool) -> Result<EvalResult, EvalError>;

// After (backend-agnostic)
pub type TaggedHandler = fn(&SharedHeap, Vec<TaggedValue>) -> Result<TaggedValue, EvalError>;
```

`PrimitiveRegistry` moves to this crate. Both tree-walker and VM register the same primitives
and use the same registry.

### Layer 2: `ApplyContext` trait for higher-order primitives

```rust
// In patina-runtime or patina-primitives
pub trait ApplyContext {
    fn heap(&self) -> &SharedHeap;
    fn apply(&self, proc: TaggedValue, args: Vec<TaggedValue>) -> Result<TaggedValue, EvalError>;
}
```

The 6 higher-order primitives get a separate handler type:
```rust
pub type HOTaggedHandler = fn(&dyn ApplyContext, Vec<TaggedValue>) -> Result<TaggedValue, EvalError>;
```

Each backend provides its own `ApplyContext` implementation. The tree-walker wraps `Evaluator`,
the VM wraps `VirtualMachine`.

### Layer 3: CPS-native primitives stay in tree-walker

Some primitives (`call-with-values`, `dynamic-wind`, `call/cc`) are "CPS-sensitive" — the
CPS evaluator handles them specially via `apply_cps_step`. These are NOT in the registry at
all; they're matched by name in `apply_cps_step`. The VM will have its own analogous
dispatch for these.

---

## 4. `Procedure` Polymorphism: CpsLambda vs BytecodeLambda

Currently `HeapObjectData::Procedure` stores `Procedure::CpsLambda`:

```rust
pub enum Procedure {
    Primitive { name, arity, qualified_name },
    CpsLambda { params, variadic, cont_param, body: Rc<CpsExpr>, env, binding_scope },
    Continuation(Rc<CpsContinuation>),
    // ...
}
```

`CpsLambda` stores a `CpsExpr` body — tree-walker specific. The VM needs a different variant.

**Solution: backend-specific lambda variant via HeapObjectData**

Add a new `HeapObjectData` variant:

```rust
// In patina-core (shared)
pub enum HeapObjectData {
    // ... existing variants ...
    Procedure(Rc<Procedure>),
    // New: compiled closure (opaque to patina-core; interpreted by the VM)
    CompiledClosure(Rc<dyn std::any::Any>),
}
```

Or more concretely, keep `Procedure` as-is but add a VM-specific variant:

```rust
pub enum Procedure {
    Primitive { name, arity, qualified_name },
    CpsLambda { ... },              // tree-walker
    BytecodeLambda {                // VM
        code_ref: CodeRef,          // index into a code table
        env: Rc<Environment>,       // captured environment
        params: Formals,
    },
    Continuation(Rc<CpsContinuation>),
    NativeClosure(Rc<dyn Fn(Vec<TaggedValue>) -> Result<TaggedValue, EvalError>>),
}
```

The tree-walker ignores `BytecodeLambda`; the VM ignores `CpsLambda`. Both read `Primitive`
and `Continuation` the same way. The `NativeClosure` variant is useful for both.

---

## 5. VM Instruction Set Design

The VM compiles `CoreExpr` to a flat bytecode. Key design decisions driven by TaggedValue:

**Stack machine** (simpler to compile to than register machine; optimization later):
- Stack of `TaggedValue` — every slot is 8 bytes
- No type-tagging needed at the stack level (already in the value)
- Primitives pop args, push result in-place

**Instruction set sketch:**

```
; Value loading
LoadLit  idx          ; push constants[idx] (TaggedValue)
LoadVar  name_idx     ; push env.get(names[name_idx])
LoadGlobal name_idx   ; push global_env.get(names[name_idx])

; Closures
MakeClosure code_ref  ; pop N values, wrap with current env → BytecodeLambda
                      ; N = arity of code_ref's free variables

; Control flow
Jump     offset
JumpIf   offset       ; pop, jump if truthy
JumpIfNot offset      ; pop, jump if falsy

; Calls
Call     argc         ; pop argc args + func, apply
TailCall argc         ; same but reuse current frame (TCO)
CallPrim name_idx     ; fast-path for primitives (no frame setup)
Return               ; pop result, restore frame

; Environment
Define   name_idx     ; pop value, env.define(name, value)
Set      name_idx     ; pop value, env.set(name, value)

; Continuations (for call/cc)
CaptureCC            ; push a Continuation wrapping current frame stack
InvokeCont           ; restore frame stack from Continuation, push value

; Multiple values
MakeValues n         ; pop n, create Values object
UnpackValues         ; split Values onto stack
```

**TCO** is explicit in the instruction set (`TailCall` vs `Call`) — the compiler marks tail
positions in `CoreExpr` during compilation. The `ExprVisitor` trait is useful here for
a tail-position marking pass.

---

## 6. Migration Path: Two Backends Side-by-Side

The goal is to run both backends simultaneously so the test suite validates the VM against
the tree-walker's known-correct output.

```
Phase 2a: Infrastructure
  - Create patina-primitives crate; move 290 heap-only primitives there
  - Add ApplyContext trait; refactor 6 higher-order primitives
  - Both tree-walker AND new patina-vm link against patina-primitives
  - All existing tests still pass (tree-walker unchanged)

Phase 2b: VM compiler (CoreExpr → Bytecode)
  - New patina-vm/src/compiler.rs: walk CoreExpr with ExprVisitor
  - Emit bytecode to a function object (Vec<Instruction> + constants)
  - No execution yet — just verify the compiler produces valid bytecode

Phase 2c: VM execution loop
  - patina-vm/src/vm.rs: fetch-decode-execute loop
  - Start with non-optimized: no TCO, no call/cc
  - Run R7RS compliance tests; compare output against tree-walker

Phase 2d: First-class continuations in VM
  - Implement CaptureCC/InvokeCont instructions
  - Reuse CpsContinuation struct for captured state
  - Enable call/cc, dynamic-wind, exception handlers

Phase 2e: Performance parity and beyond
  - Profile VM; add specialized fast paths (numeric fixnum operations)
  - Add superinstructions for common sequences
  - Target: 5-10x faster than tree-walker baseline (832ms fib(25) → <200ms)
```

---

## 7. What the Test Suite Gets for Free

Because `Interpreter<B: Backend>` is generic and `patina-tests` creates
`Interpreter<TreeWalker>`, adding a second `Interpreter<VmBackend>` is zero-effort:

```rust
// In patina-tests, run every test against both backends:
fn make_tw() -> impl Interpreter { TreeWalkInterpreter::new_tree_walker() }
fn make_vm() -> impl Interpreter { VmInterpreter::new_vm() }

assert_eval_to_with(make_tw(), expr, expected);
assert_eval_to_with(make_vm(), expr, expected);
```

The 1400+ existing tests become the VM's acceptance test suite automatically.

---

## 8. Summary: What to Build in Phase 2

| Work Item | Crate | Blocks |
|-----------|-------|--------|
| `patina-primitives` crate | new | VM can call same primitives |
| `ApplyContext` trait | `patina-runtime` | Higher-order primitives |
| `BytecodeLambda` Procedure variant | `patina-core` | VM closure creation |
| Bytecode instruction set | `patina-vm` | Everything |
| CoreExpr → Bytecode compiler | `patina-vm` | VM execution |
| Fetch-decode-execute loop | `patina-vm` | Passing tests |
| `Backend` impl for VM | `patina-vm` | `Interpreter<VmBackend>` |
| `VmInterpreter` type alias | `patina-interpreter` | User-facing API |

**Estimated leverage from sharing**: ~80% of the codebase is reused.
The VM writes ~20% new code: compiler, execution loop, and instruction set.
