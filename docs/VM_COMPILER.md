# Patina VM: Compiler Design

**Status:** Implemented — Phase 2A complete
**Input:** `CoreExpr` (from `patina-frontend` desugarer)
**Output:** `CodeObject` (bytecode for `patina-vm`)
**See also:** [VM_ISA.md](./VM_ISA.md), [VM_DECISIONS.md](./VM_DECISIONS.md)

---

## 1. Design Philosophy

The compiler is structured as a **sequence of independent passes**, each transforming
one IR into the next. This mirrors Racket BC's multi-pass pipeline.

Each pass is a pure function. No shared mutable state between passes.
Intermediate IRs can be inspected for debugging.

---

## 2. Pipeline Overview

```
CoreExpr          (from patina-frontend)
    │
    ▼
 Pre-pass A: Quasiquote Expansion (quasiquote_expand.rs)
    │  Expands Quasiquote(TaggedValue) → list/cons/append calls
    │
    ▼
 Pre-pass B: Alpha Rename (alpha_rename.rs)
    │  Resolves variables using scope-set subset rules
    │  Renames all lambda params to unique names
    │  Detects internal defines, adds them to scope frames
    │
    ▼
 Pass 1: Analysis (pass1_analysis.rs)
    │  AnalysisInfo — free vars, mutations, internal defines per lambda
    │
    ▼
 Pass 2: Closure Conversion (pass2_closure.rs)
    │  ClosedExpr — flat capture lists, MutableCell boxing
    │  Internal defines → SetLocal/WriteLocalCell
    │
    ▼
 Pass 3: Tail Position Marking (pass3_tail.rs)
    │  TailedExpr — every App/Apply marked tail or non-tail
    │
    ▼
 Pass 4: Register Allocation (pass4_registers.rs)
    │  RegExpr — every binding/temporary assigned a register
    │
    ▼
 Pass 5: Code Generation (pass5_codegen.rs)
       CodeObject — Vec<Instruction>, constant pool, source map
```

The entry point is `compile_with_qq()` in `backend.rs`, which chains quasiquote
expansion → alpha rename → 5 passes.

---

## 3. Input: `CoreExpr`

The compiler receives `CoreExpr` from the `patina-frontend` desugarer.
The 13 `CoreExprKind` variants:

```
Literal, Var, Quote, Quasiquote, Lambda, If, Set, Begin,
Define, Import, Expand, App, Apply
```

- `Import` is intercepted before compilation in `Backend::eval()`
- `Quasiquote` is expanded by the compiler's own pre-pass (not the desugarer)
- `Expand` is handled during desugaring

---

## 4. Pre-pass A — Quasiquote Expansion

**File:** `quasiquote_expand.rs`
**Input:** `CoreExpr`
**Output:** `CoreExpr` with `Quasiquote(TaggedValue)` nodes replaced by
`App` calls to `list`, `cons`, `append`.

Interns plain symbols for nested `quasiquote`/`unquote`/`unquote-splicing`
markers (not raw identifiers with scope marks).

---

## 5. Pre-pass B — Alpha Rename

**File:** `alpha_rename.rs`
**Input:** `CoreExpr` (after quasiquote expansion)
**Output:** `CoreExpr` with all variables uniquely renamed, scope sets cleared

This pass bridges the gap between the tree-walker's runtime scope-set resolution
and the VM's compile-time variable resolution:

1. Walks the `CoreExpr` tree, building an environment of scoped bindings
2. Resolves `Var` references using `binding.scopes ⊆ reference.scopes` rules
   (matching tree-walker's `get_with_scopes` semantics)
3. Distinguishes "simple" bindings (non-macro params) from "scoped" bindings
   (macro-introduced params)
4. Uses `binding_scope` from Lambda nodes to give non-macro params proper scopes
5. Detects internal `define` forms and adds them to the current lambda's scope frame
6. Renames all parameters to unique names; clears scope sets in output

```rust
struct Binding {
    name: Symbol,
    scopes: ScopeSet,
    is_simple: bool,
    unique_name: Symbol,
}
```

---

## 6. Pass 1 — Analysis

**File:** `pass1_analysis.rs`
**Input:** `CoreExpr` (alpha-renamed)
**Output:** `CoreExpr` (unchanged) + `AnalysisInfo` side table

### 6.1 Data Structures

```rust
pub struct LambdaInfo {
    /// Variables referenced in this lambda's body from outer scopes.
    pub free_vars: Vec<Symbol>,

    /// Variables bound by this lambda that are mutated (set! or internal define).
    pub mutated_bindings: HashSet<Symbol>,

    /// Names from internal define forms in this lambda's body.
    pub internal_defines: Vec<Symbol>,
}

pub struct AnalysisInfo {
    pub lambdas: HashMap<NodeId, LambdaInfo>,
    /// Global set of all mutated variable names.
    pub all_mutated: HashSet<Symbol>,
}
```

### 6.2 Algorithm

1. Pre-pass assigns monotonic `NodeId(u32)` to every Lambda node
2. Recursive descent collects free variables per lambda
3. Scans all `Set` nodes to build `all_mutated`; cross-references with each
   lambda's bound vars to populate `mutated_bindings`
4. Internal defines are added to `bound` set and marked as mutated in
   `mutated_bindings` and `all_mutated` (letrec* semantics — they need
   MutableCell boxing when captured by nested lambdas)

---

## 7. Pass 2 — Closure Conversion

**File:** `pass2_closure.rs`
**Input:** `CoreExpr` + `AnalysisInfo`
**Output:** `ClosedExpr`

### 7.1 Key Types

```rust
pub enum VarLoc {
    Local,              // local register
    LocalBoxed,         // local register holding a MutableCell
    Closure(u16),       // closure slot (immutable capture)
    ClosureBoxed(u16),  // closure slot holding a MutableCell
    Global,             // global variable (LoadGlobal at runtime)
}

pub struct ClosedLambda {
    pub params: Vec<Symbol>,
    pub rest_param: Option<Symbol>,
    pub capture_list: Vec<Symbol>,      // variables to snapshot at MakeClosure
    pub boxed_params: HashSet<Symbol>,  // params needing MutableCell wrapping
    pub body: Vec<ClosedExpr>,
    pub node_id: NodeId,
    pub internal_defines: Vec<Symbol>,
}
```

### 7.2 Key Rules

- **Capture list filters out Global vars** — globals are looked up dynamically
  at runtime, not snapshotted into closures
- A variable needs MutableCell boxing if it is **mutated AND captured by a
  nested lambda** (including self-references for recursive internal defines)
- Internal defines get `VarLoc::Local`; `Define` inside lambda bodies is
  converted to `SetLocal` / `WriteLocalCell`

---

## 8. Pass 3 — Tail Position Marking

**File:** `pass3_tail.rs`
**Input:** `ClosedExpr`
**Output:** `TailedExpr`

Annotates every `App` and `Apply` node with `is_tail: bool`.

`TailedLambda` carries `boxed_params` and `internal_defines` through to Pass 4.

### 8.1 Tail Position Rules (R7RS §3.5)

| Expression | Subexpressions in tail position |
|---|---|
| `Lambda` body | Last expression of body |
| `Begin` | Last expression (if Begin itself is in tail) |
| `If` | Both `then` and `else_` (if If is in tail) |
| `App`/`Apply` | None — arguments are never in tail |
| `Define`/`Set` | Value is never in tail |

---

## 9. Pass 4 — Register Allocation

**File:** `pass4_registers.rs`
**Input:** `TailedExpr`
**Output:** `RegExpr` (allocated)

### 9.1 Key Types

```rust
pub struct RegExpr {
    pub kind: RegExprKind,  // 20+ variants with register indices
    pub dst: u16,           // destination register for this expression
}

pub struct RegLambda {
    pub node_id: NodeId,
    pub num_params: u16,
    pub rest_param: bool,
    pub num_regs: u16,
    pub body: Vec<RegExpr>,
    pub captures: Vec<CaptureSource>,
    pub boxed_param_regs: HashSet<u16>,
    pub internal_define_regs: Vec<u16>,
}

pub enum CaptureSource {
    ParentReg(u16),           // captured from parent's local register
    ParentClosureSlot(u16),   // captured from parent's closure slot
    Global(Symbol),           // global (looked up dynamically)
}
```

### 9.2 Allocation Strategy

Linear scan per function:
- `r0..r(n-1)`: parameters
- Next slots: internal define registers (after params)
- Temporaries allocated from monotonically increasing counter, freed on last use
- `num_regs = high_water_mark`

`SetLocal` / `WriteLocalCell` in `RegExprKind` embed `value: Box<RegExpr>`.

---

## 10. Pass 5 — Code Generation

**File:** `pass5_codegen.rs`
**Input:** `RegExpr` (allocated)
**Output:** `CodeObject` + nested `CodeObject`s

### 10.1 Key Features

- **`CodeObjectId(u32)`** — unique ID from global atomic counter (`fresh_code_id()`)
- **Forward jump patching** — emits `Jump { target: 0 }`, patches after body emission
- **Constant deduplication** — identical `TaggedValue` constants share pool entries
- **Two-pass top-level define** — pre-scans all `Define` names before compiling bodies
- **Nested lambdas** — compiled recursively, each gets own `CodeObject` stored in
  `code_store`

### 10.2 Lambda Prologue

For each lambda, codegen emits:
1. `AllocCell` for each boxed parameter (MutableCell wrapping)
2. Initialize internal define registers to `UNSPECIFIED`
3. `AllocCell` for boxed internal defines

### 10.3 Internal Define Codegen

Internal defines (letrec* semantics) use no new instructions:
- Registers allocated after params
- Initialized to `UNSPECIFIED` (boxed ones get `AllocCell` too)
- `Define` in lambda body → `SetLocal` / `WriteLocalCell`

---

## 11. What Each Pass Does NOT Do

| Pass | Does NOT |
|---|---|
| Pre-pass A (Quasiquote) | Rename variables, analyze free vars |
| Pre-pass B (Alpha Rename) | Box variables, allocate registers |
| Pass 1 (Analysis) | Modify the tree, allocate registers, emit instructions |
| Pass 2 (Closure Conversion) | Allocate registers, detect tail positions |
| Pass 3 (Tail Marking) | Allocate registers, know about closure slots |
| Pass 4 (Register Allocation) | Emit instructions, resolve jump targets |
| Pass 5 (Code Generation) | Make decisions — all decisions are in the input |

---

## 12. Instruction-Count Optimizations (Track P P5) and Future Passes

Landed in pass 5 (2026-08-08), all emission-level — instructions are only
chosen or replaced, never deleted, so no pc remapping exists anywhere:

| Optimization | Where | What it does |
|---|---|---|
| In-place operands | Resolved-primitive `App` emission | All-atomic-argument calls read `LocalRef` operands from their home registers — no staging `Move`s (any non-atomic argument falls the call back to staged temps, preserving evaluation order) |
| Immediate operands | Same | A fixnum-literal *right* operand is absorbed into `AddImm`/`SubImm`/`LtImm`/`NumEqImm` — no `LoadImmediate` (right side only: the deopt must preserve operand order for non-commutative rebinds) |
| Test-branch fusion | `If` emission | A predicate (`not`, `null?`, `pair?`, `vector?`, `eq?`, `<`, `=`) feeding the branch becomes `TestJumpUnless`; the plain `JumpUnless` stays at the next pc as the deopt and slow-path landing |
| Return threading | `thread_returns`, after patching | `Jump → Return` and `Move d←s; Jump → Return d` rewritten to direct `Return`s in place; orphaned instructions keep their slots |

Measured on tak: 28 → 19 dispatches per iteration (−28% wall-clock). Test-branch
fusion (wave 2) then removed one dispatch per `if` on a predicate: −7.5% on a
`null?`-driven list walk, −2.4% deriv, −1.2% nboyer.

Still future:

| Pass | Where it slots | What it does |
|---|---|---|
| Constant folding | Between 3 and 4 | Fold `(+ 1 2)` → `3` — **caution:** must not break R7RS redefinition semantics; a folded call leaves no deopt escape (see PRD §P3's design-space notes) |
| Inlining | Between 3 and 4 | Inline small known functions |
| Dead code elimination | Between 3 and 4 | Remove unreachable branches |
| Imm-operand test fusion | Pass 5 | `LtImm`/`NumEqImm` + `JumpUnless` (`(if (= n 0) …)`) — needs a second operand form on `TestJumpUnless`; measured at 4 of 32 fusable sites in the benchmark set |
