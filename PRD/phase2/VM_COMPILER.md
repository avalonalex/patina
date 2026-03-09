# Patina VM: Compiler Design

**Status:** Draft v0.1 — open for discussion
**Input:** `CoreExpr` (from `patina-frontend` desugarer)
**Output:** `CodeObject` (bytecode for `patina-vm`)
**Depends on:** [VM_ISA.md](./VM_ISA.md), [VM_DECISIONS.md](./VM_DECISIONS.md)

---

## 1. Design Philosophy

The compiler is structured as a **sequence of independent passes**, each transforming
one IR into the next. This mirrors Racket BC's 5-pass pipeline and Gauche's
multi-pass compiler.

**Why multi-pass over single-pass:**
- Each pass has a single, well-defined responsibility — easier to understand, test,
  and debug independently
- New passes (optimizations, analyses) can be inserted without touching existing
  ones
- Intermediate IRs can be inspected and pretty-printed for debugging
- Slight compile-time overhead is acceptable — we are compiling Scheme, not C++,
  and compilation happens at load time not in a tight loop

**Tradeoff acknowledged:** A single-pass compiler could be faster to compile. We
accept this in exchange for maintainability and extensibility. Passes can always
be fused later if profiling shows compilation time is a bottleneck.

---

## 2. Pipeline Overview

```
CoreExpr          (from patina-frontend)
    │
    ▼
 Pass 1: Analysis
    │  FreeVarInfo — which variables are free/captured/mutated per lambda
    │
    ▼
 Pass 2: Closure Conversion
    │  ClosedExpr — lambdas annotated with flat capture lists;
    │               mutable captured vars wrapped in MutableCell
    │
    ▼
 Pass 3: Tail Position Marking
    │  TailExpr — every App node marked as tail or non-tail
    │
    ▼
 Pass 4: Register Allocation
    │  RegExpr — every binding and temporary assigned a register index
    │
    ▼
 Pass 5: Code Generation
       CodeObject — Vec<Instruction>, constant pool, source map
```

Each pass is a pure function `Pass_N::run(input: IR_N) -> IR_{N+1}`. Passes do
not share mutable state. The pipeline wires them together.

**Reference:**
- Racket BC: compile → letrec_check → optimize → resolve → codegen
- Gauche: 5 passes, final pass (pass5) is code generation driven by context
  (`tail`, `normal/bottom`, `stmt/bottom`)

---

## 3. Input: `CoreExpr`

The compiler receives `CoreExpr` from the existing `patina-frontend` desugarer.
No changes to the frontend are needed. The 13 `CoreExprKind` variants are the
compiler's input language:

```
Literal, Var, Quote, Quasiquote, Lambda, If, Set, Begin,
Define, Import, Expand, App, Apply
```

`Quasiquote`, `Expand`, and `Import` are handled by the desugarer before the
compiler sees them. The compiler deals with the remaining 10.

---

## 4. Pass 1 — Analysis

**Input:** `CoreExpr`
**Output:** `CoreExpr` (unchanged) + `AnalysisInfo` side table
**Responsibility:** Gather information needed by later passes without transforming
the tree.

### 4.1 Free Variable Analysis

For every `Lambda` node, compute the set of free variables: variables referenced
in the body that are not bound by the lambda's own parameters or internal
`define`s.

```rust
pub struct LambdaInfo {
    /// Variables referenced in this lambda's body that come from outer scopes.
    pub free_vars: Vec<Symbol>,

    /// Subset of free_vars that are mutated via set! anywhere in the program.
    /// These must be heap-boxed (MutableCell) rather than copied by value.
    pub mutated_free_vars: HashSet<Symbol>,

    /// Variables bound by this lambda that are captured by inner lambdas
    /// AND mutated. These must be boxed at the binding site.
    pub mutated_locals: HashSet<Symbol>,
}

pub struct AnalysisInfo {
    /// Keyed by the lambda's node identity (e.g. pointer or a NodeId assigned
    /// during parsing).
    pub lambdas: HashMap<NodeId, LambdaInfo>,
}
```

**Algorithm:** Two sub-passes:

1. **Free variable collection** — recursive descent. At each `Lambda`, record
   which `Var` references are not bound locally. Propagate upward.

2. **Mutation detection** — scan all `Set` nodes in the entire tree. For each
   `Set { var }`, mark `var` as mutated. Cross-reference with each lambda's
   `free_vars` to determine `mutated_free_vars`. Cross-reference with each
   lambda's captured-by-inner-lambdas set to determine `mutated_locals`.

This is the only pass that needs global information (mutation can happen anywhere
in the program relative to the lambda that captures the variable).

---

## 5. Pass 2 — Closure Conversion

**Input:** `CoreExpr` + `AnalysisInfo`
**Output:** `ClosedExpr`
**Responsibility:** Make all variable capture explicit. Lambdas become closed
forms that carry their capture list. Mutable captured variables are wrapped.

### 5.1 `ClosedExpr` IR

```rust
pub enum ClosedExpr {
    Literal(TaggedValue),
    Var(VarRef),
    If { test: Box<ClosedExpr>, then: Box<ClosedExpr>, else_: Box<ClosedExpr> },
    Begin(Vec<ClosedExpr>),
    Set { var: VarRef, value: Box<ClosedExpr> },
    Define { name: Symbol, value: Box<ClosedExpr> },
    App { func: Box<ClosedExpr>, args: Vec<ClosedExpr> },

    /// Lambda is now a closed lambda: capture list is explicit.
    Lambda {
        params:    Arity,
        captures:  Vec<Capture>,   // ordered list of what to capture
        body:      Vec<ClosedExpr>,
    },

    /// Box a value into a MutableCell (emitted at binding sites for
    /// variables that are mutated after capture).
    Box(Box<ClosedExpr>),

    /// Unbox a MutableCell to read the current value.
    Unbox(Box<ClosedExpr>),

    /// Update a MutableCell in place (set! on a captured mutable var).
    SetBox { cell: Box<ClosedExpr>, value: Box<ClosedExpr> },
}

pub enum VarRef {
    /// Local variable or parameter — assigned a register in Pass 4.
    Local(Symbol),
    /// Free variable captured from an enclosing scope — closure slot index.
    Closure(usize),
    /// Top-level global — looked up by name at runtime.
    Global(Symbol),
}

pub struct Capture {
    pub name:   Symbol,
    /// Where to find this variable in the enclosing scope at closure creation.
    pub source: VarRef,
    /// If true, this is a MutableCell; load/store via Unbox/SetBox.
    pub mutable: bool,
}
```

### 5.2 Transformation Rules

- `Var(x)` where `x` is a free var of the current lambda →
  `Var(VarRef::Closure(slot))` where `slot` is `x`'s index in `captures`
- `Var(x)` where `x` is a mutated local → `Unbox(Var(Local(x)))`
- `Set { var: x, value }` where `x` is a captured mutable var →
  `SetBox { cell: Var(Closure(slot)), value }`
- At each lambda's binding site for a `mutated_local` variable:
  wrap the initial value in `Box(...)`
- `Lambda` nodes get their `captures` list populated from `LambdaInfo::free_vars`

---

## 6. Pass 3 — Tail Position Marking

**Input:** `ClosedExpr`
**Output:** `TailExpr`
**Responsibility:** Annotate every `App` node with whether it is in tail position.
This is the information `Call` vs `TailCall` depends on.

### 6.1 `TailExpr` IR

`TailExpr` is identical to `ClosedExpr` except `App` carries a flag:

```rust
pub enum TailExpr {
    // ... same variants as ClosedExpr ...
    App {
        func:    Box<TailExpr>,
        args:    Vec<TailExpr>,
        is_tail: bool,           // ← the only addition
    },
    Lambda {
        params:   Arity,
        captures: Vec<Capture>,
        body:     Vec<TailExpr>, // body is always compiled in tail context
    },
    // ...
}
```

### 6.2 Tail Position Rules (R7RS §3.5)

The pass recurses with a boolean `in_tail` context:

| Expression | Subexpressions in tail position |
|---|---|
| `Lambda` body | Last expression of body |
| `Begin` | Last expression |
| `If` | Both `then` and `else_` (if the `If` itself is in tail position) |
| `App` | None — arguments are never in tail position |
| `Let`-like (after desugaring → `Lambda` + `App`) | Body of the lambda |

`App` nodes where `in_tail = true` become `TailCall` in code generation.
`App` nodes where `in_tail = false` become `Call`.

**Inspiration:** Gauche pass5 threads a `ctx` parameter (`'tail`, `'normal/bottom`,
`'stmt/bottom`) through every handler. We use a simpler boolean since we don't
need Gauche's stack-discipline context variants.

---

## 7. Pass 4 — Register Allocation

**Input:** `TailExpr`
**Output:** `RegExpr`
**Responsibility:** Assign a register index to every local variable, parameter,
and temporary. After this pass, all variable references are register indices —
no symbol lookups needed during code generation.

### 7.1 `RegExpr` IR

```rust
pub enum RegExpr {
    Literal  { dst: Reg, val: TaggedValue },
    LoadReg  { dst: Reg, src: Reg },
    LoadClosure { dst: Reg, slot: u16 },
    LoadGlobal  { dst: Reg, name: Symbol },
    StoreGlobal { name: Symbol, src: Reg },
    Box      { dst: Reg, src: Reg },
    Unbox    { dst: Reg, src: Reg },
    SetBox   { cell: Reg, val: Reg },
    If       { cond: Reg, then: Vec<RegExpr>, else_: Vec<RegExpr>, dst: Reg },
    App      { func: Reg, args: Vec<Reg>, dst: Reg, is_tail: bool },
    Lambda   { dst: Reg, code_id: CodeId, captures: Vec<Reg> },
    Define   { name: Symbol, src: Reg },
    Begin    { exprs: Vec<RegExpr>, dst: Reg },
    Return   { src: Reg },
}
```

All temporaries are made explicit. There are no anonymous subexpressions — every
intermediate value has a register.

### 7.2 Allocation Strategy

**Phase 2A: simple linear scan.**

Each function gets its own `Allocator`:
- Parameters occupy `r0, r1, ...` (fixed by calling convention).
- New temporaries are allocated from a monotonically increasing counter.
- When a temporary is no longer live (after its last use), it is pushed onto a
  free list and may be reused.
- `num_regs = high_water_mark` at the end of the function.

```rust
pub struct Allocator {
    next:       u16,
    free_list:  Vec<Reg>,
    high_water: u16,
}
```

**Phase 2B+:** Replace with a liveness-based allocator (linear scan over live
ranges, or graph coloring) if register pressure turns out to be an issue.

### 7.3 Environment Map

During this pass, a `HashMap<Symbol, Reg>` tracks the current mapping from
variable names to registers. It is scoped: entering a lambda creates a fresh map;
exiting restores the parent map.

```rust
pub struct Env {
    locals:  HashMap<Symbol, Reg>,   // name → register
    closure: HashMap<Symbol, u16>,   // name → closure slot
}
```

Global variables are not in the map — they are emitted as `LoadGlobal`/`StoreGlobal`.

---

## 8. Pass 5 — Code Generation

**Input:** `RegExpr`
**Output:** `CodeObject`
**Responsibility:** Emit `Instruction` values. No decisions — all decisions were
made in earlier passes. This pass is a straightforward translation.

### 8.1 Label Patching

Forward jumps (from `If`) emit a placeholder `Label(id)` and record a patch site.
After emitting all instructions for the function body, a single sweep patches
all placeholders to their resolved instruction indices.

```rust
struct PatchSite {
    instruction_index: usize,
    label_id:          LabelId,
}
```

### 8.2 Nested Lambdas

When code generation encounters a `Lambda` node, it recursively runs the full
pipeline (passes 1–5) on the lambda body, producing a nested `CodeObject`. The
nested `CodeObject` is added to the parent's constant pool, and a `MakeClosure`
instruction is emitted in the parent.

This means each `CodeObject` is self-contained: it has its own instruction list,
constant pool, and source map.

### 8.3 Constant Pool Deduplication

Identical `TaggedValue` constants (fixnums, characters, booleans, interned symbols)
are deduplicated — the same value emits one pool entry regardless of how many
times it appears. Heap objects (pairs, strings from `quote`) are not deduplicated
(structural equality on heap objects is expensive; identity is sufficient).

---

## 9. Pipeline Wiring

```rust
pub struct Compiler;

impl Compiler {
    pub fn compile(expr: &CoreExpr) -> Result<CodeObject, CompileError> {
        let info    = Pass1Analysis::run(expr);
        let closed  = Pass2Closure::run(expr, &info);
        let tailed  = Pass3Tail::run(&closed);
        let regged  = Pass4Registers::run(&tailed);
        let code    = Pass5Codegen::run(&regged);
        Ok(code)
    }
}
```

Each pass is a stateless struct with a single `run` method. Tests can call any
pass in isolation with a hand-constructed input.

---

## 10. What Each Pass Does NOT Do

Clear boundaries prevent passes from growing scope:

| Pass | Does NOT |
|---|---|
| Pass 1 (Analysis) | Modify the tree, allocate registers, emit instructions |
| Pass 2 (Closure Conversion) | Allocate registers, detect tail positions |
| Pass 3 (Tail Marking) | Allocate registers, know about closures |
| Pass 4 (Register Allocation) | Emit instructions, know about jump targets |
| Pass 5 (Code Generation) | Make decisions — all decisions are in the input |

---

## 11. Future Passes

New passes slot in between existing ones without touching others:

| Pass | Where it slots | What it does |
|---|---|---|
| Constant folding | Between 3 and 4 | Fold `(+ 1 2)` → `3` at compile time |
| Inlining | Between 3 and 4 | Inline small known functions |
| Dead code elimination | Between 3 and 4 | Remove unreachable branches |
| Liveness analysis | Between 4 and 5 | Better register reuse |
| Peephole optimization | After 5 | Eliminate redundant `Move`s, `LoadConst` + `Jump` patterns |

All of these are Phase 2B+ concerns. Phase 2A implements only the five core passes.

---

## 12. Open Questions

1. ~~**`CoreExpr` node identity for `AnalysisInfo`:**~~ ✅ Settled: pre-pass
   inside Pass 1. A first sub-pass walks the tree and assigns monotonic `NodeId`s
   to all `Lambda` nodes, storing them in a side table. The analysis sub-pass
   then keys `LambdaInfo` by `NodeId`. Self-contained, no frontend changes.

2. ~~**Top-level `define` sequencing:**~~ ✅ Settled: two-pass top-level. Before
   compiling any top-level expression, scan the full top-level sequence and
   pre-register all `define` names as globals. Then compile bodies in order.
   Handles forward references between top-level definitions. Consistent with how
   the tree-walker already handles top-level definitions.
