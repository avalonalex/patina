# Value Representation Architecture for VM

**Status:** Design Document - **Option D Selected**
**Created:** 2025-12-12
**Updated:** 2025-12-12 (Added Chez Scheme research, selected Option D)
**Related:** [VM_VALUE_ARCHITECTURE.md](./VM_VALUE_ARCHITECTURE.md), [VM_SPECIFICATION.md](./VM_SPECIFICATION.md)

> **Decision:** Use **Option D (Pragmatic Dual-Backend)** - Keep tree-walker unchanged with `Value`, build VM with `TaggedValue`. Accept some code duplication; refactor later if needed.

---

## Overview

This document proposes separating the current `Value` type into distinct types for different purposes:

1. **`SExpr`** - Pure syntax tree (parser output)
2. **`Datum`** - Compile-time constant data (quoted expressions)
3. **`RtValue`** - Tree-walker runtime values
4. **`TvValue`** - VM runtime values (tagged pointers)

The key insight is that the current `Value` enum serves **three different purposes**, leading to complexity and duplication.

This document also analyzes how Chez Scheme handles this problem and proposes a hybrid approach inspired by their design.

---

## Table of Contents

1. [Problem Statement](#problem-statement)
2. [How Chez Scheme Handles This](#how-chez-scheme-handles-this)
3. [Proposed Architecture Options](#proposed-architecture-options)
4. [Option A: Full SExpr Separation](#option-a-full-sexpr-separation)
5. [Option B: Hybrid Approach (Recommended)](#option-b-hybrid-approach-recommended)
6. [Type Definitions](#type-definitions)
7. [Migration Strategy](#migration-strategy)
8. [Comparison Summary](#comparison-summary)

---

## Problem Statement

### Current Architecture

```rust
// Value serves THREE purposes:

// 1. SYNTAX - Parser output
let parsed: Value = parser.parse()?;  // Value::Pair(Rc<RefCell<...>>)

// 2. COMPILE-TIME DATA - Literals in IR
pub enum CoreExpr {
    Literal(Rc<Value>),  // ← Value embedded in IR
    Quote(Rc<Value>),    // ← Value embedded in IR
}

// 3. RUNTIME VALUES - Evaluation results
pub enum Value {
    Procedure(Rc<Procedure>),    // Runtime-only
    Continuation(Rc<...>),       // Runtime-only
    Port(Rc<Port>),              // Runtime-only
    Promise(Rc<RefCell<...>>),   // Runtime-only
}
```

### Problems with Current Design

| Issue | Description |
|-------|-------------|
| **Conceptual overloading** | `Value` means different things in different contexts |
| **Unnecessary mutability** | Parser creates `Rc<RefCell<...>>` for syntax that never mutates |
| **Duplication in IRs** | Both `CoreExpr` and `CpsExpr` embed `Rc<Value>` |
| **Runtime concepts in syntax** | `Procedure`, `Continuation` can appear in "syntax" type |
| **Awkward list representation** | Parser builds linked pairs instead of vectors |

### Code Smell: Pair Chains

```rust
// Current: Parser builds cons cells for lists
fn parse_list(&mut self) -> Result<Value, ParseError> {
    let items = self.parse_items()?;
    // Convert Vec to Rc<RefCell<(Value, Value)>> chain - awkward!
    let mut result = Value::Null;
    for item in items.into_iter().rev() {
        result = Value::Pair(Rc::new(RefCell::new((item, result))));
    }
    Ok(result)
}

// Pattern matching is painful:
fn is_define(expr: &Value) -> bool {
    match expr {
        Value::Pair(p) => {
            let borrowed = p.borrow();
            match &borrowed.0 {
                Value::Symbol(s) if s.as_ref() == "define" => true,
                _ => false,
            }
        }
        _ => false,
    }
}
```

---

## How Chez Scheme Handles This

Before designing our solution, let's examine how [Chez Scheme](https://github.com/cisco/ChezScheme) - a production-quality Scheme with both interpreter and compiler backends - handles these concerns.

### Chez Scheme Architecture

```
Source Code
     │
     ▼
┌─────────────────┐
│  Reader         │  → Produces "annotated datums"
│  (get-datum/    │     (S-expressions + source locations)
│   annotations)  │
└────────┬────────┘
         │
         ▼
   Annotated S-expressions (same tagged pointer representation)
         │
         ▼
┌─────────────────┐
│  Expander       │  → Macro expansion, produces Lsrc
│  (syntax.ss)    │     (core forms only)
└────────┬────────┘
         │
         ▼
      Lsrc IR        ← Core language with `datum` terminal
         │
         ├───────────────────────┐
         │                       │
         ▼                       ▼
┌─────────────────┐      ┌─────────────────┐
│  Compiler       │      │  Interpreter    │
│  (cpnanopass.ss)│      │  (Petite Chez)  │
│  → Native Code  │      │  → Threaded     │
└─────────────────┘      └─────────────────┘
```

### Key Design Decisions in Chez

**1. Single Value Representation (Tagged Pointers Everywhere)**

From [IMPLEMENTATION.md](https://github.com/cisco/ChezScheme/blob/main/IMPLEMENTATION.md):
> "The low bits of the pointer indicate the general type of the object, such as 'pair' or 'closure'."

Chez uses tagged pointers for **all** values - from reader output through to runtime:
- Pairs use tag `#b001`
- Type-tagged objects use all low bits set, with actual type in first word
- This representation is shared between interpreter and compiler

**2. Annotated Datums (Not Separate AST)**

The reader produces **annotated S-expressions**, not a separate AST type:
```scheme
(make-annotation obj source-object stripped-obj)
```
- `obj` is the actual datum (S-expression as tagged pointer)
- `source-object` has file/position info
- `stripped-obj` is the datum without nested annotations

From the [User's Guide](https://www.scheme.com/csug8/syntax.html):
> "When source code is read from a file by load, compile-file, or variants of these, the reader attaches annotations to each object read from the file."

**3. Lsrc - The Core Language**

After macro expansion, everything becomes `Lsrc` (defined in `base-lang.ss`):
```scheme
(terminals
  (preinfo (preinfo))
  ($prelex (x))         ; unique variable
  (datum (d))           ; ← Scheme datum as compile-time constant
  ...)

(Expr (e)
  (quote d)             ; quoted datum
  (ref maybe-src x)     ; variable reference
  (if e0 e1 e2)
  (seq e0 e1)
  (call preinfo e0 e1 ...)
  (case-lambda preinfo cl ...)
  ...)
```

**Critical insight**: The `datum` terminal in Lsrc is **the same tagged pointer value** used at runtime. No conversion is needed!

**4. Interpreter and Compiler Share Everything**

From the [Introduction](https://www.scheme.com/csug8/intro.html):
> "Petite Chez Scheme is built from the same sources as Chez Scheme, with all but the compiler sources included."

Both backends use:
- Same reader (annotated datums)
- Same expander (produces Lsrc)
- Same value representation (tagged pointers)

The only difference is what happens to Lsrc:
- **Compiler**: Lsrc → nanopass IRs → native code
- **Interpreter**: Lsrc → threaded code (direct interpretation)

### Why Chez Avoids Our Problem

Chez avoids the conversion problem by using **tagged pointers from the start**:

| Stage | Chez Scheme | Current Patina |
|-------|-------------|----------------|
| Reader output | Tagged pointer | `Value::Pair(Rc<RefCell<...>>)` |
| After expansion | Lsrc with tagged `datum` | CoreExpr with `Rc<Value>` |
| Interpreter runtime | Tagged pointer | `Value` enum |
| Compiler runtime | Tagged pointer | (future) `TvValue` |
| Conversion needed | **None** | Multiple conversions |

When the Chez interpreter evaluates `(quote d)`, it just returns `d` - the tagged pointer is already in the correct format.

### Lessons for Patina

1. **Unified representation eliminates conversions** - If we use tagged pointers everywhere, no conversion is needed between stages.

2. **Annotations vs AST** - Chez doesn't have a separate AST; it annotates existing values. This preserves homoiconicity.

3. **Same code, different backends** - Chez's design allows the interpreter and compiler to share the entire frontend.

4. **The `datum` terminal** - In Lsrc, quoted data is just a tagged pointer. Simple and efficient.

### References

- [Chez Scheme GitHub](https://github.com/cisco/ChezScheme)
- [IMPLEMENTATION.md](https://github.com/cisco/ChezScheme/blob/main/IMPLEMENTATION.md) - Internal architecture
- [User's Guide - Syntax](https://www.scheme.com/csug8/syntax.html) - Syntax objects and annotations
- [Nanopass Framework](https://nanopass.org/) - Compiler IR framework
- [A Nanopass Framework for Commercial Compiler Development](https://andykeep.com/pubs/dissertation.pdf) - Andy Keep's dissertation

---

## Proposed Architecture Options

Based on our analysis, we have three main options:

### Option A: Full SExpr Separation

Separate types for each stage:
- `SExpr` (parser) → `Datum` (IR) → `RtValue`/`TvValue` (runtime)
- Clean conceptual model
- Multiple conversions required
- **7-10 weeks effort**

### Option B: Hybrid Approach

Inspired by Chez, but with practical compromises:
- `SExpr` for parsing and macro expansion (easy manipulation)
- `TaggedValue` for IR and runtime (no conversion between stages)
- Single conversion point: SExpr → TaggedValue during desugaring
- **5-7 weeks effort**

### Option D: Pragmatic Dual-Backend (Selected)

Keep tree-walker simple, build VM with optimal representation:
- `SExpr` for parsing and macro expansion (easy manipulation)
- Two desugaring paths:
  - Tree-walker: SExpr → CoreExpr with `Value` (current representation)
  - VM: SExpr → CoreExpr with `TaggedValue` (optimal representation)
- Accept some code duplication; refactor later if needed
- **4-6 weeks for VM** (tree-walker unchanged)

### Option C: Full Tagged Pointers (Chez-style)

Tagged pointers from reader through runtime:
- Most efficient (zero conversion)
- Parser becomes more complex
- Macro expansion on tagged values (less ergonomic)
- **6-8 weeks effort**

---

## Option A: Full SExpr Separation

(Detailed below in Type Definitions section)

---

## Option B: Hybrid Approach (Recommended)

This approach takes the best of both worlds:

### Architecture

```
Source Code
     │
     ▼
┌─────────────────┐
│  Parser         │  → SExpr (Vec-based, easy manipulation)
└────────┬────────┘
         │
         ▼
      SExpr           ← Immutable, Vec<SExpr> for lists
         │
         │ macro expansion (SExpr → SExpr)
         │
         ▼
      SExpr (expanded)
         │
         │ desugaring (SINGLE conversion point)
         │
         ▼
┌─────────────────┐
│  Desugarer      │  → CoreExpr with TaggedValue datums
└────────┬────────┘
         │
         ▼
      CoreExpr        ← Contains TaggedValue for literals/quotes
         │
         ├───────────────────────┐
         │                       │
         ▼                       ▼
┌─────────────────┐      ┌─────────────────┐
│  CPS Transform  │      │  VM Compiler    │
│  → CpsExpr      │      │  → Bytecode     │
└────────┬────────┘      └────────┬────────┘
         │                        │
         ▼                        ▼
      CpsExpr             Bytecode
      + TaggedValue       + TaggedValue
         │                        │
         ▼                        ▼
┌─────────────────┐      ┌─────────────────┐
│  Tree-Walker    │      │  VM Executor    │
│  (TaggedValue)  │      │  (TaggedValue)  │
└─────────────────┘      └─────────────────┘
```

### Key Design Points

**1. SExpr for Front-End (Easy Manipulation)**

```rust
/// Parser output - optimized for manipulation
pub enum SExpr {
    Integer(i64),
    Symbol(Symbol),
    List(Vec<SExpr>),      // Vec, not pairs!
    ImproperList { elements: Vec<SExpr>, tail: Box<SExpr> },
    // ... other variants
}
```

Benefits:
- Pattern matching is trivial: `match list { SExpr::List(items) => ... }`
- Building syntax is easy: `SExpr::List(vec![...])`
- No `Rc<RefCell<...>>` overhead

**2. TaggedValue for IR and Runtime (Zero Conversion)**

```rust
/// 8-byte tagged value used in IR and at runtime
#[repr(transparent)]
pub struct TaggedValue(u64);

/// CoreExpr uses TaggedValue for data
pub enum CoreExpr {
    Literal(TaggedValue),     // Not Datum!
    Quote(TaggedValue),       // Not Datum!
    Var { name: Symbol, scopes: ScopeSet },
    // ...
}
```

Benefits:
- No conversion between IR and runtime
- Both tree-walker and VM use same representation
- Cache-friendly 8-byte values

**3. Single Conversion Point**

Conversion happens **once**, during desugaring:

```rust
/// Convert SExpr to TaggedValue (called during desugaring)
fn sexpr_to_tagged(sexpr: &SExpr, heap: &mut Heap) -> TaggedValue {
    match sexpr {
        SExpr::Integer(n) if fits_fixnum(*n) => TaggedValue::fixnum(*n),
        SExpr::Boolean(true) => TaggedValue::TRUE,
        SExpr::Symbol(s) => heap.intern_symbol(s),
        SExpr::List(items) => {
            // Convert to tagged pair chain
            let tagged_items: Vec<_> = items.iter()
                .map(|item| sexpr_to_tagged(item, heap))
                .collect();
            heap.list_to_pairs(tagged_items)
        }
        // ...
    }
}

/// Desugaring: SExpr → CoreExpr with TaggedValue
fn desugar(sexpr: &SExpr, env: &MacroEnv, heap: &mut Heap) -> Result<CoreExpr, Error> {
    match sexpr {
        SExpr::Integer(n) => {
            let tv = sexpr_to_tagged(sexpr, heap);
            Ok(CoreExpr::Literal(tv))
        }
        SExpr::Quote(inner) => {
            let tv = sexpr_to_tagged(inner, heap);
            Ok(CoreExpr::Quote(tv))
        }
        // ...
    }
}
```

**4. Shared Heap for All Backends**

```rust
/// Shared heap used by tree-walker and VM
pub struct Heap {
    pairs: Vec<(TaggedValue, TaggedValue)>,
    vectors: Vec<Vec<TaggedValue>>,
    strings: Vec<String>,
    symbols: SymbolTable,
    // ...
}
```

Both backends share the same heap:
- Tree-walker: `eval_cps(&cps_expr, &mut heap)`
- VM: `vm.execute(&bytecode, &mut heap)`

### Comparison with Full Separation

| Aspect | Full Separation (Option A) | Hybrid (Option B) |
|--------|---------------------------|-------------------|
| Conversion points | 3+ (SExpr→Datum→RtValue/TvValue) | 1 (SExpr→TaggedValue) |
| Parser complexity | Simple | Simple |
| Macro expansion | On SExpr (easy) | On SExpr (easy) |
| IR representation | Datum (needs conversion) | TaggedValue (ready to use) |
| Runtime sharing | Separate RtValue/TvValue | Same TaggedValue |
| Effort | 7-10 weeks | 5-7 weeks |

### Why Hybrid is Recommended

1. **Best of both worlds**: Easy syntax manipulation (SExpr) + efficient runtime (TaggedValue)
2. **Minimal conversion**: Only one conversion point, during desugaring
3. **Shared runtime**: Tree-walker and VM use the same TaggedValue
4. **Incremental migration**: Can add SExpr first, then migrate to TaggedValue
5. **Closer to Chez model**: IR contains ready-to-use values

---

## Option D: Pragmatic Dual-Backend (Selected)

**This is the selected approach for Patina.**

### Rationale

1. **Tree-walker works** - It passes all R7RS tests, no need to change it
2. **VM is new code** - Can use optimal representation from the start
3. **Minimize risk** - Tree-walker remains as reference/fallback
4. **Accept duplication** - Some shared logic will be duplicated; can refactor later
5. **Fastest path to working VM** - No need to rewrite tree-walker first

### Architecture

```
Source Code
     │
     ▼
┌─────────────────┐
│  Parser         │  → Value (current, unchanged)
└────────┬────────┘
         │
         ▼
      Value           ← Current representation (Rc<RefCell<...>>)
         │
         │ macro expansion (Value → Value, unchanged)
         │
         ▼
      Value (expanded)
         │
         ├─────────────────────────────────────────┐
         │                                         │
         ▼                                         ▼
┌─────────────────┐                    ┌─────────────────┐
│ Desugarer       │                    │ VM Desugarer    │
│ (current)       │                    │ (new)           │
│ Value→CoreExpr  │                    │ Value→VmCoreExpr│
│ with Rc<Value>  │                    │ with TaggedValue│
└────────┬────────┘                    └────────┬────────┘
         │                                      │
         ▼                                      ▼
   CoreExpr                              VmCoreExpr
   + Rc<Value>                           + TaggedValue
         │                                      │
         ▼                                      ▼
┌─────────────────┐                    ┌─────────────────┐
│  CPS Transform  │                    │  VM Compiler    │
│  → CpsExpr      │                    │  → Bytecode     │
└────────┬────────┘                    └────────┬────────┘
         │                                      │
         ▼                                      ▼
   CpsExpr                              Bytecode
   + Value                              + TaggedValue
         │                                      │
         ▼                                      ▼
┌─────────────────┐                    ┌─────────────────┐
│  Tree-Walker    │                    │  VM Executor    │
│  (Value)        │                    │  (TaggedValue)  │
│  UNCHANGED      │                    │  NEW            │
└─────────────────┘                    └─────────────────┘
```

### Type Definitions

**Tree-Walker (Unchanged)**

```rust
// patina-core/src/core_expr.rs (existing)
pub enum CoreExpr {
    Literal(Rc<Value>),
    Quote(Rc<Value>),
    Var { name: Symbol, scopes: ScopeSet },
    Lambda { params: Formals, body: Vec<CoreExpr>, ... },
    // ...
}
```

**VM (New)**

```rust
// patina-vm/src/vm_core_expr.rs (new)
pub enum VmCoreExpr {
    Literal(TaggedValue),
    Quote(TaggedValue),
    Var { name: Symbol, scopes: ScopeSet },
    Lambda { params: VmFormals, body: Vec<VmCoreExpr>, ... },
    // ...
}

// patina-vm/src/tagged_value.rs (new)
#[repr(transparent)]
pub struct TaggedValue(u64);
```

### Desugaring Paths

**Tree-Walker Path (Existing)**

```rust
// patina-frontend/src/desugarer/mod.rs (unchanged)
pub fn desugar(expr: &Value, env: &Env) -> Result<CoreExpr, DesugarError> {
    // Current implementation
}
```

**VM Path (New)**

```rust
// patina-vm/src/desugarer.rs (new)
pub fn desugar_for_vm(
    expr: &Value,
    env: &Env,
    heap: &mut VmHeap,
) -> Result<VmCoreExpr, DesugarError> {
    match expr {
        Value::Integer(n) => {
            let tv = if TaggedValue::fits_fixnum(*n) {
                TaggedValue::fixnum(*n)
            } else {
                heap.alloc_bigint(BigInt::from(*n))
            };
            Ok(VmCoreExpr::Literal(tv))
        }
        Value::Pair(p) => {
            let borrowed = p.borrow();
            if is_special_form(&borrowed.0) {
                desugar_special_form_for_vm(&borrowed, env, heap)
            } else {
                desugar_app_for_vm(&borrowed, env, heap)
            }
        }
        // ... similar to existing desugarer, but produces VmCoreExpr
    }
}
```

### Shared Logic (Potential Duplication)

These components will have similar logic in both paths:

| Component | Tree-Walker | VM |
|-----------|-------------|-----|
| Special form detection | `is_lambda()`, `is_if()`, etc. | Same logic |
| Lambda parameter parsing | `parse_formals()` | Same logic |
| Macro expansion check | Check env for macros | Same logic |
| Hygiene scope handling | `ScopeSet` operations | Same logic |

**Mitigation strategies:**

1. **Extract shared utilities** into a common module:
   ```rust
   // patina-core/src/syntax_utils.rs
   pub fn is_special_form(head: &Value) -> Option<&str> { ... }
   pub fn parse_formals(formals: &Value) -> Result<Formals, Error> { ... }
   ```

2. **Use traits** for backend-agnostic operations:
   ```rust
   trait DesugarTarget {
       type Expr;
       type Literal;
       fn make_literal(value: &Value) -> Self::Literal;
       fn make_if(test: Self::Expr, then: Self::Expr, else_: Self::Expr) -> Self::Expr;
       // ...
   }
   ```

3. **Defer refactoring** - Get VM working first, then extract common code

### Module Structure

```
patina-core/           # Shared types
├── value.rs           # Value enum (unchanged)
├── core_expr.rs       # CoreExpr for tree-walker (unchanged)
├── cps_expr.rs        # CpsExpr (unchanged)
├── scope.rs           # ScopeSet (shared)
└── syntax_utils.rs    # NEW: shared syntax helpers

patina-frontend/       # Parsing and tree-walker desugaring
├── parser/            # Produces Value (unchanged)
├── desugarer/         # Value → CoreExpr (unchanged)
└── macro_expander/    # Value → Value (unchanged)

patina-tree-walker/    # Tree-walking interpreter (UNCHANGED)
├── eval/
└── primitives/

patina-vm/             # NEW: VM backend
├── tagged_value.rs    # TaggedValue(u64)
├── heap.rs            # VmHeap
├── vm_core_expr.rs    # VmCoreExpr with TaggedValue
├── desugarer.rs       # Value → VmCoreExpr
├── compiler/          # VmCoreExpr → Bytecode
├── exec/              # Bytecode execution
└── primitives/        # VM primitives
```

### Implementation Phases

**Phase 1: VM Foundation (2-3 weeks)**
- [ ] `TaggedValue` implementation
- [ ] `VmHeap` for pair/vector/string allocation
- [ ] `VmCoreExpr` type definition
- [ ] Basic `desugar_for_vm` (literals, variables, if, lambda, app)

**Phase 2: VM Bytecode (2-3 weeks)**
- [ ] Bytecode instruction set
- [ ] Compiler: VmCoreExpr → Bytecode
- [ ] Basic VM execution loop
- [ ] Implement `Backend` trait for VM

**Phase 3: VM Primitives (1-2 weeks)**
- [ ] Arithmetic primitives
- [ ] List primitives
- [ ] Comparison primitives
- [ ] I/O primitives (basic)

**Phase 4: Integration (1 week)**
- [ ] Run R7RS tests with VM backend
- [ ] Fix semantic differences
- [ ] Performance benchmarks

### Benefits of This Approach

1. **Low risk**: Tree-walker unchanged, serves as reference
2. **Fast iteration**: Can develop VM independently
3. **Clean separation**: VM has optimal types from the start
4. **Testable**: Can compare tree-walker vs VM output
5. **Pragmatic**: Accept duplication now, refactor later

### Future Refactoring Opportunities

Once VM is working, we can optionally:

1. **Extract SExpr type** for cleaner parsing/macro work
2. **Unify desugaring** via traits or macros
3. **Migrate tree-walker to TaggedValue** if desired
4. **Share more code** between backends

But these are **not required** for a working VM.

---

## Option A: Full SExpr Separation

(Original proposal - kept for reference)

### Type Hierarchy

```
Source Code
     │
     ▼
┌─────────┐
│ Parser  │
└────┬────┘
     │
     ▼
   SExpr          ← Pure syntax tree (immutable, no runtime concepts)
     │
     │ macro expansion (SExpr → SExpr)
     │
     ├─────────────────────┬────────────────────┐
     │                     │                    │
     ▼                     ▼                    ▼
┌──────────┐        ┌──────────┐         ┌──────────┐
│ Desugar  │        │ Desugar  │         │ Compile  │
│ to       │        │ to       │         │ to       │
│ CoreExpr │        │ CpsExpr  │         │ Bytecode │
└────┬─────┘        └────┬─────┘         └────┬─────┘
     │                   │                    │
     │ + Datum           │ + Datum            │ + TvConstant
     │                   │                    │
     ▼                   ▼                    ▼
┌──────────┐        ┌──────────┐         ┌──────────┐
│ (future) │        │ CPS Eval │         │ VM Exec  │
│ CoreEval │        │          │         │          │
└────┬─────┘        └────┬─────┘         └────┬─────┘
     │                   │                    │
     ▼                   ▼                    ▼
  TvValue             RtValue              TvValue
  (tagged)            (enum)               (tagged)
```

---

## Type Definitions

### SExpr - Pure Syntax

```rust
//! Parser output - pure syntax tree
//!
//! Design principles:
//! - Immutable (no RefCell)
//! - No runtime concepts (no Procedure, Continuation, etc.)
//! - Lists are Vec, not linked pairs
//! - Cheap to clone (uses Rc for large data)

/// S-expression - the syntax of Scheme
#[derive(Debug, Clone, PartialEq)]
pub enum SExpr {
    // ========== Atoms ==========

    /// Exact integer (fits in i64)
    Integer(i64),

    /// Arbitrary precision integer
    BigInteger(Rc<BigInt>),

    /// Exact rational
    Rational(Rc<BigRational>),

    /// Inexact real (IEEE 754)
    Float(f64),

    /// Complex number (real and imaginary parts)
    Complex(Box<ComplexSExpr>),

    /// Boolean (#t or #f)
    Boolean(bool),

    /// Character (#\a, #\newline, etc.)
    Character(char),

    /// String literal ("hello")
    String(Rc<str>),

    /// Symbol (foo, +, define)
    Symbol(Symbol),

    /// Identifier with hygiene scopes (from macro expansion)
    Identifier(Identifier),

    // ========== Compounds ==========

    /// List: (a b c)
    /// Stored as Vec for easy manipulation
    List(Vec<SExpr>),

    /// Improper list: (a b . c)
    /// The `improper` field is the final cdr
    ImproperList {
        elements: Vec<SExpr>,
        tail: Box<SExpr>,
    },

    /// Vector: #(1 2 3)
    Vector(Vec<SExpr>),

    /// Bytevector: #u8(1 2 3)
    Bytevector(Vec<u8>),

    // ========== Special Syntax ==========

    /// Quote: 'x or (quote x)
    Quote(Box<SExpr>),

    /// Quasiquote: `x or (quasiquote x)
    Quasiquote(Box<SExpr>),

    /// Unquote: ,x or (unquote x)
    Unquote(Box<SExpr>),

    /// Unquote-splicing: ,@x or (unquote-splicing x)
    UnquoteSplicing(Box<SExpr>),

    // ========== Empty ==========

    /// Empty list: ()
    Null,
}

/// Complex number in syntax
#[derive(Debug, Clone, PartialEq)]
pub struct ComplexSExpr {
    pub real: SExpr,
    pub imag: SExpr,
}

/// Interned symbol
pub type Symbol = Rc<str>;

/// Identifier with hygiene information
#[derive(Debug, Clone, PartialEq)]
pub struct Identifier {
    pub name: Symbol,
    pub scopes: ScopeSet,
}
```

### SExpr Conveniences

```rust
impl SExpr {
    /// Check if this is a list starting with the given symbol
    pub fn is_form(&self, name: &str) -> bool {
        match self {
            SExpr::List(items) if !items.is_empty() => {
                matches!(&items[0], SExpr::Symbol(s) if s.as_ref() == name)
            }
            _ => false,
        }
    }

    /// Get list items (panics if not a list)
    pub fn as_list(&self) -> &[SExpr] {
        match self {
            SExpr::List(items) => items,
            _ => panic!("Expected list, got {:?}", self),
        }
    }

    /// Pattern match as (head . tail) for macro-style processing
    pub fn as_cons(&self) -> Option<(&SExpr, &[SExpr])> {
        match self {
            SExpr::List(items) if !items.is_empty() => {
                Some((&items[0], &items[1..]))
            }
            _ => None,
        }
    }

    /// Get symbol name (for identifiers too)
    pub fn symbol_name(&self) -> Option<&str> {
        match self {
            SExpr::Symbol(s) => Some(s.as_ref()),
            SExpr::Identifier(id) => Some(id.name.as_ref()),
            _ => None,
        }
    }
}
```

---

### Datum - Compile-Time Constants

```rust
//! Datum - compile-time constant data
//!
//! This represents data that can appear in quoted expressions.
//! It's a subset of SExpr that survives to runtime as constants.
//!
//! Key differences from SExpr:
//! - No Identifier (hygiene resolved at compile time)
//! - No Quote/Quasiquote/Unquote (processed at compile time)
//! - Lists become pairs (proper Scheme representation)
//! - All data is immutable

/// Compile-time constant data
#[derive(Debug, Clone, PartialEq)]
pub enum Datum {
    // ========== Immediate Values ==========
    Integer(i64),
    Boolean(bool),
    Character(char),
    Null,

    // ========== Heap-Allocated (Rc for sharing) ==========
    BigInteger(Rc<BigInt>),
    Rational(Rc<BigRational>),
    Float(f64),  // Could be immediate in tagged representation
    Complex(Rc<(Datum, Datum)>),

    String(Rc<str>),
    Symbol(Symbol),

    /// Pair (immutable at compile time)
    /// Runtime may convert to mutable pairs
    Pair(Rc<(Datum, Datum)>),

    /// Vector (immutable at compile time)
    Vector(Rc<Vec<Datum>>),

    /// Bytevector
    Bytevector(Rc<Vec<u8>>),
}

impl Datum {
    /// Convert a proper list to a Datum pair chain
    pub fn list(items: impl IntoIterator<Item = Datum>) -> Datum {
        let items: Vec<_> = items.into_iter().collect();
        items.into_iter().rev().fold(Datum::Null, |acc, item| {
            Datum::Pair(Rc::new((item, acc)))
        })
    }

    /// Create an improper list (a b . c)
    pub fn improper_list(items: Vec<Datum>, tail: Datum) -> Datum {
        items.into_iter().rev().fold(tail, |acc, item| {
            Datum::Pair(Rc::new((item, acc)))
        })
    }
}
```

### SExpr → Datum Conversion

```rust
/// Convert syntax to datum (for quote)
///
/// This is called when processing (quote expr) or 'expr
/// Identifiers are converted to symbols (scopes discarded)
pub fn sexpr_to_datum(sexpr: &SExpr) -> Result<Datum, ConversionError> {
    match sexpr {
        // Atoms - direct conversion
        SExpr::Integer(n) => Ok(Datum::Integer(*n)),
        SExpr::BigInteger(n) => Ok(Datum::BigInteger(n.clone())),
        SExpr::Rational(r) => Ok(Datum::Rational(r.clone())),
        SExpr::Float(f) => Ok(Datum::Float(*f)),
        SExpr::Complex(c) => {
            let real = sexpr_to_datum(&c.real)?;
            let imag = sexpr_to_datum(&c.imag)?;
            Ok(Datum::Complex(Rc::new((real, imag))))
        }
        SExpr::Boolean(b) => Ok(Datum::Boolean(*b)),
        SExpr::Character(c) => Ok(Datum::Character(*c)),
        SExpr::String(s) => Ok(Datum::String(s.clone())),
        SExpr::Symbol(s) => Ok(Datum::Symbol(s.clone())),
        SExpr::Null => Ok(Datum::Null),

        // Identifier → Symbol (discard scopes in quoted data)
        SExpr::Identifier(id) => Ok(Datum::Symbol(id.name.clone())),

        // List → Pair chain
        SExpr::List(items) => {
            let datums: Vec<_> = items.iter()
                .map(sexpr_to_datum)
                .collect::<Result<_, _>>()?;
            Ok(Datum::list(datums))
        }

        // Improper list → Pair chain with non-null tail
        SExpr::ImproperList { elements, tail } => {
            let datums: Vec<_> = elements.iter()
                .map(sexpr_to_datum)
                .collect::<Result<_, _>>()?;
            let tail_datum = sexpr_to_datum(tail)?;
            Ok(Datum::improper_list(datums, tail_datum))
        }

        // Vector
        SExpr::Vector(items) => {
            let datums: Vec<_> = items.iter()
                .map(sexpr_to_datum)
                .collect::<Result<_, _>>()?;
            Ok(Datum::Vector(Rc::new(datums)))
        }

        // Bytevector
        SExpr::Bytevector(bytes) => {
            Ok(Datum::Bytevector(Rc::new(bytes.clone())))
        }

        // Quote forms should be processed before this
        SExpr::Quote(_) | SExpr::Quasiquote(_) |
        SExpr::Unquote(_) | SExpr::UnquoteSplicing(_) => {
            Err(ConversionError::UnexpectedQuoteForm)
        }
    }
}
```

---

### CoreExpr - VM IR

```rust
//! CoreExpr - Intermediate representation for VM compilation
//!
//! Key changes from current design:
//! - Uses Datum instead of Rc<Value>
//! - No runtime concepts embedded

/// Core expression (desugared, ready for compilation)
#[derive(Debug, Clone)]
pub enum CoreExpr {
    /// Literal constant
    Literal(Datum),

    /// Quoted data
    Quote(Datum),

    /// Variable reference
    Var {
        name: Symbol,
        scopes: ScopeSet,  // For hygienic lookup
    },

    /// Lambda abstraction
    Lambda {
        params: Formals,
        body: Vec<CoreExpr>,
        binding_scope: Option<ScopeId>,
    },

    /// Conditional
    If {
        test: Box<CoreExpr>,
        then: Box<CoreExpr>,
        else_: Box<CoreExpr>,
    },

    /// Variable mutation
    Set {
        var: Symbol,
        scopes: ScopeSet,
        value: Box<CoreExpr>,
    },

    /// Definition
    Define {
        name: Symbol,
        value: Box<CoreExpr>,
    },

    /// Sequence
    Begin(Vec<CoreExpr>),

    /// Function application
    App {
        func: Box<CoreExpr>,
        args: Vec<CoreExpr>,
    },

    /// Import declaration
    Import {
        import_sets: Vec<Datum>,  // Was Vec<Value>
    },
}

/// Lambda parameter forms
#[derive(Debug, Clone, PartialEq)]
pub enum Formals {
    /// Fixed arity: (lambda (x y z) ...)
    Fixed(Vec<ScopedParam>),

    /// Variadic: (lambda args ...)
    Variadic(ScopedParam),

    /// Mixed: (lambda (x y . rest) ...)
    Mixed {
        fixed: Vec<ScopedParam>,
        rest: ScopedParam,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScopedParam {
    pub name: Symbol,
    pub scopes: ScopeSet,
}
```

---

### CpsExpr - CPS IR

```rust
//! CpsExpr - Continuation-passing style IR
//!
//! Used by tree-walker for proper tail calls and call/cc

pub type ContVar = Symbol;

/// CPS expression
#[derive(Debug, Clone)]
pub enum CpsExpr {
    // ========== Trivial Expressions ==========

    /// Literal constant
    Literal(Datum),

    /// Quoted data
    Quote(Datum),

    /// Variable reference
    Var {
        name: Symbol,
        scopes: ScopeSet,
    },

    /// CPS lambda (has explicit continuation parameter)
    CpsLambda {
        params: Formals,
        cont_param: ContVar,
        body: Box<CpsExpr>,
        binding_scope: Option<ScopeId>,
    },

    // ========== Serious Expressions ==========

    /// Let-bind a continuation
    LetCont {
        name: ContVar,
        params: Vec<ScopedParam>,
        body: Box<CpsExpr>,
        in_expr: Box<CpsExpr>,
    },

    /// Apply continuation
    AppCont {
        cont: ContVar,
        args: Vec<CpsExpr>,
    },

    /// Function application (passes continuation)
    App {
        func: Box<CpsExpr>,
        args: Vec<CpsExpr>,
        cont: ContVar,
    },

    /// Conditional
    If {
        test: Box<CpsExpr>,
        then_cont: ContVar,
        else_cont: ContVar,
    },

    /// Set and continue
    Set {
        var: Symbol,
        scopes: ScopeSet,
        value: Box<CpsExpr>,
        cont: ContVar,
    },

    /// Define and continue
    Define {
        name: Symbol,
        value: Box<CpsExpr>,
        cont: ContVar,
    },

    /// Sequence
    Begin {
        exprs: Vec<CpsExpr>,
        cont: ContVar,
    },

    // ========== Control Operations ==========

    /// Capture current continuation
    CallCC {
        func: Box<CpsExpr>,
        cont: ContVar,
    },

    /// Abort to prompt
    Abort {
        tag: Box<CpsExpr>,
        value: Box<CpsExpr>,
    },

    /// Install prompt
    PushPrompt {
        tag: Box<CpsExpr>,
        body: Box<CpsExpr>,
        handler: Box<CpsExpr>,
        cont: ContVar,
    },
}
```

---

### RtValue - Tree-Walker Runtime Values

```rust
//! RtValue - Runtime values for tree-walker
//!
//! This is what evaluation produces. Key characteristics:
//! - Mutable pairs, vectors, strings (for set-car!, vector-set!, etc.)
//! - Runtime-only concepts (Procedure, Continuation, Port, etc.)
//! - Uses Rc<RefCell<...>> for mutation

/// Runtime value (tree-walker)
#[derive(Debug, Clone)]
pub enum RtValue {
    // ========== Immediates ==========
    Integer(i64),
    Boolean(bool),
    Character(char),
    Null,
    Eof,
    Unspecified,

    // ========== Numbers ==========
    BigInteger(Rc<BigInt>),
    Rational(Rc<BigRational>),
    Real(f64),
    Complex(Box<(RtValue, RtValue)>),

    // ========== Data (from Datum, but mutable) ==========
    Symbol(Symbol),
    String(Rc<RefCell<String>>),       // Mutable!
    Pair(Rc<RefCell<(RtValue, RtValue)>>),  // Mutable!
    Vector(Rc<RefCell<Vec<RtValue>>>),      // Mutable!
    Bytevector(Rc<RefCell<Vec<u8>>>),       // Mutable!

    // ========== Runtime-Only ==========

    /// User-defined procedure (closure)
    Procedure(Rc<RtProcedure>),

    /// Primitive procedure
    Primitive(Rc<RtPrimitive>),

    /// First-class continuation
    Continuation(Rc<RtContinuation>),

    /// I/O port
    Port(Rc<RtPort>),

    /// Promise (lazy evaluation)
    Promise(Rc<RefCell<RtPromiseState>>),

    /// Parameter object
    Parameter(Rc<RtParameter>),

    /// Record type descriptor
    RecordType(Rc<RtRecordType>),

    /// Record instance
    Record {
        record_type: Rc<RtRecordType>,
        fields: Rc<RefCell<Vec<RtValue>>>,
    },

    /// Multiple values
    Values(Vec<RtValue>),

    /// Exception object
    Exception(Rc<RtException>),

    /// Library
    Library(Rc<RtLibrary>),

    /// Environment (for eval)
    Environment {
        env: Rc<Environment<RtValue>>,
        mutable: bool,
    },
}

/// Closure
pub struct RtProcedure {
    pub params: Formals,
    pub body: CpsExpr,
    pub env: Rc<Environment<RtValue>>,
    pub name: Option<Symbol>,
}

/// Primitive procedure
pub struct RtPrimitive {
    pub name: Symbol,
    pub func: fn(&[RtValue]) -> Result<RtValue, EvalError>,
    pub arity: Arity,
}

/// Captured continuation
pub struct RtContinuation {
    pub cont: CpsContinuation,
    pub dynamic_wind: Vec<WindEntry>,
}
```

### Datum → RtValue Conversion

```rust
/// Convert compile-time datum to runtime value
pub fn datum_to_rtvalue(datum: &Datum) -> RtValue {
    match datum {
        Datum::Integer(n) => RtValue::Integer(*n),
        Datum::BigInteger(n) => RtValue::BigInteger(n.clone()),
        Datum::Rational(r) => RtValue::Rational(r.clone()),
        Datum::Float(f) => RtValue::Real(*f),
        Datum::Complex(c) => {
            let real = datum_to_rtvalue(&c.0);
            let imag = datum_to_rtvalue(&c.1);
            RtValue::Complex(Box::new((real, imag)))
        }
        Datum::Boolean(b) => RtValue::Boolean(*b),
        Datum::Character(c) => RtValue::Character(*c),
        Datum::Null => RtValue::Null,

        Datum::String(s) => {
            // Convert immutable to mutable
            RtValue::String(Rc::new(RefCell::new(s.to_string())))
        }
        Datum::Symbol(s) => RtValue::Symbol(s.clone()),

        Datum::Pair(p) => {
            // Convert immutable pair to mutable
            let car = datum_to_rtvalue(&p.0);
            let cdr = datum_to_rtvalue(&p.1);
            RtValue::Pair(Rc::new(RefCell::new((car, cdr))))
        }

        Datum::Vector(v) => {
            let elements: Vec<_> = v.iter().map(datum_to_rtvalue).collect();
            RtValue::Vector(Rc::new(RefCell::new(elements)))
        }

        Datum::Bytevector(bv) => {
            RtValue::Bytevector(Rc::new(RefCell::new(bv.as_ref().clone())))
        }
    }
}
```

---

### TvValue - VM Runtime Values

```rust
//! TvValue - Tagged pointer values for VM
//!
//! 8-byte representation for maximum performance

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TvValue(u64);

impl TvValue {
    const TAG_BITS: u32 = 3;
    const TAG_MASK: u64 = 0b111;

    const TAG_FIXNUM: u64   = 0b000;
    const TAG_SPECIAL: u64  = 0b001;
    const TAG_CHAR: u64     = 0b010;
    const TAG_PAIR: u64     = 0b011;
    const TAG_VECTOR: u64   = 0b100;
    const TAG_STRING: u64   = 0b101;
    const TAG_CLOSURE: u64  = 0b110;
    const TAG_OBJECT: u64   = 0b111;

    pub const TRUE: Self        = Self(0x08 | Self::TAG_SPECIAL);
    pub const FALSE: Self       = Self(0x00 | Self::TAG_SPECIAL);
    pub const NULL: Self        = Self(0x10 | Self::TAG_SPECIAL);
    pub const EOF: Self         = Self(0x18 | Self::TAG_SPECIAL);
    pub const UNSPECIFIED: Self = Self(0x20 | Self::TAG_SPECIAL);

    // ... (see VM_VALUE_ARCHITECTURE.md for full implementation)
}

/// Convert datum to VM constant
pub fn datum_to_tvvalue(datum: &Datum, heap: &mut VmHeap) -> TvValue {
    match datum {
        Datum::Integer(n) if TvValue::fits_fixnum(*n) => TvValue::fixnum(*n),
        Datum::Integer(n) => heap.alloc_bigint(BigInt::from(*n)),
        Datum::Boolean(true) => TvValue::TRUE,
        Datum::Boolean(false) => TvValue::FALSE,
        Datum::Character(c) => TvValue::character(*c),
        Datum::Null => TvValue::NULL,
        Datum::Float(f) => TvValue::float(*f),

        Datum::Pair(p) => {
            let car = datum_to_tvvalue(&p.0, heap);
            let cdr = datum_to_tvvalue(&p.1, heap);
            heap.alloc_pair(car, cdr)
        }

        // ... other cases
    }
}
```

---

## Updated Pipeline

### Parser

```rust
// Before
impl Parser {
    pub fn parse(&mut self) -> Result<Value, ParseError>;
}

// After
impl Parser {
    pub fn parse(&mut self) -> Result<SExpr, ParseError>;
}
```

**Key simplification**: Lists become `Vec<SExpr>` instead of linked pairs.

```rust
fn parse_list(&mut self) -> Result<SExpr, ParseError> {
    self.expect(Token::LParen)?;
    let mut items = Vec::new();

    while !self.at(Token::RParen) {
        if self.at(Token::Dot) {
            // Improper list: (a b . c)
            self.advance();
            let tail = self.parse_expr()?;
            self.expect(Token::RParen)?;
            return Ok(SExpr::ImproperList {
                elements: items,
                tail: Box::new(tail),
            });
        }
        items.push(self.parse_expr()?);
    }

    self.expect(Token::RParen)?;

    if items.is_empty() {
        Ok(SExpr::Null)
    } else {
        Ok(SExpr::List(items))
    }
}
```

### Macro Expander

```rust
// Before: Works on Value with Rc<RefCell<...>>
fn expand(input: &Value, env: &MacroEnv) -> Result<Value, MacroError>;

// After: Works on immutable SExpr
fn expand(input: &SExpr, env: &MacroEnv) -> Result<SExpr, MacroError>;
```

**Key simplification**: Pattern matching is straightforward.

```rust
fn expand_let(items: &[SExpr], env: &MacroEnv) -> Result<SExpr, MacroError> {
    // items = [bindings, body...]
    // bindings is SExpr::List of binding pairs

    match &items[0] {
        SExpr::List(bindings) => {
            // Easy to iterate!
            for binding in bindings {
                match binding {
                    SExpr::List(pair) if pair.len() == 2 => {
                        let var = &pair[0];
                        let init = &pair[1];
                        // ...
                    }
                    _ => return Err(MacroError::InvalidBinding),
                }
            }
        }
        _ => return Err(MacroError::InvalidLetForm),
    }

    // Build output as SExpr
    Ok(SExpr::List(vec![
        SExpr::List(vec![
            SExpr::Symbol("lambda".into()),
            // ... params ...
            // ... body ...
        ]),
        // ... args ...
    ]))
}
```

### Desugarer

```rust
// SExpr → CoreExpr (for VM)
pub fn desugar_to_core(
    sexpr: &SExpr,
    env: &MacroEnv,
) -> Result<CoreExpr, DesugarError> {
    match sexpr {
        SExpr::Integer(n) => Ok(CoreExpr::Literal(Datum::Integer(*n))),
        SExpr::Boolean(b) => Ok(CoreExpr::Literal(Datum::Boolean(*b))),

        SExpr::Symbol(s) => Ok(CoreExpr::Var {
            name: s.clone(),
            scopes: ScopeSet::new(),
        }),

        SExpr::Identifier(id) => Ok(CoreExpr::Var {
            name: id.name.clone(),
            scopes: id.scopes.clone(),
        }),

        SExpr::Quote(inner) => {
            let datum = sexpr_to_datum(inner)?;
            Ok(CoreExpr::Quote(datum))
        }

        SExpr::List(items) if !items.is_empty() => {
            // Check for special forms
            if let Some(name) = items[0].symbol_name() {
                match name {
                    "lambda" => desugar_lambda(&items[1..], env),
                    "if" => desugar_if(&items[1..], env),
                    "define" => desugar_define(&items[1..], env),
                    "set!" => desugar_set(&items[1..], env),
                    "begin" => desugar_begin(&items[1..], env),
                    "quote" => {
                        let datum = sexpr_to_datum(&items[1])?;
                        Ok(CoreExpr::Quote(datum))
                    }
                    _ => {
                        // Check for macro, else application
                        if let Some(macro_def) = env.get_macro(name) {
                            let expanded = expand_macro(sexpr, macro_def)?;
                            desugar_to_core(&expanded, env)
                        } else {
                            desugar_app(items, env)
                        }
                    }
                }
            } else {
                desugar_app(items, env)
            }
        }

        SExpr::Null => Ok(CoreExpr::Literal(Datum::Null)),

        _ => Err(DesugarError::UnexpectedForm(format!("{:?}", sexpr))),
    }
}

// SExpr → CpsExpr (for tree-walker)
pub fn desugar_to_cps(
    sexpr: &SExpr,
    env: &MacroEnv,
) -> Result<CpsExpr, DesugarError> {
    // Similar structure, but produces CpsExpr
    // Could share logic with desugar_to_core via traits/generics
}
```

---

## Handling `read` and `eval`

### `read` - Returns Runtime Value

```rust
/// (read) primitive - parse from port, return runtime value
fn prim_read(args: &[RtValue]) -> Result<RtValue, EvalError> {
    let port = expect_input_port(&args[0])?;
    let source = port.read_datum()?;

    // Parse to SExpr
    let sexpr = Parser::new(&source).parse()?;

    // Convert to runtime value
    Ok(sexpr_to_rtvalue(&sexpr))
}

/// Convert syntax to runtime value (for read)
fn sexpr_to_rtvalue(sexpr: &SExpr) -> RtValue {
    match sexpr {
        SExpr::Integer(n) => RtValue::Integer(*n),
        SExpr::Symbol(s) => RtValue::Symbol(s.clone()),

        SExpr::List(items) => {
            // Convert to mutable pair chain
            let rtvalues: Vec<_> = items.iter().map(sexpr_to_rtvalue).collect();
            rtvalues.into_iter().rev().fold(RtValue::Null, |acc, item| {
                RtValue::Pair(Rc::new(RefCell::new((item, acc))))
            })
        }

        // ... other cases
    }
}
```

### `eval` - Takes Runtime Value, Returns Runtime Value

```rust
/// (eval expr env) primitive
fn prim_eval(args: &[RtValue]) -> Result<RtValue, EvalError> {
    let expr = &args[0];
    let env = expect_environment(&args[1])?;

    // Convert runtime value to syntax
    let sexpr = rtvalue_to_sexpr(expr)?;

    // Desugar and evaluate
    let cps = desugar_to_cps(&sexpr, &env.macro_env())?;
    evaluate_cps(&cps, &env)
}

/// Convert runtime value to syntax (for eval)
fn rtvalue_to_sexpr(value: &RtValue) -> Result<SExpr, ConversionError> {
    match value {
        RtValue::Integer(n) => Ok(SExpr::Integer(*n)),
        RtValue::Symbol(s) => Ok(SExpr::Symbol(s.clone())),
        RtValue::Null => Ok(SExpr::Null),

        RtValue::Pair(p) => {
            // Convert pair chain to list if proper
            let borrowed = p.borrow();
            let car = rtvalue_to_sexpr(&borrowed.0)?;

            match &borrowed.1 {
                RtValue::Null => Ok(SExpr::List(vec![car])),
                RtValue::Pair(_) => {
                    // Recursively collect into list
                    let mut items = vec![car];
                    collect_list(&borrowed.1, &mut items)?;
                    Ok(SExpr::List(items))
                }
                other => {
                    // Improper list
                    let tail = rtvalue_to_sexpr(other)?;
                    Ok(SExpr::ImproperList {
                        elements: vec![car],
                        tail: Box::new(tail),
                    })
                }
            }
        }

        // Runtime-only values can't become syntax
        RtValue::Procedure(_) => Err(ConversionError::NotDatum("procedure")),
        RtValue::Continuation(_) => Err(ConversionError::NotDatum("continuation")),
        RtValue::Port(_) => Err(ConversionError::NotDatum("port")),

        // ... other cases
    }
}
```

---

## Module Structure

```
patina-core/
├── lib.rs
├── sexpr.rs           # SExpr type
├── datum.rs           # Datum type
├── symbol.rs          # Symbol interning
├── scope.rs           # ScopeSet, ScopeId
├── core_expr.rs       # CoreExpr
├── cps_expr.rs        # CpsExpr
├── formals.rs         # Lambda parameter types
└── convert/
    ├── mod.rs
    ├── sexpr_to_datum.rs
    └── datum_display.rs

patina-frontend/
├── lexer/
├── parser/
│   └── mod.rs         # Produces SExpr
├── macro_expander/
│   ├── mod.rs         # SExpr → SExpr
│   ├── pattern.rs
│   └── template.rs
└── desugarer/
    ├── mod.rs
    ├── to_core.rs     # SExpr → CoreExpr
    └── to_cps.rs      # SExpr → CpsExpr

patina-tree-walker/
├── rt_value.rs        # RtValue type
├── environment.rs     # Environment<RtValue>
├── convert/
│   ├── datum_to_rt.rs
│   ├── sexpr_to_rt.rs
│   └── rt_to_sexpr.rs
├── eval/
│   ├── mod.rs
│   └── cps_eval.rs
└── primitives/
    └── *.rs           # fn(&[RtValue]) -> Result<RtValue, _>

patina-vm/
├── tv_value.rs        # TvValue type
├── heap.rs            # VmHeap
├── convert/
│   └── datum_to_tv.rs
├── compiler/
│   └── mod.rs         # CoreExpr → Bytecode
├── exec/
│   └── mod.rs
└── primitives/
    └── *.rs           # fn(&[TvValue], &mut Heap) -> TvValue
```

---

## Benefits Summary

| Aspect | Before | After |
|--------|--------|-------|
| Parser output | `Value::Pair(Rc<RefCell<...>>)` chains | `SExpr::List(Vec<...>)` |
| Pattern matching | Unwrap Rc, borrow RefCell | Direct match on Vec |
| Macro expansion | Builds Rc<RefCell> chains | Builds Vec |
| IR literals | `Rc<Value>` (can contain procedures) | `Datum` (data only) |
| Type clarity | `Value` means 3 things | Each type has one purpose |
| Backend flexibility | All share `Value` | Each has own runtime type |

---

## Migration Strategy

### Phase 1: Add New Types (Non-Breaking)

1. Add `SExpr` type alongside `Value`
2. Add `Datum` type
3. Add conversion functions
4. Keep existing code working

### Phase 2: Update Parser

1. Parser produces `SExpr`
2. Add `sexpr_to_value()` shim for compatibility
3. Existing code sees `Value` through shim

### Phase 3: Update Macro System

1. Macro expander works on `SExpr`
2. Pattern matching simplified
3. Template building simplified

### Phase 4: Update Desugarer

1. Input: `SExpr`
2. Output: `CoreExpr` with `Datum` (or `CpsExpr`)
3. Remove `Rc<Value>` from IRs

### Phase 5: Update Tree-Walker

1. Introduce `RtValue`
2. Update primitives
3. Update evaluator

### Phase 6: Build VM

1. Uses `TvValue` from start
2. Compiles `CoreExpr` with `Datum`
3. No `Value` dependency

---

## Estimated Effort

| Phase | Effort | Risk |
|-------|--------|------|
| Define SExpr, Datum | 1 week | Low |
| Update Parser | 1 week | Medium |
| Update Macro System | 1-2 weeks | Medium |
| Update Desugarer | 1 week | Medium |
| Introduce RtValue | 1-2 weeks | High |
| Update Tree-Walker | 1-2 weeks | High |
| Fix Tests | 1 week | Medium |
| **Total** | **7-10 weeks** | **Medium-High** |

---

## Comparison Summary

### All Architecture Options

| Approach | Effort | Conversions | Parser | Macro System | Tree-Walker | VM Runtime |
|----------|--------|-------------|--------|--------------|-------------|------------|
| Current (Value everywhere) | 0 | Implicit | Rc<RefCell> | Awkward | Value | N/A |
| Option A: Full SExpr Separation | 7-10 weeks | 3+ stages | Vec-based | Easy | RtValue | TvValue |
| Option B: Hybrid | 5-7 weeks | 1 (desugar) | Vec-based | Easy | TaggedValue | TaggedValue |
| Option C: Full Tagged (Chez) | 6-8 weeks | 0 | Tagged | On tagged | TaggedValue | TaggedValue |
| **Option D: Pragmatic (Selected)** | **4-6 weeks** | **At VM boundary** | **Unchanged** | **Unchanged** | **Value (unchanged)** | **TaggedValue** |

### Selected: Option D (Pragmatic Dual-Backend)

**Why Option D is the best choice for Patina right now:**

1. **Lowest risk**: Tree-walker completely unchanged, serves as reference
2. **Fastest to working VM**: No frontend changes required
3. **Pragmatic**: Accept some duplication, refactor later if needed
4. **Testable**: Can compare tree-walker vs VM output for same input
5. **Incremental**: VM developed independently without affecting existing code

**Trade-offs accepted:**
- Some code duplication between desugarers
- Two different value representations in codebase
- Future refactoring opportunity (can unify later)

### When to Choose Other Options

**Option A (Full Separation)** if:
- Maximum conceptual clarity is the priority
- You want to rewrite the parser anyway
- Clean separation matters more than speed

**Option B (Hybrid)** if:
- You want both backends to share TaggedValue
- You're willing to migrate tree-walker
- Long-term code sharing is important

**Option C (Full Tagged)** if:
- Maximum performance is critical (zero conversion)
- You're willing to rewrite the entire frontend
- Macro expansion on tagged values is acceptable

**Option D (Pragmatic - Selected)** if:
- You want the fastest path to a working VM
- Tree-walker stability is important
- You're okay with some duplication initially

---

## Open Questions

1. **Shared Symbol Table?**
   - Should `Symbol` be shared across all types?
   - Currently: Yes (just `Rc<str>`)

2. **SExpr for Debugging?**
   - Should we keep ability to convert RtValue/TvValue back to SExpr?
   - Needed for: `write`, `display`, error messages
   - Answer: Yes, via `rtvalue_to_sexpr()` / `tvvalue_to_sexpr()`

3. **Incremental Adoption?**
   - Can we add SExpr without breaking existing code?
   - Answer: Yes, via shim functions during migration

4. **Test Strategy?**
   - How to ensure correctness during migration?
   - Answer: Keep old tests working via shims, add new tests for new types

---

## References

### Patina Internal

- Current `Value` implementation: `patina-core/src/value.rs`
- Current `CoreExpr`: `patina-core/src/core_expr.rs`
- Current `CpsExpr`: `patina-core/src/cps_expr.rs`
- [VM_VALUE_ARCHITECTURE.md](./VM_VALUE_ARCHITECTURE.md) - Dual representation design
- [VM_SPECIFICATION.md](./VM_SPECIFICATION.md) - Full VM specification
- [TAGGED_POINTERS.md](./TAGGED_POINTERS.md) - Tagged pointer design

### Chez Scheme

- [Chez Scheme GitHub](https://github.com/cisco/ChezScheme) - Source code
- [IMPLEMENTATION.md](https://github.com/cisco/ChezScheme/blob/main/IMPLEMENTATION.md) - Internal architecture documentation
- [User's Guide - Syntax](https://www.scheme.com/csug8/syntax.html) - Syntax objects and annotations
- [User's Guide - Introduction](https://www.scheme.com/csug8/intro.html) - Overview of Chez/Petite architecture
- [Chez Scheme Wikipedia](https://en.wikipedia.org/wiki/Chez_Scheme) - Background and history

### Nanopass Framework

- [Nanopass Framework](https://nanopass.org/) - Official website
- [A Nanopass Framework for Commercial Compiler Development](https://andykeep.com/pubs/dissertation.pdf) - Andy Keep's dissertation (2013)
- [nanopass-framework-scheme](https://github.com/nanopass/nanopass-framework-scheme) - Reference implementation
